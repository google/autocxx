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
use proc_macro2::TokenStream;
use quote::quote;
use std::fs::File;
use std::io::Write;
use std::panic::RefUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
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
    ) -> std::thread::Result<()> {
        // Copy all items from the source dir into our temporary dir if their name matches
        // the pattern given in `library_name`.
        self.move_items_into_temp_dir(library_path, library_name);
        for header_name in header_names {
            self.move_items_into_temp_dir(header_path, header_name);
        }
        let temp_path = self.temp_dir.path().to_str().unwrap();
        std::env::set_var("RUSTFLAGS", format!("-L {}", temp_path));
        std::env::set_var("AUTOCXX_INC", temp_path);
        std::panic::catch_unwind(|| {
            let test_cases = trybuild::TestCases::new();
            test_cases.pass(rs_path)
        })
    }
}

fn write_to_file(tdir: &TempDir, filename: &str, content: &str) -> PathBuf {
    let path = tdir.path().join(filename);
    let mut f = File::create(&path).unwrap();
    info!("Writing to {:?}: {}", path, content);
    f.write_all(content.as_bytes()).unwrap();
    path
}

/// A positive test, we expect to pass.
fn run_test(
    cxx_code: &str,
    header_code: &str,
    rust_code: TokenStream,
    allowed_funcs: &[&str],
    allowed_pods: &[&str],
) {
    do_run_test(
        cxx_code,
        header_code,
        rust_code,
        allowed_funcs,
        allowed_pods,
    )
    .unwrap()
}

fn run_test_expect_fail(
    cxx_code: &str,
    header_code: &str,
    rust_code: TokenStream,
    allowed_funcs: &[&str],
    allowed_pods: &[&str],
) {
    do_run_test(
        cxx_code,
        header_code,
        rust_code,
        allowed_funcs,
        allowed_pods,
    )
    .expect_err("Unexpected success");
}

/// In the future maybe the tests will distinguish the exact type of failure expected.
#[derive(Debug)]
enum TestError {
    AutoCxx(autocxx_build::Error),
    CppBuild(cc::Error),
    RsBuild,
}

