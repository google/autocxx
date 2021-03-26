// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use indoc::indoc;
use log::info;
use proc_macro2::{Span, TokenStream};
use quote::quote;
use std::fs::File;
use std::io::Write;
use std::panic::RefUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use syn::Token;
use tempfile::{tempdir, TempDir};
use test_env_log::test;

const KEEP_TEMPDIRS: bool = false;

lazy_static::lazy_static! {
    static ref BUILDER: Mutex<LinkableTryBuilder> = Mutex::new(LinkableTryBuilder::new());
}

/// TryBuild which maintains a directory of libraries to link.
/// This is desirable because otherwise, if we alter the RUSTFLAGS
/// then trybuild rebuilds *everything* including all the dev-dependencies.
/// This object exists purely so that we use the same RUSTFLAGS for every
/// test case.
struct LinkableTryBuilder {
    /// Directory in which we'll keep any linkable libraries
    temp_dir: TempDir,
}

impl LinkableTryBuilder {
    fn new() -> Self {
        LinkableTryBuilder {
            temp_dir: tempdir().unwrap(),
        }
    }

    fn move_items_into_temp_dir<P1: AsRef<Path>>(&self, src_path: &P1, pattern: &str) {
        for item in std::fs::read_dir(src_path).unwrap() {
            let item = item.unwrap();
            if item.file_name().into_string().unwrap().contains(pattern) {
                let dest = self.temp_dir.path().join(item.file_name());
                if dest.exists() {
                    std::fs::remove_file(&dest).unwrap();
                }
                std::fs::rename(item.path(), dest).unwrap();
            }
        }
    }

    fn build<P1: AsRef<Path>, P2: AsRef<Path>, P3: AsRef<Path> + RefUnwindSafe>(
        &self,
        library_path: &P1,
        library_name: &str,
        header_path: &P2,
        header_names: &[&str],
        rs_path: &P3,
        generated_rs_files: Vec<PathBuf>,
    ) -> std::thread::Result<()> {
        // Copy all items from the source dir into our temporary dir if their name matches
        // the pattern given in `library_name`.
        self.move_items_into_temp_dir(library_path, library_name);
        for header_name in header_names {
            self.move_items_into_temp_dir(header_path, header_name);
        }
        for generated_rs in generated_rs_files {
            self.move_items_into_temp_dir(
                &generated_rs.parent().unwrap().to_path_buf(),
                &generated_rs.file_name().unwrap().to_str().unwrap(),
            );
        }
        let temp_path = self.temp_dir.path().to_str().unwrap();
        std::env::set_var("RUSTFLAGS", format!("-L {}", temp_path));
        std::env::set_var("AUTOCXX_RS", temp_path);
        std::panic::catch_unwind(|| {
            let test_cases = trybuild::TestCases::new();
            test_cases.pass(rs_path)
        })
    }
}

fn write_to_file(tdir: &TempDir, filename: &str, content: &str) -> PathBuf {
    let path = tdir.path().join(filename);
    let mut f = File::create(&path).unwrap();
    f.write_all(content.as_bytes()).unwrap();
    path
}

/// A positive test, we expect to pass.
fn run_test(
    cxx_code: &str,
    header_code: &str,
    rust_code: TokenStream,
    generate: &[&str],
    generate_pods: &[&str],
) {
    do_run_test(
        cxx_code,
        header_code,
        rust_code,
        generate,
        generate_pods,
        None,
        &[],
    )
    .unwrap()
}

/// A positive test, we expect to pass.
fn run_test_ex(
    cxx_code: &str,
    header_code: &str,
    rust_code: TokenStream,
    generate: &[&str],
    generate_pods: &[&str],
    extra_directives: Option<TokenStream>,
    defines: &[&str],
) {
    do_run_test(
        cxx_code,
        header_code,
        rust_code,
        generate,
        generate_pods,
        extra_directives,
        defines,
    )
    .unwrap()
}

fn run_test_expect_fail(
    cxx_code: &str,
    header_code: &str,
    rust_code: TokenStream,
    generate: &[&str],
    generate_pods: &[&str],
) {
    do_run_test(
        cxx_code,
        header_code,
        rust_code,
        generate,
        generate_pods,
        None,
        &[],
    )
    .expect_err("Unexpected success");
}

/// In the future maybe the tests will distinguish the exact type of failure expected.
#[derive(Debug)]
enum TestError {
    AutoCxx(crate::BuilderError),
    CppBuild(cc::Error),
    RsBuild,
}

fn do_run_test(
    cxx_code: &str,
    header_code: &str,
    rust_code: TokenStream,
    generate: &[&str],
    generate_pods: &[&str],
    extra_directives: Option<TokenStream>,
    definitions: &[&str],
) -> Result<(), TestError> {
    // Step 1: Write the C++ header snippet to a temp file
    let tdir = tempdir().unwrap();
    write_to_file(&tdir, "input.h", &format!("#pragma once\n{}", header_code));
    write_to_file(&tdir, "cxx.h", crate::HEADER);

    // Step 2: Expand the snippet of Rust code into an entire
    //         program including include_cxx!
    let generate = generate.iter().map(|s| {
        quote! {
            generate!(#s)
        }
    });
    let generate_pods = generate_pods.iter().map(|s| {
        quote! {
            generate_pod!(#s)
        }
    });

    let hexathorpe = Token![#](Span::call_site());
    let unexpanded_rust = quote! {
        use autocxx::include_cpp;

        include_cpp!(
            #hexathorpe include "input.h"
            safety!(unsafe_ffi)
            #(#generate)*
            #(#generate_pods)*
            #extra_directives
        );

        fn main() {
            #rust_code
        }

        #[link(name="autocxx-demo")]
        extern {}
    };
    info!("Unexpanded Rust: {}", unexpanded_rust);

    let write_rust_to_file = |ts: &TokenStream| -> PathBuf {
        // Step 3: Write the Rust code to a temp file
        let rs_code = format!("{}", ts);
        write_to_file(&tdir, "input.rs", &rs_code)
    };

    let target_dir = tdir.path().join("target");
    std::fs::create_dir(&target_dir).unwrap();

    let rs_path = write_rust_to_file(&unexpanded_rust);

    info!("Path is {:?}", tdir.path());
    let build_results = crate::builder::build_to_custom_directory(
        &rs_path,
        &[tdir.path()],
        &definitions,
        Some(target_dir.clone()),
        None,
    )
    .map_err(TestError::AutoCxx)?;
    let mut b = build_results.0;
    let generated_rs_files = build_results.1;

    let target = rust_info::get().target_triple.unwrap();

    if !cxx_code.is_empty() {
        // Step 4: Write the C++ code snippet to a .cc file, along with a #include
        //         of the header emitted in step 5.
        let cxx_code = format!("#include \"{}\"\n{}", "input.h", cxx_code);
        let cxx_path = write_to_file(&tdir, "input.cxx", &cxx_code);
        b.file(cxx_path);
    }

    for d in definitions {
        let mut it = d.split("=");
        let k = it.next().unwrap();
        let v = it.next();
        b.define(k, v);
    }

    b.out_dir(&target_dir)
        .host(&target)
        .target(&target)
        .opt_level(1)
        .flag("-std=c++14")
        .include(tdir.path())
        .try_compile("autocxx-demo")
        .map_err(TestError::CppBuild)?;
    // Step 8: use the trybuild crate to build the Rust file.
    let r = BUILDER.lock().unwrap().build(
        &target_dir,
        "autocxx-demo",
        &tdir.path(),
        &["input.h", "cxx.h"],
        &rs_path,
        generated_rs_files,
    );
    if r.is_err() {
        return Err(TestError::RsBuild); // details of Rust panic are a bit messy to include, and
                                        // not important at the moment.
    }
    if KEEP_TEMPDIRS {
        println!("Tempdir: {:?}", tdir.into_path().to_str());
    }
    Ok(())
}

#[test]
fn test_return_void() {
    let cxx = indoc! {"
        void do_nothing() {
        }
    "};
    let hdr = indoc! {"
        void do_nothing();
    "};
    let rs = quote! {
        ffi::do_nothing();
    };
    run_test(cxx, hdr, rs, &["do_nothing"], &[]);
}

#[test]
fn test_two_funcs() {
    let cxx = indoc! {"
        void do_nothing1() {
        }
        void do_nothing2() {
        }
    "};
    let hdr = indoc! {"
        void do_nothing1();
        void do_nothing2();
    "};
    let rs = quote! {
        ffi::do_nothing1();
        ffi::do_nothing2();
    };
    run_test(cxx, hdr, rs, &["do_nothing1", "do_nothing2"], &[]);
}

#[test]
fn test_two_funcs_with_definition() {
    // Test to ensure C++ header isn't included twice
    let cxx = indoc! {"
        void do_nothing1() {
        }
        void do_nothing2() {
        }
    "};
    let hdr = indoc! {"
        struct Bob {
            int a;
        };
        void do_nothing1();
        void do_nothing2();
    "};
    let rs = quote! {
        ffi::do_nothing1();
        ffi::do_nothing2();
    };
    println!("Here");

    info!("Here2");
    run_test(cxx, hdr, rs, &["do_nothing1", "do_nothing2"], &[]);
}

#[test]
fn test_return_i32() {
    let cxx = indoc! {"
        uint32_t give_int() {
            return 5;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        uint32_t give_int();
    "};
    let rs = quote! {
        assert_eq!(ffi::give_int(), 5);
    };
    run_test(cxx, hdr, rs, &["give_int"], &[]);
}

#[test]
fn test_take_i32() {
    let cxx = indoc! {"
        uint32_t take_int(uint32_t a) {
            return a + 3;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        uint32_t take_int(uint32_t a);
    "};
    let rs = quote! {
        assert_eq!(ffi::take_int(3), 6);
    };
    run_test(cxx, hdr, rs, &["take_int"], &[]);
}

#[test]
#[ignore] // because cxx doesn't support unique_ptrs to primitives.
fn test_give_up_int() {
    let cxx = indoc! {"
        std::unique_ptr<uint32_t> give_up() {
            return std::make_unique<uint32_t>(12);
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        std::unique_ptr<uint32_t> give_up();
    "};
    let rs = quote! {
        assert_eq!(ffi::give_up().as_ref().unwrap(), 12);
    };
    run_test(cxx, hdr, rs, &["give_up"], &[]);
}

#[test]
#[ignore] // because we don't yet implement UniquePtr etc. for autocxx::c_int and friends
fn test_give_up_ctype() {
    let cxx = indoc! {"
        std::unique_ptr<int> give_up() {
            return std::make_unique<int>(12);
        }
    "};
    let hdr = indoc! {"
        #include <memory>
        std::unique_ptr<int> give_up();
    "};
    let rs = quote! {
        assert_eq!(ffi::give_up().as_ref().unwrap(), autocxx::c_int(12));
    };
    run_test(cxx, hdr, rs, &["give_up"], &[]);
}

#[test]
fn test_give_string_up() {
    let cxx = indoc! {"
        std::unique_ptr<std::string> give_str_up() {
            return std::make_unique<std::string>(\"Bob\");
        }
    "};
    let hdr = indoc! {"
        #include <memory>
        #include <string>
        std::unique_ptr<std::string> give_str_up();
    "};
    let rs = quote! {
        assert_eq!(ffi::give_str_up().as_ref().unwrap().to_str().unwrap(), "Bob");
    };
    run_test(cxx, hdr, rs, &["give_str_up"], &[]);
}

#[test]
fn test_give_string_plain() {
    let cxx = indoc! {"
        std::string give_str() {
            return std::string(\"Bob\");
        }
    "};
    let hdr = indoc! {"
        #include <string>
        std::string give_str();
    "};
    let rs = quote! {
        assert_eq!(ffi::give_str().as_ref().unwrap(), "Bob");
    };
    run_test(cxx, hdr, rs, &["give_str"], &[]);
}

#[test]
fn test_cycle_string_up() {
    let cxx = indoc! {"
        std::unique_ptr<std::string> give_str_up() {
            return std::make_unique<std::string>(\"Bob\");
        }
        uint32_t take_str_up(std::unique_ptr<std::string> a) {
            return a->length();
        }
    "};
    let hdr = indoc! {"
        #include <memory>
        #include <string>
        #include <cstdint>
        std::unique_ptr<std::string> give_str_up();
        uint32_t take_str_up(std::unique_ptr<std::string> a);
    "};
    let rs = quote! {
        let s = ffi::give_str_up();
        assert_eq!(ffi::take_str_up(s), 3);
    };
    run_test(cxx, hdr, rs, &["give_str_up", "take_str_up"], &[]);
}

#[test]
fn test_cycle_string() {
    let cxx = indoc! {"
        std::string give_str() {
            return std::string(\"Bob\");
        }
        uint32_t take_str(std::string a) {
            return a.length();
        }
    "};
    let hdr = indoc! {"
        #include <string>
        #include <cstdint>
        std::string give_str();
        uint32_t take_str(std::string a);
    "};
    let rs = quote! {
        let s = ffi::give_str();
        assert_eq!(ffi::take_str(s), 3);
    };
    let generate = &["give_str", "take_str"];
    run_test(cxx, hdr, rs, generate, &[]);
}

#[test]
fn test_cycle_string_by_ref() {
    let cxx = indoc! {"
        std::unique_ptr<std::string> give_str() {
            return std::make_unique<std::string>(\"Bob\");
        }
        uint32_t take_str(const std::string& a) {
            return a.length();
        }
    "};
    let hdr = indoc! {"
        #include <string>
        #include <memory>
        #include <cstdint>
        std::unique_ptr<std::string> give_str();
        uint32_t take_str(const std::string& a);
    "};
    let rs = quote! {
        let s = ffi::give_str();
        assert_eq!(ffi::take_str(s.as_ref().unwrap()), 3);
    };
    let generate = &["give_str", "take_str"];
    run_test(cxx, hdr, rs, generate, &[]);
}

#[test]
fn test_cycle_string_by_mut_ref() {
    let cxx = indoc! {"
        std::unique_ptr<std::string> give_str() {
            return std::make_unique<std::string>(\"Bob\");
        }
        uint32_t take_str(std::string& a) {
            return a.length();
        }
    "};
    let hdr = indoc! {"
        #include <string>
        #include <memory>
        #include <cstdint>
        std::unique_ptr<std::string> give_str();
        uint32_t take_str(std::string& a);
    "};
    let rs = quote! {
        let mut s = ffi::give_str();
        assert_eq!(ffi::take_str(s.as_mut().unwrap()), 3);
    };
    let generate = &["give_str", "take_str"];
    run_test(cxx, hdr, rs, generate, &[]);
}

#[test]
fn test_give_pod_by_value() {
    let cxx = indoc! {"
        Bob give_bob() {
            Bob a;
            a.a = 3;
            a.b = 4;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        Bob give_bob();
    "};
    let rs = quote! {
        assert_eq!(ffi::give_bob().b, 4);
    };
    run_test(cxx, hdr, rs, &["give_bob"], &["Bob"]);
}

#[test]
fn test_give_pod_class_by_value() {
    let cxx = indoc! {"
        Bob give_bob() {
            Bob a;
            a.a = 3;
            a.b = 4;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        class Bob {
        public:
            uint32_t a;
            uint32_t b;
        };
        Bob give_bob();
    "};
    let rs = quote! {
        assert_eq!(ffi::give_bob().b, 4);
    };
    run_test(cxx, hdr, rs, &["give_bob"], &["Bob"]);
}

#[test]
fn test_give_pod_by_up() {
    let cxx = indoc! {"
        std::unique_ptr<Bob> give_bob() {
            auto a = std::make_unique<Bob>();
            a->a = 3;
            a->b = 4;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        std::unique_ptr<Bob> give_bob();
    "};
    let rs = quote! {
        assert_eq!(ffi::give_bob().as_ref().unwrap().b, 4);
    };
    run_test(cxx, hdr, rs, &["give_bob"], &["Bob"]);
}

#[test]
fn test_take_pod_by_value() {
    let cxx = indoc! {"
        uint32_t take_bob(Bob a) {
            return a.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        uint32_t take_bob(Bob a);
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 12, b: 13 };
        assert_eq!(ffi::take_bob(a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob"], &["Bob"]);
}

#[test]
fn test_take_pod_by_ref() {
    let cxx = indoc! {"
        uint32_t take_bob(const Bob& a) {
            return a.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        uint32_t take_bob(const Bob& a);
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 12, b: 13 };
        assert_eq!(ffi::take_bob(&a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob"], &["Bob"]);
}

#[test]
fn test_take_pod_by_ref_and_ptr() {
    let cxx = indoc! {"
        uint32_t take_bob_ref(const Bob& a) {
            return a.a;
        }
        uint32_t take_bob_ptr(const Bob* a) {
            return a->a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        uint32_t take_bob_ref(const Bob& a);
        uint32_t take_bob_ptr(const Bob* a);
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 12, b: 13 };
        assert_eq!(ffi::take_bob_ref(&a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob_ref", "take_bob_ptr"], &["Bob"]);
}

#[test]
fn test_return_pod_by_ref_and_ptr() {
    let hdr = indoc! {"
        #include <cstdint>
        struct B {
            uint32_t a;
        };
        struct A {
            B b;
        };
        inline const B& return_b_ref(const A& a) {
            return a.b;
        }
        inline const B* return_b_ptr(const A& a) {
            return &a.b;
        }
    "};
    let rs = quote! {
        let a = ffi::A { b: ffi::B { a: 3 } };
        assert_eq!(ffi::return_b_ref(&a).a, 3);
        let b_ptr = ffi::return_b_ptr(&a);
        assert_eq!(unsafe { b_ptr.as_ref() }.unwrap().a, 3);
    };
    run_test("", hdr, rs, &["return_b_ref", "return_b_ptr"], &["A", "B"]);
}

#[test]
fn test_take_pod_by_mut_ref() {
    let cxx = indoc! {"
        uint32_t take_bob(Bob& a) {
            a.b = 14;
            return a.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        uint32_t take_bob(Bob& a);
    "};
    let rs = quote! {
        let mut a = Box::pin(ffi::Bob { a: 12, b: 13 });
        assert_eq!(ffi::take_bob(a.as_mut()), 12);
        assert_eq!(a.b, 14);
    };
    run_test(cxx, hdr, rs, &["take_bob"], &["Bob"]);
}

#[test]
fn test_take_nested_pod_by_value() {
    let cxx = indoc! {"
        uint32_t take_bob(Bob a) {
            return a.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Phil {
            uint32_t d;
        };
        struct Bob {
            uint32_t a;
            uint32_t b;
            Phil c;
        };
        uint32_t take_bob(Bob a);
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 12, b: 13, c: ffi::Phil { d: 4 } };
        assert_eq!(ffi::take_bob(a), 12);
    };
    // Should be no need to allowlist Phil below
    run_test(cxx, hdr, rs, &["take_bob"], &["Bob"]);
}

#[test]
fn test_take_nonpod_by_value() {
    let cxx = indoc! {"
        Bob::Bob(uint32_t a0, uint32_t b0)
           : a(a0), b(b0) {}
        uint32_t take_bob(Bob a) {
            return a.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Bob {
            Bob(uint32_t a, uint32_t b);
            uint32_t a;
            uint32_t b;
            std::string reason_why_this_is_nonpod;
        };
        uint32_t take_bob(Bob a);
    "};
    let rs = quote! {
        let a = ffi::Bob::make_unique(12, 13);
        assert_eq!(ffi::take_bob(a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob", "Bob"], &[]);
}

#[test]
fn test_take_nonpod_by_ref() {
    let cxx = indoc! {"
        uint32_t take_bob(const Bob& a) {
            return a.a;
        }
        std::unique_ptr<Bob> make_bob(uint32_t a) {
            auto b = std::make_unique<Bob>();
            b->a = a;
            return b;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        std::unique_ptr<Bob> make_bob(uint32_t a);
        uint32_t take_bob(const Bob& a);
    "};
    let rs = quote! {
        let a = ffi::make_bob(12);
        assert_eq!(ffi::take_bob(&a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob", "Bob", "make_bob"], &[]);
}

#[test]
fn test_take_nonpod_by_ptr_simple() {
    let cxx = indoc! {"
        uint32_t take_bob(const Bob* a) {
            return a->a;
        }
        std::unique_ptr<Bob> make_bob(uint32_t a) {
            auto b = std::make_unique<Bob>();
            b->a = a;
            return b;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        std::unique_ptr<Bob> make_bob(uint32_t a);
        uint32_t take_bob(const Bob* a);
    "};
    let rs = quote! {
        let a = ffi::make_bob(12);
        let a_ptr = a.into_raw();
        assert_eq!(unsafe { ffi::take_bob(a_ptr) }, 12);
        unsafe { cxx::UniquePtr::from_raw(a_ptr) }; // so we drop
    };
    run_test(cxx, hdr, rs, &["take_bob", "Bob", "make_bob"], &[]);
}

#[test]
fn test_take_nonpod_by_ptr_in_method() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        #include <cstdint>
        class A {
        public:
            A() {};
            uint32_t take_bob(const Bob* a) const {
                return a->a;
            }
            std::unique_ptr<Bob> make_bob(uint32_t a) const {
                auto b = std::make_unique<Bob>();
                b->a = a;
                return b;
            }
            uint16_t a;
        };

    "};
    let rs = quote! {
        let a = ffi::A::make_unique();
        let b = a.as_ref().unwrap().make_bob(12);
        let b_ptr = b.into_raw();
        assert_eq!(unsafe { a.as_ref().unwrap().take_bob(b_ptr) }, 12);
        unsafe { cxx::UniquePtr::from_raw(b_ptr) }; // so we drop
    };
    run_test("", hdr, rs, &["A", "Bob"], &[]);
}

#[test]
fn test_take_nonpod_by_ptr_in_wrapped_method() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct C {
            C() {}
            uint32_t a;
        };
        struct Bob {
            uint32_t a;
        };
        class A {
        public:
            A() {};
            uint32_t take_bob(const Bob* a, C) const {
                return a->a;
            }
            std::unique_ptr<Bob> make_bob(uint32_t a) const {
                auto b = std::make_unique<Bob>();
                b->a = a;
                return b;
            }
            uint16_t a;
        };

    "};
    let rs = quote! {
        let a = ffi::A::make_unique();
        let c = ffi::C::make_unique();
        let b = a.as_ref().unwrap().make_bob(12);
        let b_ptr = b.into_raw();
        assert_eq!(unsafe { a.as_ref().unwrap().take_bob(b_ptr, c) }, 12);
        unsafe { cxx::UniquePtr::from_raw(b_ptr) }; // so we drop
    };
    run_test("", hdr, rs, &["A", "Bob", "C"], &[]);
}

#[test]
fn test_take_char_by_ptr_in_wrapped_method() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct C {
            C() { test = \"hi\"; }
            uint32_t a;
            char* test;
        };
        class A {
        public:
            A() {};
            uint32_t take_char(const char* a, C) const {
                return a[0];
            }
            const char* make_char(C extra) const {
                return extra.test;
            }
            uint16_t a;
        };

    "};
    let rs = quote! {
        let a = ffi::A::make_unique();
        let c1 = ffi::C::make_unique();
        let c2 = ffi::C::make_unique();
        let ch = a.as_ref().unwrap().make_char(c1);
        assert_eq!(unsafe { ch.as_ref()}.unwrap(), &104i8);
        assert_eq!(unsafe { a.as_ref().unwrap().take_char(ch, c2) }, 104);
    };
    run_test("", hdr, rs, &["A", "C"], &[]);
}

#[test]
fn test_take_nonpod_by_mut_ref() {
    let cxx = indoc! {"
        uint32_t take_bob(Bob& a) {
            return a.a;
        }
        std::unique_ptr<Bob> make_bob(uint32_t a) {
            auto b = std::make_unique<Bob>();
            b->a = a;
            return b;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        std::unique_ptr<Bob> make_bob(uint32_t a);
        uint32_t take_bob(Bob& a);
    "};
    let rs = quote! {
        let mut a = ffi::make_bob(12);
        assert_eq!(ffi::take_bob(a.pin_mut()), 12);
    };
    // TODO confirm that the object really was mutated by C++ in this
    // and similar tests.
    run_test(cxx, hdr, rs, &["take_bob", "Bob", "make_bob"], &[]);
}

#[test]
fn test_return_nonpod_by_value() {
    let cxx = indoc! {"
        Bob::Bob(uint32_t a0, uint32_t b0)
           : a(a0), b(b0) {}
        Bob give_bob(uint32_t a) {
            Bob c(a, 44);
            return c;
        }
        uint32_t take_bob(std::unique_ptr<Bob> a) {
            return a->a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            Bob(uint32_t a, uint32_t b);
            uint32_t a;
            uint32_t b;
        };
        Bob give_bob(uint32_t a);
        uint32_t take_bob(std::unique_ptr<Bob> a);
    "};
    let rs = quote! {
        let a = ffi::give_bob(13);
        assert_eq!(ffi::take_bob(a), 13);
    };
    run_test(cxx, hdr, rs, &["take_bob", "give_bob", "Bob"], &[]);
}

#[test]
fn test_get_str_by_up() {
    let cxx = indoc! {"
    std::unique_ptr<std::string> get_str() {
            return std::make_unique<std::string>(\"hello\");
        }
    "};
    let hdr = indoc! {"
        #include <string>
        #include <memory>
        std::unique_ptr<std::string> get_str();
    "};
    let rs = quote! {
        assert_eq!(ffi::get_str().as_ref().unwrap(), "hello");
    };
    run_test(cxx, hdr, rs, &["get_str"], &[]);
}

#[test]
fn test_get_str_by_value() {
    let cxx = indoc! {"
        std::string get_str() {
            return \"hello\";
        }
    "};
    let hdr = indoc! {"
        #include <string>
        std::string get_str();
    "};
    let rs = quote! {
        assert_eq!(ffi::get_str().as_ref().unwrap(), "hello");
    };
    run_test(cxx, hdr, rs, &["get_str"], &[]);
}

#[test]
fn test_cycle_nonpod_with_str_by_ref() {
    let cxx = indoc! {"
        uint32_t take_bob(const Bob& a) {
            return a.a;
        }
        std::unique_ptr<Bob> make_bob() {
            auto a = std::make_unique<Bob>();
            a->a = 32;
            a->b = \"hello\";
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        #include <memory>
        struct Bob {
            uint32_t a;
            std::string b;
        };
        uint32_t take_bob(const Bob& a);
        std::unique_ptr<Bob> make_bob();
    "};
    let rs = quote! {
        let a = ffi::make_bob();
        assert_eq!(ffi::take_bob(a.as_ref().unwrap()), 32);
    };
    run_test(cxx, hdr, rs, &["take_bob", "Bob", "make_bob"], &[]);
}

#[test]
fn test_make_up() {
    let cxx = indoc! {"
        Bob::Bob() : a(3) {
        }
        uint32_t take_bob(const Bob& a) {
            return a.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        class Bob {
        public:
            Bob();
            uint32_t a;
        };
        uint32_t take_bob(const Bob& a);
    "};
    let rs = quote! {
        let a = ffi::Bob::make_unique(); // TODO test with all sorts of arguments.
        assert_eq!(ffi::take_bob(a.as_ref().unwrap()), 3);
    };
    run_test(cxx, hdr, rs, &["Bob", "take_bob"], &[]);
}

#[test]
fn test_make_up_with_args() {
    let cxx = indoc! {"
        Bob::Bob(uint32_t a0, uint32_t b0)
           : a(a0), b(b0) {}
        uint32_t take_bob(const Bob& a) {
            return a.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            Bob(uint32_t a, uint32_t b);
            uint32_t a;
            uint32_t b;
        };
        uint32_t take_bob(const Bob& a);
    "};
    let rs = quote! {
        let a = ffi::Bob::make_unique(12, 13);
        assert_eq!(ffi::take_bob(a.as_ref().unwrap()), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob", "Bob"], &[]);
}

#[test]
#[ignore] // because we don't support unique_ptrs to primitives
fn test_make_up_int() {
    let cxx = indoc! {"
        Bob::Bob(uint32_t a) : b(a) {
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        class Bob {
        public:
            Bob(uint32_t a);
            uint32_t b;
        };
    "};
    let rs = quote! {
        let a = ffi::Bob::make_unique(3);
        assert_eq!(a.as_ref().unwrap().b, 3);
    };
    run_test(cxx, hdr, rs, &["Bob"], &[]);
}

#[test]
fn test_enum_with_funcs() {
    let cxx = indoc! {"
        Bob give_bob() {
            return Bob::BOB_VALUE_2;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        enum Bob {
            BOB_VALUE_1,
            BOB_VALUE_2,
        };
        Bob give_bob();
    "};
    let rs = quote! {
        let a = ffi::Bob::BOB_VALUE_2;
        let b = ffi::give_bob();
        assert!(a == b);
    };
    run_test(cxx, hdr, rs, &["Bob", "give_bob"], &[]);
}

#[test]
fn test_enum_no_funcs() {
    let cxx = indoc! {"
    "};
    let hdr = indoc! {"
        enum Bob {
            BOB_VALUE_1,
            BOB_VALUE_2,
        };
    "};
    let rs = quote! {
        let a = ffi::Bob::BOB_VALUE_1;
        let b = ffi::Bob::BOB_VALUE_2;
        assert!(a != b);
    };
    run_test(cxx, hdr, rs, &["Bob"], &[]);
}

#[test] // works, but causes compile warnings
fn test_take_pod_class_by_value() {
    let cxx = indoc! {"
        uint32_t take_bob(Bob a) {
            return a.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        class Bob {
        public:
            uint32_t a;
            uint32_t b;
        };
        uint32_t take_bob(Bob a);
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 12, b: 13 };
        assert_eq!(ffi::take_bob(a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob"], &["Bob"]);
}

#[test]
fn test_pod_method() {
    let cxx = indoc! {"
        uint32_t Bob::get_bob() const {
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob() const;
        };
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 12, b: 13 };
        assert_eq!(a.get_bob(), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob"], &["Bob"]);
}

#[test]
fn test_pod_mut_method() {
    let cxx = indoc! {"
        uint32_t Bob::get_bob() {
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob();
        };
    "};
    let rs = quote! {
        let mut a = Box::pin(ffi::Bob { a: 12, b: 13 });
        assert_eq!(a.as_mut().get_bob(), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob"], &["Bob"]);
}

#[test]
fn test_define_int() {
    let cxx = indoc! {"
    "};
    let hdr = indoc! {"
        #define BOB 3
    "};
    let rs = quote! {
        assert_eq!(ffi::BOB, 3);
    };
    run_test(cxx, hdr, rs, &["BOB"], &[]);
}

#[test]
fn test_define_str() {
    let cxx = indoc! {"
    "};
    let hdr = indoc! {"
        #define BOB \"foo\"
    "};
    let rs = quote! {
        assert_eq!(std::str::from_utf8(ffi::BOB).unwrap().trim_end_matches(char::from(0)), "foo");
    };
    run_test(cxx, hdr, rs, &["BOB"], &[]);
}

#[test]
fn test_i32_const() {
    let cxx = indoc! {"
    "};
    let hdr = indoc! {"
        #include <cstdint>  
        const uint32_t BOB = 3;
    "};
    let rs = quote! {
        assert_eq!(ffi::BOB, 3);
    };
    run_test(cxx, hdr, rs, &["BOB"], &[]);
}

#[test]
fn test_negative_rs_nonsense() {
    // Really just testing the test infrastructure.
    let cxx = indoc! {"
    "};
    let hdr = indoc! {"
        #include <cstdint>  
        const uint32_t BOB = 3;
    "};
    let rs = quote! {
        foo bar
    };
    run_test_expect_fail(cxx, hdr, rs, &["BOB"], &[]);
}

#[test]
fn test_negative_cpp_nonsense() {
    // Really just testing the test infrastructure.
    let cxx = indoc! {"
    "};
    let hdr = indoc! {"
        #include <cstdint>  
        const uint32_t BOB = CAT;
    "};
    let rs = quote! {
        assert_eq!(ffi::BOB, 3);
    };
    run_test_expect_fail(cxx, hdr, rs, &["BOB"], &[]);
}

#[test]
fn test_negative_make_nonpod() {
    let cxx = indoc! {"
        uint32_t take_bob(const Bob& a) {
            return a.a;
        }
        std::unique_ptr<Bob> make_bob(uint32_t a) {
            auto b = std::make_unique<Bob>();
            b->a = a;
            return b;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        std::unique_ptr<Bob> make_bob(uint32_t a);
        uint32_t take_bob(const Bob& a);
    "};
    let rs = quote! {
        ffi::Bob {};
    };
    let rs2 = quote! {
        ffi::Bob { a: 12 };
    };
    let rs3 = quote! {
        ffi::Bob { do_not_attempt_to_allocate_nonpod_types: [] };
    };
    run_test_expect_fail(cxx, hdr, rs, &["take_bob", "Bob", "make_bob"], &[]);
    run_test_expect_fail(cxx, hdr, rs2, &["take_bob", "Bob", "make_bob"], &[]);
    run_test_expect_fail(cxx, hdr, rs3, &["take_bob", "Bob", "make_bob"], &[]);
}

#[test]
fn test_method_pass_pod_by_value() {
    let cxx = indoc! {"
        uint32_t Bob::get_bob(Anna) const {
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Anna {
            uint32_t a;
        };
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob(Anna a) const;
        };
    "};
    let rs = quote! {
        let a = ffi::Anna { a: 14 };
        let b = ffi::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob"], &["Bob", "Anna"]);
}

#[test]
fn test_inline_method() {
    let hdr = indoc! {"
        #include <cstdint>
        struct Anna {
            uint32_t a;
        };
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob(Anna) const {
                return a;
            }
        };
    "};
    let rs = quote! {
        let a = ffi::Anna { a: 14 };
        let b = ffi::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(a), 12);
    };
    run_test("", hdr, rs, &["take_bob"], &["Bob", "Anna"]);
}

#[test]
fn test_method_pass_pod_by_reference() {
    let cxx = indoc! {"
        uint32_t Bob::get_bob(const Anna&) const {
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Anna {
            uint32_t a;
        };
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob(const Anna& a) const;
        };
    "};
    let rs = quote! {
        let a = ffi::Anna { a: 14 };
        let b = ffi::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(&a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob"], &["Bob", "Anna"]);
}

#[test]
fn test_method_pass_pod_by_mut_reference() {
    let cxx = indoc! {"
        uint32_t Bob::get_bob(Anna&) const {
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Anna {
            uint32_t a;
        };
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob(Anna& a) const;
        };
    "};
    let rs = quote! {
        let mut a = Box::pin(ffi::Anna { a: 14 });
        let b = ffi::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(a.as_mut()), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob"], &["Bob", "Anna"]);
}

#[test]
fn test_method_pass_pod_by_up() {
    let cxx = indoc! {"
        uint32_t Bob::get_bob(std::unique_ptr<Anna>) const {
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Anna {
            uint32_t a;
        };
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob(std::unique_ptr<Anna> z) const;
        };
    "};
    let rs = quote! {
        let a = ffi::Anna { a: 14 };
        let b = ffi::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(cxx::UniquePtr::new(a)), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob"], &["Bob", "Anna"]);
}

#[test]
fn test_method_pass_nonpod_by_value() {
    let cxx = indoc! {"
        uint32_t Bob::get_bob(Anna) const {
            return a;
        }
        Anna give_anna() {
            Anna a;
            a.a = 10;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Anna {
            uint32_t a;
            std::string b;
        };
        Anna give_anna();
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob(Anna a) const;
        };
    "};
    let rs = quote! {
        let a = ffi::give_anna();
        let b = ffi::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(a), 12);
    };
    run_test(
        cxx,
        hdr,
        rs,
        &["take_bob", "Anna", "give_anna", "get_bob"],
        &["Bob"],
    );
}

#[test]
fn test_method_pass_nonpod_by_value_with_up() {
    // Checks that existing UniquePtr params are not wrecked
    // by the conversion we do here.
    let cxx = indoc! {"
        uint32_t Bob::get_bob(Anna, std::unique_ptr<Anna>) const {
            return a;
        }
        Anna give_anna() {
            Anna a;
            a.a = 10;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        #include <memory>
        struct Anna {
            uint32_t a;
            std::string b;
        };
        Anna give_anna();
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob(Anna a, std::unique_ptr<Anna>) const;
        };
    "};
    let rs = quote! {
        let a = ffi::give_anna();
        let a2 = ffi::give_anna();
        let b = ffi::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(a, a2), 12);
    };
    run_test(
        cxx,
        hdr,
        rs,
        &["take_bob", "Anna", "give_anna", "get_bob"],
        &["Bob"],
    );
}

#[test]
fn test_method_pass_nonpod_by_reference() {
    let cxx = indoc! {"
        uint32_t Bob::get_bob(const Anna&) const {
            return a;
        }
        Anna give_anna() {
            Anna a;
            a.a = 10;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Anna {
            uint32_t a;
            std::string b;
        };
        Anna give_anna();
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob(const Anna& a) const;
        };
    "};
    let rs = quote! {
        let a = ffi::give_anna();
        let b = ffi::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(a.as_ref().unwrap()), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob", "Anna", "give_anna"], &["Bob"]);
}

#[test]
fn test_method_pass_nonpod_by_mut_reference() {
    let cxx = indoc! {"
        uint32_t Bob::get_bob(Anna&) const {
            return a;
        }
        Anna give_anna() {
            Anna a;
            a.a = 10;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Anna {
            uint32_t a;
            std::string b;
        };
        Anna give_anna();
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob(Anna& a) const;
        };
    "};
    let rs = quote! {
        let mut a = ffi::give_anna();
        let b = ffi::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(a.as_mut().unwrap()), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob", "Anna", "give_anna"], &["Bob"]);
}

#[test]
fn test_method_pass_nonpod_by_up() {
    let cxx = indoc! {"
        uint32_t Bob::get_bob(std::unique_ptr<Anna>) const {
            return a;
        }
        Anna give_anna() {
            Anna a;
            a.a = 10;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        #include <string>
        struct Anna {
            uint32_t a;
            std::string b;
        };
        Anna give_anna();
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob(std::unique_ptr<Anna> z) const;
        };
    "};
    let rs = quote! {
        let a = ffi::give_anna();
        let b = ffi::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob", "give_anna"], &["Bob"]);
}

#[test]
fn test_method_return_nonpod_by_value() {
    let cxx = indoc! {"
        Anna Bob::get_anna() const {
            Anna a;
            a.a = 12;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Anna {
            uint32_t a;
            std::string b;
        };
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            Anna get_anna() const;
        };
    "};
    let rs = quote! {
        let b = ffi::Bob { a: 12, b: 13 };
        let a = b.get_anna();
        assert!(!a.is_null());
    };
    run_test(cxx, hdr, rs, &["take_bob", "Anna", "get_anna"], &["Bob"]);
}

#[test]
fn test_pass_string_by_value() {
    let cxx = indoc! {"
        uint32_t measure_string(std::string z) {
            return z.length();
        }
        std::unique_ptr<std::string> get_msg() {
            return std::make_unique<std::string>(\"hello\");
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        #include <memory>
        uint32_t measure_string(std::string a);
        std::unique_ptr<std::string> get_msg();
    "};
    let rs = quote! {
        let a = ffi::get_msg();
        let c = ffi::measure_string(a);
        assert_eq!(c, 5);
    };
    run_test(cxx, hdr, rs, &["measure_string", "get_msg"], &[]);
}

#[test]
fn test_return_string_by_value() {
    let cxx = indoc! {"
        std::string get_msg() {
            return \"hello\";
        }
    "};
    let hdr = indoc! {"
        #include <string>
        std::string get_msg();
    "};
    let rs = quote! {
        let a = ffi::get_msg();
        assert!(a.as_ref().unwrap() == "hello");
    };
    run_test(cxx, hdr, rs, &["get_msg"], &[]);
}

#[test]
fn test_method_pass_string_by_value() {
    let cxx = indoc! {"
        uint32_t Bob::measure_string(std::string z) const {
            return z.length();
        }
        std::unique_ptr<std::string> get_msg() {
            return std::make_unique<std::string>(\"hello\");
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        #include <memory>
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t measure_string(std::string a) const;
        };
        std::unique_ptr<std::string> get_msg();
    "};
    let rs = quote! {
        let a = ffi::get_msg();
        let b = ffi::Bob { a: 12, b: 13 };
        let c = b.measure_string(a);
        assert_eq!(c, 5);
    };
    run_test(
        cxx,
        hdr,
        rs,
        &["measure_string", "Bob", "get_msg"],
        &["Bob"],
    );
}

#[test]
fn test_method_return_string_by_value() {
    let cxx = indoc! {"
        std::string Bob::get_msg() const {
            return \"hello\";
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            std::string get_msg() const;
        };
    "};
    let rs = quote! {
        let b = ffi::Bob { a: 12, b: 13 };
        let a = b.get_msg();
        assert!(a.as_ref().unwrap() == "hello");
    };
    run_test(cxx, hdr, rs, &["take_bob", "get_msg"], &["Bob"]);
}

#[test]
fn test_pass_rust_string_by_ref() {
    let cxx = indoc! {"
        uint32_t measure_string(const rust::String& z) {
            return std::string(z).length();
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <cxx.h>
        uint32_t measure_string(const rust::String& z);
    "};
    let rs = quote! {
        let c = ffi::measure_string(&"hello".to_string());
        assert_eq!(c, 5);
    };
    run_test(cxx, hdr, rs, &["measure_string"], &[]);
}

#[test]
fn test_pass_rust_string_by_value() {
    let cxx = indoc! {"
        uint32_t measure_string(rust::String z) {
            return std::string(z).length();
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <cxx.h>
        uint32_t measure_string(rust::String z);
    "};
    let rs = quote! {
        let c = ffi::measure_string("hello".into());
        assert_eq!(c, 5);
    };
    run_test(cxx, hdr, rs, &["measure_string"], &[]);
}

#[test]
fn test_pass_rust_str() {
    // passing by value is the only legal option
    let cxx = indoc! {"
        uint32_t measure_string(rust::Str z) {
            return std::string(z).length();
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <cxx.h>
        uint32_t measure_string(rust::Str z);
    "};
    let rs = quote! {
        let c = ffi::measure_string("hello");
        assert_eq!(c, 5);
    };
    run_test(cxx, hdr, rs, &["measure_string"], &[]);
}

#[test]
fn test_multiple_classes_with_methods() {
    let hdr = indoc! {"
        #include <cstdint>

        struct TrivialStruct {
            uint32_t val = 0;
        
            uint32_t get() const;
            uint32_t inc();
        };
        TrivialStruct make_trivial_struct();
        
        class TrivialClass {
          public:
            uint32_t get() const;
            uint32_t inc();
        
          private:
            uint32_t val_ = 1;
        };
        TrivialClass make_trivial_class();

        struct OpaqueStruct {
            // ~OpaqueStruct();
            uint32_t val = 2;
        
            uint32_t get() const;
            uint32_t inc();
        };
        OpaqueStruct make_opaque_struct();

        class OpaqueClass {
          public:
            // ~OpaqueClass();
            uint32_t get() const;
            uint32_t inc();
        
          private:
            uint32_t val_ = 3;
        };
        OpaqueClass make_opaque_class();
    "};
    let cxx = indoc! {"
        TrivialStruct make_trivial_struct() { return {}; }
        TrivialClass make_trivial_class() { return {}; }
        OpaqueStruct make_opaque_struct() { return {}; }
        OpaqueClass make_opaque_class() { return {}; }

        uint32_t TrivialStruct::get() const { return val;}
        uint32_t TrivialClass::get() const { return val_; }
        uint32_t OpaqueStruct::get() const { return val;}
        uint32_t OpaqueClass::get() const { return val_; }

        uint32_t TrivialStruct::inc() { return ++val; }
        uint32_t TrivialClass::inc() { return ++val_; }
        uint32_t OpaqueStruct::inc() { return ++val; }
        uint32_t OpaqueClass::inc() { return ++val_; }
    "};
    let rs = quote! {
        use ffi::*;

        let mut ts = Box::pin(make_trivial_struct());
        assert_eq!(ts.get(), 0);
        assert_eq!(ts.as_mut().inc(), 1);
        assert_eq!(ts.as_mut().inc(), 2);

        let mut tc = Box::pin(make_trivial_class());
        assert_eq!(tc.get(), 1);
        assert_eq!(tc.as_mut().inc(), 2);
        assert_eq!(tc.as_mut().inc(), 3);

        let mut os= make_opaque_struct();
        assert_eq!(os.get(), 2);
        assert_eq!(os.pin_mut().inc(), 3);
        assert_eq!(os.pin_mut().inc(), 4);

        let mut oc = make_opaque_class();
        assert_eq!(oc.get(), 3);
        assert_eq!(oc.pin_mut().inc(), 4);
        assert_eq!(oc.pin_mut().inc(), 5);
    };
    run_test_ex(
        cxx,
        hdr,
        rs,
        &[
            "make_trivial_struct",
            "make_trivial_class",
            "make_opaque_struct",
            "make_opaque_class",
            "OpaqueStruct",
            "OpaqueClass",
        ],
        &["TrivialStruct", "TrivialClass"],
        None,
        &[],
    );
}

#[test]
fn test_ns_return_struct() {
    let cxx = indoc! {"
        A::B::Bob give_bob() {
            A::B::Bob a;
            a.a = 3;
            a.b = 4;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        namespace A {
            namespace B {
                struct Bob {
                    uint32_t a;
                    uint32_t b;
                };
            }
        }
        A::B::Bob give_bob();
    "};
    let rs = quote! {
        assert_eq!(ffi::give_bob().b, 4);
    };
    run_test(cxx, hdr, rs, &["give_bob"], &["A::B::Bob"]);
}

#[test]
fn test_ns_take_struct() {
    let cxx = indoc! {"
    uint32_t take_bob(A::B::Bob a) {
        return a.a;
    }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        namespace A {
            namespace B {
                struct Bob {
                    uint32_t a;
                    uint32_t b;
                };
            }
        }
        uint32_t take_bob(A::B::Bob a);
    "};
    let rs = quote! {
        let a = ffi::A::B::Bob { a: 12, b: 13 };
        assert_eq!(ffi::take_bob(a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob"], &["A::B::Bob"]);
}

#[test]
fn test_ns_func() {
    let cxx = indoc! {"
        using namespace C;
        A::B::Bob C::give_bob() {
            A::B::Bob a;
            a.a = 3;
            a.b = 4;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        namespace A {
            namespace B {
                struct Bob {
                    uint32_t a;
                    uint32_t b;
                };
            }
        }
        namespace C {
            ::A::B::Bob give_bob();
        }
    "};
    let rs = quote! {
        assert_eq!(ffi::C::give_bob().b, 4);
    };
    run_test(cxx, hdr, rs, &["C::give_bob"], &["A::B::Bob"]);
}

#[test]
fn test_overload_constructors() {
    let cxx = indoc! {"
        Bob::Bob() {}
        Bob::Bob(uint32_t _a) :a(_a) {}
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            Bob();
            Bob(uint32_t a);
            uint32_t a;
            uint32_t b;
        };
    "};
    let rs = quote! {
        ffi::Bob::make_unique();
        ffi::Bob::make_unique1(32);
    };
    run_test(cxx, hdr, rs, &["Bob"], &[]);
}

#[test]
fn test_overload_functions() {
    let cxx = indoc! {"
        void daft(uint32_t a) {}
        void daft(uint8_t a) {}
        void daft(std::string a) {}
        void daft(Fred a) {}
        void daft(Norma a) {}
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Fred {
            uint32_t a;
        };
        struct Norma {
            Norma() {}
            uint32_t a;
        };
        void daft(uint32_t);
        void daft(uint8_t);
        void daft(std::string);
        void daft(Fred);
        void daft(Norma);
    "};
    let rs = quote! {
        use ffi::ToCppString;
        ffi::daft(32);
        ffi::daft1(8);
        ffi::daft2("hello".to_cpp());
        let b = ffi::Fred { a: 3 };
        ffi::daft3(b);
        let c = ffi::Norma::make_unique();
        ffi::daft4(c);
    };
    run_test(
        cxx,
        hdr,
        rs,
        &["Norma", "daft", "daft1", "daft2", "daft3", "daft4"],
        &["Fred"],
    );
}

#[test]
#[ignore] // At present, bindgen generates two separate 'daft1'
          // functions here, and there's not much we can do about that.
fn test_overload_numeric_functions() {
    // Because bindgen deals with conflicting overloaded functions by
    // appending a numeric suffix, let's see if we can cope.
    let cxx = indoc! {"
        void daft1(uint32_t) {}
        void daft2(uint8_t) {}
        void daft(std::string) {}
        void daft(Fred) {}
        void daft(Norma) {}
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Fred {
            uint32_t a;
        };
        struct Norma {
            uint32_t a;
        };
        void daft1(uint32_t a);
        void daft2(uint8_t a);
        void daft(std::string a);
        void daft(Fred a);
        void daft(Norma a);
    "};
    let rs = quote! {
        use ffi::ToCppString;
        ffi::daft(32);
        ffi::daft1(8);
        ffi::daft2("hello".to_cpp());
        let b = ffi::Fred { a: 3 };
        ffi::daft3(b);
        let c = ffi::Norma::make_unique();
        ffi::daft4(c);
    };
    run_test(
        cxx,
        hdr,
        rs,
        &["Norma", "daft", "daft1", "daft2", "daft3", "daft4"],
        &["Fred"],
    );
}

#[test]
fn test_overload_methods() {
    let cxx = indoc! {"
        void Bob::daft(uint32_t) const {}
        void Bob::daft(uint8_t) const {}
        void Bob::daft(std::string) const {}
        void Bob::daft(Fred) const {}
        void Bob::daft(Norma) const {}
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Fred {
            uint32_t a;
        };
        struct Norma {
            Norma() {}
            uint32_t a;
        };
        struct Bob {
            uint32_t a;
            void daft(uint32_t) const;
            void daft(uint8_t) const;
            void daft(std::string) const;
            void daft(Fred) const;
            void daft(Norma) const;
        };
    "};
    let rs = quote! {
        use ffi::ToCppString;
        let a = ffi::Bob { a: 12 };
        a.daft(32);
        a.daft1(8);
        a.daft2("hello".to_cpp());
        let b = ffi::Fred { a: 3 };
        a.daft3(b);
        let c = ffi::Norma::make_unique();
        a.daft4(c);
    };
    run_test(cxx, hdr, rs, &["Norma"], &["Fred", "Bob"]);
}

#[test]
fn test_ns_constructor() {
    let cxx = indoc! {"
        A::Bob::Bob() {}
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                Bob();
                uint32_t a;
                uint32_t b;
            };
        }
    "};
    let rs = quote! {
        ffi::A::Bob::make_unique();
    };
    run_test(cxx, hdr, rs, &["A::Bob"], &[]);
}

#[test]
fn test_ns_up_direct() {
    let cxx = indoc! {"
        std::unique_ptr<A::Bob> A::get_bob() {
            A::Bob b;
            b.a = 2;
            b.b = 3;
            return std::make_unique<A::Bob>(b);
        }
        uint32_t give_bob(A::Bob bob) {
            return bob.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                uint32_t a;
                uint32_t b;
            };
            std::unique_ptr<Bob> get_bob();
        }
        uint32_t give_bob(A::Bob bob);
    "};
    let rs = quote! {
        assert_eq!(ffi::give_bob(ffi::A::get_bob()), 2);
    };
    run_test(cxx, hdr, rs, &["give_bob", "A::get_bob"], &[]);
}

#[test]
fn test_ns_up_wrappers() {
    let cxx = indoc! {"
        A::Bob get_bob() {
            A::Bob b;
            b.a = 2;
            b.b = 3;
            return b;
        }
        uint32_t give_bob(A::Bob bob) {
            return bob.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        namespace A {
            struct Bob {
                uint32_t a;
                uint32_t b;
            };
        }
        A::Bob get_bob();
        uint32_t give_bob(A::Bob bob);
    "};
    let rs = quote! {
        assert_eq!(ffi::give_bob(ffi::get_bob()), 2);
    };
    run_test(cxx, hdr, rs, &["give_bob", "get_bob"], &[]);
}

#[test]
fn test_ns_up_wrappers_in_up() {
    let cxx = indoc! {"
        A::Bob A::get_bob() {
            A::Bob b;
            b.a = 2;
            b.b = 3;
            return b;
        }
        uint32_t give_bob(A::Bob bob) {
            return bob.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        namespace A {
            struct Bob {
                uint32_t a;
                uint32_t b;
            };
            Bob get_bob();
        }
        uint32_t give_bob(A::Bob bob);
    "};
    let rs = quote! {
        assert_eq!(ffi::give_bob(ffi::A::get_bob()), 2);
    };
    run_test(cxx, hdr, rs, &["give_bob", "A::get_bob"], &[]);
}

#[test]
fn test_return_reference() {
    let cxx = indoc! {"
        const Bob& give_bob(const Bob& input_bob) {
            return input_bob;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        const Bob& give_bob(const Bob& input_bob);
    "};
    let rs = quote! {
        let b = ffi::Bob { a: 3, b: 4 };
        assert_eq!(ffi::give_bob(&b).b, 4);
    };
    run_test(cxx, hdr, rs, &["give_bob"], &["Bob"]);
}

#[test]
fn test_destructor() {
    let hdr = indoc! {"
        struct WithDtor {
            ~WithDtor();
        };
        WithDtor make_with_dtor();
    "};
    let cxx = indoc! {"
        WithDtor::~WithDtor() {}
        WithDtor make_with_dtor() {
            return {};
        }
    "};
    let rs = quote! {
        use ffi::*;
        let with_dtor: cxx::UniquePtr<WithDtor> = make_with_dtor();
        drop(with_dtor);
    };
    run_test(cxx, hdr, rs, &["WithDtor", "make_with_dtor"], &[]);
}

#[test]
fn test_static_func() {
    let hdr = indoc! {"
        #include <cstdint>
        struct WithStaticMethod {
            static uint32_t call();
        };
    "};
    let cxx = indoc! {"
        uint32_t WithStaticMethod::call() {
            return 42;
        }
    "};
    let rs = quote! {
        assert_eq!(ffi::WithStaticMethod::call(), 42);
    };
    run_test(cxx, hdr, rs, &["WithStaticMethod"], &[]);
}

#[test]
fn test_static_func_wrapper() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct A {
            std::string a;
            static A CreateA(std::string a, std::string b) {
                A c;
                c.a = a;
                return c;
            }
        };
    "};
    let rs = quote! {
        use ffi::ToCppString;
        ffi::A::CreateA("a".to_cpp(), "b".to_cpp());
    };
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_give_pod_typedef_by_value() {
    let cxx = indoc! {"
        Horace give_bob() {
            Horace a;
            a.a = 3;
            a.b = 4;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        using Horace = Bob;
        Horace give_bob();
    "};
    let rs = quote! {
        assert_eq!(ffi::give_bob().b, 4);
    };
    run_test(cxx, hdr, rs, &["give_bob"], &["Bob"]);
}

#[ignore] // because we need to put some aliases in the output ffi mod.
#[test]
fn test_use_pod_typedef() {
    let cxx = indoc! {"
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        using Horace = Bob;
    "};
    let rs = quote! {
        let h = Horace { a: 3, b: 4 };
        assert_eq!(h.b, 4);
    };
    run_test(cxx, hdr, rs, &[], &["Bob"]);
}

#[ignore] // we don't yet allow typedefs to be listed in allow_pod
#[test]
fn test_use_pod_typedef_with_allowpod() {
    let cxx = indoc! {"
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        using Horace = Bob;
    "};
    let rs = quote! {
        let h = Horace { a: 3, b: 4 };
        assert_eq!(h.b, 4);
    };
    run_test(cxx, hdr, rs, &[], &["Horace"]);
}

#[test]
fn test_give_nonpod_typedef_by_value() {
    let cxx = indoc! {"
        Horace give_bob() {
            Horace a;
            a.a = 3;
            a.b = 4;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        using Horace = Bob;
        Horace give_bob();
        inline uint32_t take_horace(const Horace& horace) { return horace.b; }
    "};
    let rs = quote! {
        assert_eq!(ffi::take_horace(ffi::give_bob().as_ref().unwrap()), 4);
    };
    run_test(cxx, hdr, rs, &["give_bob", "take_horace"], &[]);
}

#[test]
#[ignore] // bindgen creates functions called create and create1,
          // then impl blocks for Bob and Fred which call them from methods each
          // called simply 'create'. Because of this mismatch we have no way to know
          // that 'create1' is a static function, and this fails. We _could_ parse
          // the contents of the impl block methods in order to find out what is
          // actually being called, but currently bindgen in fact gets this wrong,
          // and calls 'create' from both of them. The first step to getting this
          // working would therefore be to make a minimal test case and file it
          // against bindgen. TODO. Then after that was fixed we could look at
          // whether it's worth fixing this by parsing the contents of the impl'ed
          // methods.
fn test_conflicting_static_functions() {
    let cxx = indoc! {"
        Bob Bob::create() { Bob a; return a; }
        Fred Fred::create() { Fred b; return b; }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            static Bob create();
        };
        struct Fred {
            uint32_t b;
            static Fred create();
        };
    "};
    let rs = quote! {
        ffi::Bob::create();
        ffi::Fred::create();
    };
    run_test(cxx, hdr, rs, &[], &["Bob", "Fred"]);
}

#[test]
fn test_conflicting_ns_up_functions() {
    let cxx = indoc! {"
        uint32_t A::create(C) { return 3; }
        uint32_t B::create(C) { return 4; }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct C {
            C() {}
            uint32_t a;
        };
        namespace A {
            uint32_t create(C c);
        };
        namespace B {
            uint32_t create(C c);
        };
    "};
    let rs = quote! {
        let c = ffi::C::make_unique();
        let c2 = ffi::C::make_unique();
        assert_eq!(ffi::A::create(c), 3);
        assert_eq!(ffi::B::create(c2), 4);
    };
    run_test(cxx, hdr, rs, &["A::create", "B::create", "C"], &[]);
}

#[test]
fn test_conflicting_methods() {
    let cxx = indoc! {"
        uint32_t Bob::get() const { return a; }
        uint32_t Fred::get() const { return b; }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t get() const;
        };
        struct Fred {
            uint32_t b;
            uint32_t get() const;
        };
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 10 };
        let b = ffi::Fred { b: 20 };
        assert_eq!(a.get(), 10);
        assert_eq!(b.get(), 20);
    };
    run_test(cxx, hdr, rs, &[], &["Bob", "Fred"]);
}

#[test]
// There's a bindgen bug here. bindgen generates
// functions called 'get' and 'get1' but then generates impl
// blocks which call 'get' and 'get'. By luck, we currently
// should not be broken by this, but at some point we should take
// the time to create a minimal bindgen test case and submit it
// as a bindgen bug.
fn test_conflicting_up_wrapper_methods_not_in_ns() {
    // Ensures the two names 'get' do not conflict in the flat
    // cxx::bridge mod namespace.
    let cxx = indoc! {"
        Bob::Bob() : a(\"hello\") {}
        Fred::Fred() : b(\"goodbye\") {}
        std::string Bob::get() const { return a; }
        std::string Fred::get() const { return b; }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Bob {
            Bob();
            std::string a;
            std::string get() const;
        };
        struct Fred {
            Fred();
            std::string b;
            std::string get() const;
        };
    "};
    let rs = quote! {
        let a = ffi::Bob::make_unique();
        let b = ffi::Fred::make_unique();
        assert_eq!(a.get().as_ref().unwrap().to_str().unwrap(), "hello");
        assert_eq!(b.get().as_ref().unwrap().to_str().unwrap(), "goodbye");
    };
    run_test(cxx, hdr, rs, &["Bob", "Fred"], &[]);
}

#[test]
fn test_conflicting_methods_in_ns() {
    let cxx = indoc! {"
        uint32_t A::Bob::get() const { return a; }
        uint32_t B::Fred::get() const { return b; }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        namespace A {
            struct Bob {
                uint32_t a;
                uint32_t get() const;
            };
        }
        namespace B {
            struct Fred {
                uint32_t b;
                uint32_t get() const;
            };
        }
    "};
    let rs = quote! {
        let a = ffi::A::Bob { a: 10 };
        let b = ffi::B::Fred { b: 20 };
        assert_eq!(a.get(), 10);
        assert_eq!(b.get(), 20);
    };
    run_test(cxx, hdr, rs, &[], &["A::Bob", "B::Fred"]);
}

#[test]
fn test_conflicting_up_wrapper_methods_in_ns() {
    let cxx = indoc! {"
        A::Bob::Bob() : a(\"hello\") {}
        B::Fred::Fred() : b(\"goodbye\") {}
        std::string A::Bob::get() const { return a; }
        std::string B::Fred::get() const { return b; }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        namespace A {
            struct Bob {
                Bob();
                std::string a;
                std::string get() const;
            };
        }
        namespace B {
            struct Fred {
                Fred();
                std::string b;
                std::string get() const;
            };
        }
    "};
    let rs = quote! {
        let a = ffi::A::Bob::make_unique();
        let b = ffi::B::Fred::make_unique();
        assert_eq!(a.get().as_ref().unwrap().to_str().unwrap(), "hello");
        assert_eq!(b.get().as_ref().unwrap().to_str().unwrap(), "goodbye");
    };
    run_test(cxx, hdr, rs, &["A::Bob", "B::Fred"], &[]);
}

#[test]
fn test_ns_struct_pod_request() {
    let hdr = indoc! {"
        #include <cstdint>
        namespace A {
            struct Bob {
                uint32_t a;
            };
        }
    "};
    let rs = quote! {
        ffi::A::Bob { a: 12 };
    };
    run_test("", hdr, rs, &[], &["A::Bob"]);
}

#[test]
fn test_conflicting_ns_funcs() {
    let cxx = indoc! {"
        uint32_t A::get() { return 10; }
        uint32_t B::get() { return 20; }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        namespace A {
            uint32_t get();
        }
        namespace B {
            uint32_t get();
        }
    "};
    let rs = quote! {
        assert_eq!(ffi::A::get(), 10);
        assert_eq!(ffi::B::get(), 20);
    };
    run_test(cxx, hdr, rs, &["A::get", "B::get"], &[]);
}

#[ignore]
// because currently we feed a flat namespace to cxx
// This would be relatively easy to enable now that we have the facility
// to add aliases to the 'use' statements we generate, plus
// bridge_name_tracker to pick a unique name. TODO.
#[test]
fn test_conflicting_ns_structs() {
    let hdr = indoc! {"
        #include <cstdint>
        namespace A {
            struct Bob {
                uint32_t a;
            };
        }
        namespace B {
            struct Bob {
                uint32_t a;
            };
        }
    "};
    let rs = quote! {
        ffi::A::Bob { a: 12 };
        ffi::B::Bob { b: 12 };
    };
    run_test("", hdr, rs, &[], &["A::Bob", "B::Bob"]);
}

#[test]
fn test_make_string() {
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
        };
    "};
    let rs = quote! {
        use ffi::ToCppString;
        let a = "hello".to_cpp();
        assert_eq!(a.to_str().unwrap(), "hello");
    };
    run_test("", hdr, rs, &["Bob"], &[]);
}

#[test]
fn test_string_constant() {
    let hdr = indoc! {"
        #include <cstdint>
        const char* STRING = \"Foo\";
    "};
    let rs = quote! {
        let a = std::str::from_utf8(ffi::STRING).unwrap().trim_end_matches(char::from(0));
        assert_eq!(a, "Foo");
    };
    run_test("", hdr, rs, &["STRING"], &[]);
}

#[test]
fn test_pod_constant_harmless_inside_type() {
    // Check that the presence of this constant doesn't break anything.
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
        };
        struct Anna {
            uint32_t a;
            const Bob BOB = Bob { 10 };
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &[], &["Anna"]);
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/93
fn test_pod_constant() {
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
        };
        const Bob BOB = Bob { 10 };
    "};
    let rs = quote! {
        let a = &ffi::BOB;
        assert_eq!(a.a, 10);
    };
    run_test("", hdr, rs, &["BOB"], &["Bob"]);
}

#[test]
fn test_pod_static_harmless_inside_type() {
    // Check that the presence of this constant doesn't break anything.
    // Remove this test when the following one is enabled.
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
        };
        struct Anna {
            uint32_t a;
            static Bob BOB;
        };
        Bob Anna::BOB = Bob { 10 };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &[], &["Anna"]);
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/93
fn test_pod_static() {
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
        };
        static Bob BOB = Bob { 10 };
    "};
    let rs = quote! {
        let a = &ffi::BOB;
        assert_eq!(a.a, 10);
    };
    run_test("", hdr, rs, &["BOB"], &["Bob"]);
}

#[test]
#[ignore] // this probably requires code generation on the C++
          // side. It's not at all clear how best to handle this.
fn test_non_pod_constant() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Bob {
            std::string a;
            std::string get() { return a };
        };
        const Bob BOB = Bob { \"hello\" };
    "};
    let rs = quote! {
        let a = ffi::BOB;
        // following line assumes that 'a' is a &Bob
        // but who knows how we'll really do this.
        assert_eq!(a.get().as_ref().unwrap().to_str().unwrap(), "hello");
    };
    run_test("", hdr, rs, &["BOB"], &[]);
}

#[test]
fn test_templated_typedef() {
    let hdr = indoc! {"
        #include <string>
        #include <cstdint>

        template <typename STRING_TYPE> class BasicStringPiece {
        public:
            const STRING_TYPE* ptr_;
            size_t length_;
        };
        typedef BasicStringPiece<uint8_t> StringPiece;

        struct Origin {
            Origin() {}
            StringPiece host;
        };
    "};
    let rs = quote! {
        ffi::Origin::make_unique();
    };
    run_test("", hdr, rs, &["Origin"], &[]);
}

#[test]
fn test_struct_templated_typedef() {
    let hdr = indoc! {"
        #include <string>
        #include <cstdint>

        struct Concrete {
            uint8_t a;
        };
        template <typename STRING_TYPE> class BasicStringPiece {
        public:
            const STRING_TYPE* ptr_;
            size_t length_;
        };
        typedef BasicStringPiece<Concrete> StringPiece;

        struct Origin {
            Origin() {}
            StringPiece host;
        };
    "};
    let rs = quote! {
        ffi::Origin::make_unique();
    };
    run_test("", hdr, rs, &["Origin"], &[]);
}

#[test]
fn test_enum_typedef() {
    let hdr = indoc! {"
        enum ConstraintSolverParameters_TrailCompression : int {
            ConstraintSolverParameters_TrailCompression_NO_COMPRESSION = 0,
            ConstraintSolverParameters_TrailCompression_COMPRESS_WITH_ZLIB = 1,
            ConstraintSolverParameters_TrailCompression_ConstraintSolverParameters_TrailCompression_INT_MIN_SENTINEL_DO_NOT_USE_ = -2147483648,
            ConstraintSolverParameters_TrailCompression_ConstraintSolverParameters_TrailCompression_INT_MAX_SENTINEL_DO_NOT_USE_ = 2147483647
        };
        typedef ConstraintSolverParameters_TrailCompression TrailCompression;
    "};
    let rs = quote! {
        let _ = ffi::TrailCompression::ConstraintSolverParameters_TrailCompression_NO_COMPRESSION;
    };
    run_test("", hdr, rs, &["TrailCompression"], &[]);
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/264
fn test_conflicting_usings() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <cstddef>
        typedef size_t diff;
        struct A {
            using diff = diff;
            diff a;
        };
        struct B {
            using diff = diff;
            diff a;
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &[], &["A", "B"]);
}

#[test]
fn test_conflicting_usings_with_self_declaration1() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <cstddef>
        struct common_params {
            using difference_type = ptrdiff_t;
        };
        template <typename Params>
        class btree_node {
            public:
            using difference_type = typename Params::difference_type;
            Params params;
        };
        template <typename Tree>
        class btree_container {
            public:
            using difference_type = typename Tree::difference_type;
            void clear() {}
            Tree b;
            uint32_t a;
        };
        typedef btree_container<btree_node<common_params>> my_tree;
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["my_tree"], &[]);
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/106
fn test_string_templated_typedef() {
    let hdr = indoc! {"
        #include <string>
        #include <cstdint>

        template <typename STRING_TYPE> class BasicStringPiece {
        public:
            const STRING_TYPE* ptr_;
            size_t length_;
        };
        typedef BasicStringPiece<std::string> StringPiece;

        struct Origin {
            Origin() {}
            StringPiece host;
        };
    "};
    let rs = quote! {
        ffi::Origin::make_unique();
    };
    run_test("", hdr, rs, &["Origin"], &[]);
}

#[ignore] // https://github.com/rust-lang/rust-bindgen/issues/1924
#[test]
fn test_associated_type_templated_typedef() {
    let hdr = indoc! {"
        #include <string>
        #include <cstdint>

        template <typename STRING_TYPE> class BasicStringPiece {
        public:
            typedef size_t size_type;
            typedef typename STRING_TYPE::value_type value_type;
            const value_type* ptr_;
            size_type length_;
        };

        typedef BasicStringPiece<std::string> StringPiece;

        struct Origin {
            // void SetHost(StringPiece host);
            StringPiece host;
        };
    "};
    let rs = quote! {
        ffi::Origin::make_unique();
    };
    run_test("", hdr, rs, &["Origin"], &[]);
}

#[test]
fn test_foreign_ns_func_arg_pod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                uint32_t a;
            };
        }
        namespace B {
            inline uint32_t daft(A::Bob a) { return a.a; }
        }
    "};
    let rs = quote! {
        let a = ffi::A::Bob { a: 12 };
        assert_eq!(ffi::B::daft(a), 12);
    };
    run_test("", hdr, rs, &["B::daft"], &["A::Bob"]);
}

#[test]
fn test_foreign_ns_func_arg_nonpod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                uint32_t a;
                Bob(uint32_t _a) :a(_a) {}
            };
        }
        namespace B {
            inline uint32_t daft(A::Bob a) { return a.a; }
        }
    "};
    let rs = quote! {
        let a = ffi::A::Bob::make_unique(12);
        assert_eq!(ffi::B::daft(a), 12);
    };
    run_test("", hdr, rs, &["B::daft", "A::Bob"], &[]);
}

#[test]
fn test_foreign_ns_meth_arg_pod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                uint32_t a;
            };
        }
        namespace B {
            struct C {
                uint32_t a;
                uint32_t daft(A::Bob a) const { return a.a; }
            };
        }
    "};
    let rs = quote! {
        let a = ffi::A::Bob { a: 12 };
        let b = ffi::B::C { a: 12 };
        assert_eq!(b.daft(a), 12);
    };
    run_test("", hdr, rs, &[], &["A::Bob", "B::C"]);
}

#[test]
fn test_foreign_ns_meth_arg_nonpod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                uint32_t a;
                Bob(uint32_t _a) :a(_a) {}
            };
        }
        namespace B {
            struct C {
                uint32_t a;
                uint32_t daft(A::Bob a) const { return a.a; }
            };
        }
    "};
    let rs = quote! {
        let a = ffi::A::Bob::make_unique(12);
        let b = ffi::B::C { a: 12 };
        assert_eq!(b.daft(a), 12);
    };
    run_test("", hdr, rs, &["A::Bob"], &["B::C"]);
}

#[test]
fn test_foreign_ns_cons_arg_pod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                uint32_t a;
            };
        }
        namespace B {
            struct C {
                uint32_t a;
                C(const A::Bob& input) : a(input.a) {}
            };
        }
    "};
    let rs = quote! {
        let a = ffi::A::Bob { a: 12 };
        let b = ffi::B::C::make_unique(&a);
        assert_eq!(b.as_ref().unwrap().a, 12);
    };
    run_test("", hdr, rs, &[], &["B::C", "A::Bob"]);
}

#[test]
fn test_foreign_ns_cons_arg_nonpod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                Bob(uint32_t _a) :a(_a) {}
                uint32_t a;
            };
        }
        namespace B {
            struct C {
                uint32_t a;
                C(const A::Bob& input) : a(input.a) {}
            };
        }
    "};
    let rs = quote! {
        let a = ffi::A::Bob::make_unique(12);
        let b = ffi::B::C::make_unique(&a);
        assert_eq!(b.as_ref().unwrap().a, 12);
    };
    run_test("", hdr, rs, &["A::Bob"], &["B::C"]);
}

#[test]
fn test_foreign_ns_func_ret_pod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                uint32_t a;
            };
        }
        namespace B {
            inline A::Bob daft() { A::Bob bob; bob.a = 12; return bob; }
        }
    "};
    let rs = quote! {
        assert_eq!(ffi::B::daft().a, 12);
    };
    run_test("", hdr, rs, &["B::daft"], &["A::Bob"]);
}

#[test]
fn test_foreign_ns_func_ret_nonpod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                uint32_t a;
            };
        }
        namespace B {
            inline A::Bob daft() { A::Bob bob; bob.a = 12; return bob; }
        }
    "};
    let rs = quote! {
        ffi::B::daft().as_ref().unwrap();
    };
    run_test("", hdr, rs, &["B::daft", "A::Bob"], &[]);
}

#[test]
fn test_foreign_ns_meth_ret_pod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                uint32_t a;
            };
        }
        namespace B {
            struct C {
                uint32_t a;
                A::Bob daft() const { A::Bob bob; bob.a = 12; return bob; }
            };
        }
    "};
    let rs = quote! {
        let b = ffi::B::C { a: 12 };
        assert_eq!(b.daft().a, 12);
    };
    run_test("", hdr, rs, &[], &["A::Bob", "B::C"]);
}

#[test]
fn test_foreign_ns_meth_ret_nonpod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                uint32_t a;
            };
        }
        namespace B {
            struct C {
                uint32_t a;
                A::Bob daft() const { A::Bob bob; bob.a = 12; return bob; }
            };
        }
    "};
    let rs = quote! {
        let b = ffi::B::C { a: 14 };
        b.daft().as_ref().unwrap();
    };
    run_test("", hdr, rs, &["A::Bob"], &["B::C"]);
}

#[test]
fn test_root_ns_func_arg_pod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        namespace B {
            inline uint32_t daft(Bob a) { return a.a; }
        }
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 12 };
        assert_eq!(ffi::B::daft(a), 12);
    };
    run_test("", hdr, rs, &["B::daft"], &["Bob"]);
}

#[test]
fn test_root_ns_func_arg_nonpod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
            Bob(uint32_t _a) :a(_a) {}
        };
        namespace B {
            inline uint32_t daft(Bob a) { return a.a; }
        }
    "};
    let rs = quote! {
        let a = ffi::Bob::make_unique(12);
        assert_eq!(ffi::B::daft(a), 12);
    };
    run_test("", hdr, rs, &["B::daft", "Bob"], &[]);
}

#[test]
fn test_root_ns_meth_arg_pod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        namespace B {
            struct C {
                uint32_t a;
                uint32_t daft(Bob a) const { return a.a; }
            };
        }
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 12 };
        let b = ffi::B::C { a: 12 };
        assert_eq!(b.daft(a), 12);
    };
    run_test("", hdr, rs, &[], &["Bob", "B::C"]);
}

#[test]
fn test_root_ns_meth_arg_nonpod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
            Bob(uint32_t _a) :a(_a) {}
        };
        namespace B {
            struct C {
                uint32_t a;
                uint32_t daft(Bob a) const { return a.a; }
            };
        }
    "};
    let rs = quote! {
        let a = ffi::Bob::make_unique(12);
        let b = ffi::B::C { a: 12 };
        assert_eq!(b.daft(a), 12);
    };
    run_test("", hdr, rs, &["Bob"], &["B::C"]);
}

#[test]
fn test_root_ns_cons_arg_pod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        namespace B {
            struct C {
                uint32_t a;
                C(const Bob& input) : a(input.a) {}
            };
        }
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 12 };
        let b = ffi::B::C::make_unique(&a);
        assert_eq!(b.as_ref().unwrap().a, 12);
    };
    run_test("", hdr, rs, &[], &["B::C", "Bob"]);
}

#[test]
fn test_root_ns_cons_arg_nonpod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            Bob(uint32_t _a) :a(_a) {}
            uint32_t a;
        };
        namespace B {
            struct C {
                uint32_t a;
                C(const Bob& input) : a(input.a) {}
            };
        }
    "};
    let rs = quote! {
        let a = ffi::Bob::make_unique(12);
        let b = ffi::B::C::make_unique(&a);
        assert_eq!(b.as_ref().unwrap().a, 12);
    };
    run_test("", hdr, rs, &["Bob"], &["B::C"]);
}

#[test]
fn test_root_ns_func_ret_pod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        namespace B {
            inline Bob daft() { Bob bob; bob.a = 12; return bob; }
        }
    "};
    let rs = quote! {
        assert_eq!(ffi::B::daft().a, 12);
    };
    run_test("", hdr, rs, &["B::daft"], &["Bob"]);
}

#[test]
fn test_root_ns_func_ret_nonpod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        namespace B {
            inline Bob daft() { Bob bob; bob.a = 12; return bob; }
        }
    "};
    let rs = quote! {
        ffi::B::daft().as_ref().unwrap();
    };
    run_test("", hdr, rs, &["B::daft", "Bob"], &[]);
}

#[test]
fn test_root_ns_meth_ret_pod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        namespace B {
            struct C {
                uint32_t a;
                Bob daft() const { Bob bob; bob.a = 12; return bob; }
            };
        }
    "};
    let rs = quote! {
        let b = ffi::B::C { a: 12 };
        assert_eq!(b.daft().a, 12);
    };
    run_test("", hdr, rs, &[], &["Bob", "B::C"]);
}

#[test]
fn test_root_ns_meth_ret_nonpod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        namespace B {
            struct C {
                uint32_t a;
                Bob daft() const { Bob bob; bob.a = 12; return bob; }
            };
        }
    "};
    let rs = quote! {
        let b = ffi::B::C { a: 12 };
        b.daft().as_ref().unwrap();
    };
    run_test("", hdr, rs, &["Bob"], &["B::C"]);
}

#[test]
fn test_forward_declaration() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct A;
        struct B {
            B() {}
            uint32_t a;
            void daft(const A&) {}
            void daft2(std::unique_ptr<A>) {}
        };
    "};
    let rs = quote! {
        ffi::B::make_unique();
    };
    run_test("", hdr, rs, &["B"], &[]);
}

#[test]
fn test_ulong() {
    let hdr = indoc! {"
    inline unsigned long daft(unsigned long a) { return a; }
    "};
    let rs = quote! {
        assert_eq!(ffi::daft(autocxx::c_ulong(34)), autocxx::c_ulong(34));
    };
    run_test("", hdr, rs, &["daft"], &[]);
}

#[test]
fn test_typedef_to_ulong() {
    let hdr = indoc! {"
        #include <cstddef>
        inline size_t daft(size_t a) { return a; }
    "};
    let rs = quote! {
        assert_eq!(ffi::daft(autocxx::c_ulong(34)), autocxx::c_ulong(34));
    };
    run_test("", hdr, rs, &["daft"], &[]);
}

#[test]
fn test_generate_typedef_to_ulong() {
    let hdr = indoc! {"
        #include <cstdint>
        typedef uint32_t fish_t;
    "};
    let rs = quote! {
        let _: ffi::fish_t;
    };
    run_test("", hdr, rs, &[], &["fish_t"]);
}

#[test]
fn test_ulong_method() {
    let hdr = indoc! {"
    class A {
        public:
        A() {};
        unsigned long daft(unsigned long a) const { return a; }
    };
    "};
    let rs = quote! {
        let a = ffi::A::make_unique();
        assert_eq!(a.as_ref().unwrap().daft(autocxx::c_ulong(34)), autocxx::c_ulong(34));
    };
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_ulong_wrapped_method() {
    let hdr = indoc! {"
    #include <cstdint>
    struct B {
        B() {};
        uint32_t a;
    };
    class A {
        public:
        A() {};
        unsigned long daft(unsigned long a, B extra) const { return a; }
    };
    "};
    let rs = quote! {
        let b = ffi::B::make_unique();
        let a = ffi::A::make_unique();
        assert_eq!(a.as_ref().unwrap().daft(autocxx::c_ulong(34), b), autocxx::c_ulong(34));
    };
    run_test("", hdr, rs, &["A", "B"], &[]);
}

#[test]
fn test_reserved_name() {
    let hdr = indoc! {"
        #include <cstdint>
        inline uint32_t async(uint32_t a) { return a; }
    "};
    let rs = quote! {
        assert_eq!(ffi::async_(34), 34);
    };
    run_test("", hdr, rs, &["async_"], &[]);
}

#[test]
fn test_generic_type() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        template<typename TY>
        struct Container {
            Container(TY a_) : a(a_) {}
            TY a;
        };
        struct Secondary {
            Secondary() {}
            void take_a(const Container<char>) const {}
            void take_b(const Container<uint16_t>) const {}
            uint16_t take_c(std::string a) const { return 10 + a.size(); }
        };
    "};
    let rs = quote! {
        use ffi::ToCppString;
        let item = ffi::Secondary::make_unique();
        assert_eq!(item.take_c("hello".to_cpp()), 15)
    };
    run_test("", hdr, rs, &["Secondary"], &[]);
}

#[test]
fn test_cycle_generic_type() {
    let hdr = indoc! {"
        #include <cstdint>
        template<typename TY>
        struct Container {
            Container(TY a_) : a(a_) {}
            TY a;
        };
        inline Container<char> make_thingy() {
            Container<char> a('a');
            return a;
        }
        typedef Container<char> Concrete;
        inline uint32_t take_thingy(Concrete a) {
            return a.a;
        }
    "};
    let rs = quote! {
        assert_eq!(ffi::take_thingy(ffi::make_thingy()), 'a' as u32)
    };
    run_test("", hdr, rs, &["take_thingy", "make_thingy"], &[]);
}

#[test]
fn test_virtual_fns() {
    let hdr = indoc! {"
        #include <cstdint>
        class A {
        public:
            A(uint32_t num) : b(num) {}
            virtual uint32_t foo(uint32_t a) { return a+1; };
            virtual ~A() {}
            uint32_t b;
        };
        class B: public A {
        public:
            B() : A(3), c(4) {}
            virtual uint32_t foo(uint32_t a) { return a+2; };
            uint32_t c;
        };
    "};
    let rs = quote! {
        let mut a = ffi::A::make_unique(12);
        assert_eq!(a.pin_mut().foo(2), 3);
        let mut b = ffi::B::make_unique();
        assert_eq!(b.pin_mut().foo(2), 4);
    };
    run_test("", hdr, rs, &["A", "B"], &[]);
}

#[test]
fn test_const_virtual_fns() {
    let hdr = indoc! {"
        #include <cstdint>
        class A {
        public:
            A(uint32_t num) : b(num) {}
            virtual uint32_t foo(uint32_t a) const { return a+1; };
            virtual ~A() {}
            uint32_t b;
        };
        class B: public A {
        public:
            B() : A(3), c(4) {}
            virtual uint32_t foo(uint32_t a) const { return a+2; };
            uint32_t c;
        };
    "};
    let rs = quote! {
        let a = ffi::A::make_unique(12);
        assert_eq!(a.foo(2), 3);
        let b = ffi::B::make_unique();
        assert_eq!(b.foo(2), 4);
    };
    run_test("", hdr, rs, &["A", "B"], &[]);
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/197
fn test_virtual_fns_inheritance() {
    let hdr = indoc! {"
        #include <cstdint>
        class A {
        public:
            A(uint32_t num) : b(num) {}
            virtual uint32_t foo(uint32_t a) { return a+1; };
            virtual ~A() {}
            uint32_t b;
        };
        class B: public A {
        public:
            B() : A(3), c(4) {}
            uint32_t c;
        };
    "};
    let rs = quote! {
        let mut b = ffi::B::make_unique();
        assert_eq!(b.pin_mut().foo(2), 3);
    };
    run_test("", hdr, rs, &["B"], &[]);
}

#[test]
fn test_vector_cycle_up() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <vector>
        #include <memory>
        struct A {
            uint32_t a;
        };
        inline uint32_t take_vec(std::unique_ptr<std::vector<A>> many_as) {
            return many_as->size();
        }
        inline std::unique_ptr<std::vector<A>> get_vec() {
            auto items = std::make_unique<std::vector<A>>();
            items->push_back(A { 3 });
            items->push_back(A { 4 });
            return items;
        }
    "};
    let rs = quote! {
        let v = ffi::get_vec();
        assert_eq!(v.as_ref().unwrap().is_empty(), false);
        assert_eq!(ffi::take_vec(v), 2);
    };
    run_test("", hdr, rs, &["take_vec", "get_vec"], &[]);
}

#[test]
fn test_vector_cycle_bare() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <vector>
        struct A {
            uint32_t a;
        };
        inline uint32_t take_vec(std::vector<A> many_as) {
            return many_as.size();
        }
        inline std::vector<A> get_vec() {
            std::vector<A> items;
            items.push_back(A { 3 });
            items.push_back(A { 4 });
            return items;
        }
    "};
    let rs = quote! {
        assert_eq!(ffi::take_vec(ffi::get_vec()), 2);
    };
    run_test("", hdr, rs, &["take_vec", "get_vec"], &[]);
}

#[test]
fn test_typedef_to_std() {
    let hdr = indoc! {"
        #include <string>
        typedef std::string my_string;
        inline uint32_t take_str(my_string a) {
            return a.size();
        }
    "};
    let rs = quote! {
        use ffi::ToCppString;
        assert_eq!(ffi::take_str("hello".to_cpp()), 5);
    };
    run_test("", hdr, rs, &["take_str"], &[]);
}

#[test]
fn test_typedef_to_up_in_fn_call() {
    let hdr = indoc! {"
        #include <string>
        #include <memory>
        typedef std::unique_ptr<std::string> my_string;
        inline uint32_t take_str(my_string a) {
            return a->size();
        }
    "};
    let rs = quote! {
        use ffi::ToCppString;
        assert_eq!(ffi::take_str("hello".to_cpp()), 5);
    };
    run_test("", hdr, rs, &["take_str"], &[]);
}

#[test]
fn test_typedef_in_pod_struct() {
    let hdr = indoc! {"
        #include <string>
        typedef uint32_t my_int;
        struct A {
            my_int a;
        };
        inline uint32_t take_a(A a) {
            return a.a;
        }
    "};
    let rs = quote! {
        let a = ffi::A {
            a: 32,
        };
        assert_eq!(ffi::take_a(a), 32);
    };
    run_test("", hdr, rs, &["take_a"], &["A"]);
}

#[test]
fn test_typedef_to_std_in_struct() {
    let hdr = indoc! {"
        #include <string>
        typedef std::string my_string;
        struct A {
            my_string a;
        };
        inline A make_a(std::string b) {
            A bob;
            bob.a = b;
            return bob;
        }
        inline uint32_t take_a(A a) {
            return a.a.size();
        }
    "};
    let rs = quote! {
        use ffi::ToCppString;
        assert_eq!(ffi::take_a(ffi::make_a("hello".to_cpp())), 5);
    };
    run_test("", hdr, rs, &["make_a", "take_a"], &[]);
}

#[test]
fn test_typedef_to_up_in_struct() {
    let hdr = indoc! {"
        #include <string>
        #include <memory>
        typedef std::unique_ptr<std::string> my_string;
        struct A {
            my_string a;
        };
        inline A make_a(std::string b) {
            A bob;
            bob.a = std::make_unique<std::string>(b);
            return bob;
        }
        inline uint32_t take_a(A a) {
            return a.a->size();
        }
    "};
    let rs = quote! {
        use ffi::ToCppString;
        assert_eq!(ffi::take_a(ffi::make_a("hello".to_cpp())), 5);
    };
    run_test("", hdr, rs, &["make_a", "take_a"], &[]);
}

#[test]
fn test_float() {
    let hdr = indoc! {"
    inline float daft(float a) { return a; }
    "};
    let rs = quote! {
        assert_eq!(ffi::daft(34.0f32), 34.0f32);
    };
    run_test("", hdr, rs, &["daft"], &[]);
}

#[test]
fn test_double() {
    let hdr = indoc! {"
    inline double daft(double a) { return a; }
    "};
    let rs = quote! {
        assert_eq!(ffi::daft(34.0f64), 34.0f64);
    };
    run_test("", hdr, rs, &["daft"], &[]);
}

#[test]
fn test_issues_217_222() {
    let hdr = indoc! {"
    #include <string>
    #include <cstdint>

    template <typename STRING_TYPE> class BasicStringPiece {
        public:
         typedef size_t size_type;
         typedef typename STRING_TYPE::traits_type traits_type;
         typedef typename STRING_TYPE::value_type value_type;
         typedef const value_type* pointer;
         typedef const value_type& reference;
         typedef const value_type& const_reference;
         typedef ptrdiff_t difference_type;
         typedef const value_type* const_iterator;
         typedef std::reverse_iterator<const_iterator> const_reverse_iterator;
         static const size_type npos;
    };

    template<typename CHAR>
    class Replacements {
     public:
      Replacements() {
      }
      void SetScheme(const CHAR*) {
      }
      uint16_t a;
    };

    struct Component {
        uint16_t a;
    };

    template <typename STR>
    class StringPieceReplacements : public Replacements<typename STR::value_type> {
        private:
         using CharT = typename STR::value_type;
         using StringPieceT = BasicStringPiece<STR>;
         using ParentT = Replacements<CharT>;
         using SetterFun = void (ParentT::*)(const CharT*, const Component&);
         void SetImpl(SetterFun, StringPieceT) {
        }
        public:
        void SetSchemeStr(const CharT* str) { SetImpl(&ParentT::SetScheme, str); }
    };

    class GURL {
        public:
        typedef StringPieceReplacements<std::string> UrlReplacements;
        GURL() {}
        GURL ReplaceComponents(const Replacements<char>&) const {
            return GURL();
        }
        uint16_t a;
    };
    "};
    let rs = quote! {
        ffi::GURL::make_unique();
    };
    // The block! directives here are to avoid running into
    // https://github.com/rust-lang/rust-bindgen/pull/1975
    run_test_ex(
        "",
        hdr,
        rs,
        &["GURL"],
        &[],
        Some(quote! { block!("StringPiece") block!("Replacements") }),
        &[],
    );
}

#[test]
#[ignore] // https://github.com/rust-lang/rust-bindgen/pull/1975, https://github.com/google/autocxx/issues/106
fn test_dependent_qualified_type() {
    let hdr = indoc! {"
    #include <stddef.h>
    struct MyString {
        typedef char value_type;
    };
    template<typename T> struct MyStringView {
        typedef typename T::value_type view_value_type;
        const view_value_type* start;
        size_t length;
    };
    const char* HELLO = \"hello\";
    MyStringView<MyString> make_string_view() {
        MyStringView<MyString> r;
        r.start = HELLO;
        r.length = 2;
        return r;
    }
    size_t take_string_view(const MyStringView<MyString>& bit) {
        return bit.length;
    }
    "};
    let rs = quote! {
        let sv = ffi::make_string_view();
        assert_eq!(ffi::take_string_view(sv.as_ref().unwrap()), 2);
    };
    run_test("", hdr, rs, &["take_string_view", "make_string_view"], &[]);
}

#[test]
#[ignore] // https://github.com/rust-lang/rust-bindgen/pull/1975, https://github.com/google/autocxx/issues/106
fn test_simple_dependent_qualified_type() {
    let hdr = indoc! {"
    #include <stddef.h>
    #include <stdint.h>
    struct MyString {
        typedef char value_type;
    };
    template<typename T> struct MyStringView {
        typedef typename T::value_type view_value_type;
        const view_value_type* start;
        size_t length;
    };
    typedef MyStringView<MyString>::view_value_type MyChar;
    MyChar make_char() {
        return 'a';
    }
    uint32_t take_char(MyChar c) {
        return static_cast<unsigned char>(c);
    }
    "};
    let rs = quote! {
        let c = ffi::make_char();
        assert_eq!(ffi::take_char(c), 97);
    };
    run_test("", hdr, rs, &["make_char", "take_char"], &[]);
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/266
fn test_take_array() {
    let hdr = indoc! {"
    #include <cstdint>
    uint32_t take_array(const uint32_t a[4]) {
        return a[0] + a[2];
    }
    "};
    let rs = quote! {
        let c: [u32; 4usize] = [ 10, 20, 30, 40 ];
        let c = c as *const [_];
        assert_eq!(ffi::take_array(&c), 40);
    };
    run_test("", hdr, rs, &["take_array"], &[]);
}

#[test]
fn test_issue_264() {
    let hdr = indoc! {"
    namespace a {
        typedef int b;
        inline namespace c {}
        template <typename> class aa;
        namespace c {
        template <typename d, typename = d, typename = aa<d>> class e;
        }
        typedef e<char> f;
        template <typename g, typename, template <typename> typename> struct h {
          using i = g;
        };
        template <typename g, template <typename> class k> using j = h<g, void, k>;
        template <typename g, template <typename> class k>
        using m = typename j<g, k>::i;
        template <typename> struct l { typedef b ab; };
        template <typename p> class aa {
        public:
          typedef p n;
        };
        struct r {
          template <typename p> using o = typename p::c;
        };
        template <typename ad> struct u : r {
          typedef typename ad::n n;
          using ae = m<n, o>;
          template <typename af, typename> struct v { using i = typename l<f>::ab; };
          using ab = typename v<ad, ae>::i;
        };
        } // namespace a
        namespace q {
        template <typename ad> struct w : a::u<ad> {};
        } // namespace q
        namespace a {
        namespace c {
        template <typename, typename, typename ad> class e {
          typedef q::w<ad> s;
        public:
          typedef typename s::ab ab;
        };
        } // namespace c
        } // namespace a
        namespace ag {
        namespace ah {
        typedef a::f::ab t;
        class ai {
          t aj;
        };
        class al;
        namespace am {
        class an {
        public:
          void ao(ai);
        };
        } // namespace am
        class ap {
        public:
          al aq();
        };
        class ar {
          am::an as;
        };
        class al {
          ar at;
        };
        struct au {
          ap av;
        };
        } // namespace ah
        } // namespace ag
        namespace operations_research {
        class aw {
          ag::ah::au ax;
        };
        class Solver {
          aw ay;
        };
        } // namespace operations_research
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["operations_research::Solver"], &[]);
}

#[test]
fn test_unexpected_use() {
    // https://github.com/google/autocxx/issues/303
    let hdr = indoc! {"
        typedef int a;
        namespace b {
        namespace c {
        enum d : a;
        }
        } // namespace b
        namespace {
        using d = b::c::d;
        }
        namespace content {
        class RenderFrameHost {
        public:
            RenderFrameHost() {}
        d e;
        };
        } // namespace content
        "};
    let rs = quote! {
        let _ = ffi::content::RenderFrameHost::make_unique();
    };
    run_test("", hdr, rs, &["content::RenderFrameHost"], &[]);
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/305
fn test_get_pure_virtual() {
    let hdr = indoc! {"
        #include <cstdint>
        class A {
        public:
            virtual uint32_t get_val() const = 0;
        };
        class B : public A {
        public:
            virtual uint32_t get_val() const { return 3; }
        };
        const B b;
        inline const A* get_a() { return &b; };
    "};
    let rs = quote! {
        let a = ffi::get_a();
        let a_ref = unsafe { a.as_ref() }.unwrap();
        assert_eq!(a_ref.get_val(), 3);
    };
    run_test("", hdr, rs, &["get_a"], &[]);
}

#[test]
fn test_vector_of_pointers() {
    // Just ensures the troublesome API is ignored
    let hdr = indoc! {"
        #include <vector>
        namespace operations_research {
        class a;
        class Solver {
        public:
          struct b c(std::vector<a *>);
        };
        class a {};
        } // namespace operations_research
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["operations_research::Solver"], &[]);
}

#[test]
fn test_pointer_to_pointer() {
    // Just ensures the troublesome API is ignored
    let hdr = indoc! {"
        namespace operations_research {
        class a;
        class Solver {
        public:
          struct b c(a **);
        };
        class a {};
        } // namespace operations_research
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["operations_research::Solver"], &[]);
}

#[test]
fn test_defines_effective() {
    let hdr = indoc! {"
        #include <cstdint>
        #ifdef FOO
        inline uint32_t a() { return 4; }
        #endif
    "};
    let rs = quote! {
        ffi::a();
    };
    run_test_ex("", hdr, rs, &["a"], &[], None, &["FOO"]);
}

// Yet to test:
// 6. Ifdef
// 7. Out param pointers
// 10. ExcludeUtilities
// Stuff which requires much more thought:
// 1. Shared pointers
// Negative tests:
// 1. Private methods
// 2. Private fields