fn do_run_test(
    cxx_code: &str,
    header_code: &str,
    rust_code: TokenStream,
    allowed_funcs: &[&str],
    allowed_pods: &[&str],
) -> Result<(), TestError> {
    // Step 1: Write the C++ header snippet to a temp file
    let tdir = tempdir().unwrap();
    write_to_file(&tdir, "input.h", &format!("#pragma once\n{}", header_code));
    write_to_file(&tdir, "cxx.h", crate::HEADER);
    // Step 2: Expand the snippet of Rust code into an entire
    //         program including include_cxx!
    // TODO - we're not quoting #s below (in the "" sense), and it's not entirely
    // clear how we're getting away with it, but quoting it doesn't work.
    let allowed_funcs = allowed_funcs.iter().map(|s| {
        quote! {
            Allow(#s),
        }
    });
    let allowed_pods = allowed_pods.iter().map(|s| {
        quote! {
            AllowPOD(#s),
        }
    });
    let expanded_rust = quote! {
        use autocxx::include_cxx;

        include_cxx!(
            Header("input.h"),
            #(#allowed_funcs)*
            #(#allowed_pods)*
        );

        fn main() {
            #rust_code
        }

        #[link(name="autocxx-demo")]
        extern {}
    };
    info!("Expanded Rust: {}", expanded_rust);
    // Step 3: Write the Rust code to a temp file
    let rs_code = format!("{}", expanded_rust);
    let rs_path = write_to_file(&tdir, "input.rs", &rs_code);

    // Step 4: Write the C++ code snippet to a .cc file, along with a #include
    //         of the header emitted in step 5.
    let cxx_code = format!("#include \"{}\"\n{}", "input.h", cxx_code);
    let cxx_path = write_to_file(&tdir, "input.cxx", &cxx_code);

    info!("Path is {:?}", tdir.path());
    let target_dir = tdir.path().join("target");
    std::fs::create_dir(&target_dir).unwrap();
    let target = rust_info::get().target_triple.unwrap();
    let mut b = autocxx_build::build_to_custom_directory(
        &rs_path,
        tdir.path().to_str().unwrap(),
        target_dir.clone(),
    )
    .map_err(TestError::AutoCxx)?;
    b.file(cxx_path)
        .out_dir(&target_dir)
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
        ffi::cxxbridge::do_nothing();
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
        ffi::cxxbridge::do_nothing1();
        ffi::cxxbridge::do_nothing2();
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
        ffi::cxxbridge::do_nothing1();
        ffi::cxxbridge::do_nothing2();
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
        assert_eq!(ffi::cxxbridge::give_int(), 5);
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
        assert_eq!(ffi::cxxbridge::take_int(3), 6);
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
        assert_eq!(ffi::cxxbridge::give_up().as_ref().unwrap(), 12);
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
        assert_eq!(ffi::cxxbridge::give_str_up().as_ref().unwrap().to_str().unwrap(), "Bob");
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
        assert_eq!(ffi::cxxbridge::give_str().as_ref().unwrap(), "Bob");
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
        let s = ffi::cxxbridge::give_str_up();
        assert_eq!(ffi::cxxbridge::take_str_up(s), 3);
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
        let s = ffi::cxxbridge::give_str();
        assert_eq!(ffi::cxxbridge::take_str(s), 3);
    };
    let allowed_funcs = &["give_str", "take_str"];
    run_test(cxx, hdr, rs, allowed_funcs, &[]);
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
        let s = ffi::cxxbridge::give_str();
        assert_eq!(ffi::cxxbridge::take_str(s.as_ref().unwrap()), 3);
    };
    let allowed_funcs = &["give_str", "take_str"];
    run_test(cxx, hdr, rs, allowed_funcs, &[]);
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
        let mut s = ffi::cxxbridge::give_str();
        assert_eq!(ffi::cxxbridge::take_str(s.as_mut().unwrap()), 3);
    };
    let allowed_funcs = &["give_str", "take_str"];
    run_test(cxx, hdr, rs, allowed_funcs, &[]);
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
        assert_eq!(ffi::cxxbridge::give_bob().b, 4);
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
        assert_eq!(ffi::cxxbridge::give_bob().b, 4);
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
        assert_eq!(ffi::cxxbridge::give_bob().as_ref().unwrap().b, 4);
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
        let a = ffi::cxxbridge::Bob { a: 12, b: 13 };
        assert_eq!(ffi::cxxbridge::take_bob(a), 12);
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
        let a = ffi::cxxbridge::Bob { a: 12, b: 13 };
        assert_eq!(ffi::cxxbridge::take_bob(&a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob"], &["Bob"]);
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
        let mut a = ffi::cxxbridge::Bob { a: 12, b: 13 };
        assert_eq!(ffi::cxxbridge::take_bob(&mut a), 12);
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
        let a = ffi::cxxbridge::Bob { a: 12, b: 13, c: ffi::cxxbridge::Phil { d: 4 } };
        assert_eq!(ffi::cxxbridge::take_bob(a), 12);
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
        let a = ffi::cxxbridge::Bob_make_unique(12, 13);
        assert_eq!(ffi::cxxbridge::take_bob(a), 12);
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
        let a = ffi::cxxbridge::make_bob(12);
        assert_eq!(ffi::cxxbridge::take_bob(&a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob", "Bob", "make_bob"], &[]);
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
        let mut a = ffi::cxxbridge::make_bob(12);
        assert_eq!(ffi::cxxbridge::take_bob(&mut a), 12);
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
        let a = ffi::cxxbridge::give_bob(13);
        assert_eq!(ffi::cxxbridge::take_bob(a), 13);
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
        assert_eq!(ffi::cxxbridge::get_str().as_ref().unwrap(), "hello");
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
        assert_eq!(ffi::cxxbridge::get_str().as_ref().unwrap(), "hello");
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
        let a = ffi::cxxbridge::make_bob();
        assert_eq!(ffi::cxxbridge::take_bob(a.as_ref().unwrap()), 32);
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
        let a = ffi::cxxbridge::Bob::make_unique(); // TODO test with all sorts of arguments.
        assert_eq!(ffi::cxxbridge::take_bob(a.as_ref().unwrap()), 3);
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
        let a = ffi::cxxbridge::Bob_make_unique(12, 13);
        assert_eq!(ffi::cxxbridge::take_bob(a.as_ref().unwrap()), 12);
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
        let a = ffi::cxxbridge::Bob::make_unique(3);
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
        let a = ffi::cxxbridge::Bob::BOB_VALUE_2;
        let b = ffi::cxxbridge::give_bob();
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
        let a = ffi::cxxbridge::Bob::BOB_VALUE_1;
        let b = ffi::cxxbridge::Bob::BOB_VALUE_2;
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
        let a = ffi::cxxbridge::Bob { a: 12, b: 13 };
        assert_eq!(ffi::cxxbridge::take_bob(a), 12);
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
        let a = ffi::cxxbridge::Bob { a: 12, b: 13 };
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
        let mut a = ffi::cxxbridge::Bob { a: 12, b: 13 };
        assert_eq!(a.get_bob(), 12);
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
        assert_eq!(ffi::defs::BOB, 3);
    };
    run_test(cxx, hdr, rs, &[], &[]);
}

#[test]
fn test_define_str() {
    let cxx = indoc! {"
    "};
    let hdr = indoc! {"
        #define BOB \"foo\"
    "};
    let rs = quote! {
        assert_eq!(ffi::defs::BOB, "foo");
    };
    run_test(cxx, hdr, rs, &[], &[]);
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
        ffi::cxxbridge::Bob {};
    };
    let rs2 = quote! {
        ffi::cxxbridge::Bob { a: 12 };
    };
    let rs3 = quote! {
        ffi::cxxbridge::Bob { do_not_attempt_to_allocate_nonpod_types: [] };
    };
    run_test_expect_fail(cxx, hdr, rs, &["take_bob", "Bob", "make_bob"], &[]);
    run_test_expect_fail(cxx, hdr, rs2, &["take_bob", "Bob", "make_bob"], &[]);
    run_test_expect_fail(cxx, hdr, rs3, &["take_bob", "Bob", "make_bob"], &[]);
}

#[test]
fn test_method_pass_pod_by_value() {
    let cxx = indoc! {"
        uint32_t Bob::get_bob(Anna z) const {
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
        let a = ffi::cxxbridge::Anna { a: 14 };
        let b = ffi::cxxbridge::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob"], &["Bob", "Anna"]);
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
        let a = ffi::cxxbridge::Anna { a: 14 };
        let b = ffi::cxxbridge::Bob { a: 12, b: 13 };
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
        let mut a = ffi::cxxbridge::Anna { a: 14 };
        let b = ffi::cxxbridge::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(&mut a), 12);
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
        let a = ffi::cxxbridge::Anna { a: 14 };
        let b = ffi::cxxbridge::Bob { a: 12, b: 13 };
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
        let a = ffi::cxxbridge::give_anna();
        let b = ffi::cxxbridge::Bob { a: 12, b: 13 };
        assert_eq!(ffi::cxxbridge::get_bob(&b, a), 12);
        // assert_eq!(b.get_bob(a), 12); // eventual goal
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
        let a = ffi::cxxbridge::give_anna();
        let b = ffi::cxxbridge::Bob { a: 12, b: 13 };
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
        let mut a = ffi::cxxbridge::give_anna();
        let b = ffi::cxxbridge::Bob { a: 12, b: 13 };
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
        let a = ffi::cxxbridge::give_anna();
        let b = ffi::cxxbridge::Bob { a: 12, b: 13 };
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
        let b = ffi::cxxbridge::Bob { a: 12, b: 13 };
        // let a = b.get_bob(); // eventual goal
        let a = ffi::cxxbridge::get_anna(&b);
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
        let a = ffi::cxxbridge::get_msg();
        let c = ffi::cxxbridge::measure_string(a);
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
        let a = ffi::cxxbridge::get_msg();
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
        let a = ffi::cxxbridge::get_msg();
        let b = ffi::cxxbridge::Bob { a: 12, b: 13 };
        let c = ffi::cxxbridge::measure_string(&b, a);
        // let c = b.measure_string(a); // eventual goal
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
        let b = ffi::cxxbridge::Bob { a: 12, b: 13 };
        // let a = b.get_msg(); // eventual goal
        let a = ffi::cxxbridge::get_msg(&b);
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
        let c = ffi::cxxbridge::measure_string(&"hello".to_string());
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
        let c = ffi::cxxbridge::measure_string("hello".into());
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
        let c = ffi::cxxbridge::measure_string("hello");
        assert_eq!(c, 5);
    };
    run_test(cxx, hdr, rs, &["measure_string"], &[]);
}

// Yet to test:
// 1. Make UniquePtr<CxxStrings> in Rust
// 3. Constants
// 5. Templated stuff
// 6. Ifdef
// 7. Out params
// 8. Opaque type handling
// 9. Multiple functions in one header
// 10. ExcludeUtilities
// Stuff which requires much more thought:
// 1. Shared pointers
// Negative tests:
// 1. Private methods
// 2. Private fields
