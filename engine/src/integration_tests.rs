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

    fn copy_items_into_temp_dir<P1: AsRef<Path>>(&self, src_path: &P1, pattern: &str) {
        for item in std::fs::read_dir(src_path).unwrap() {
            let item = item.unwrap();
            if item.file_name().into_string().unwrap().contains(pattern) {
                let dest = self.temp_dir.path().join(item.file_name());
                std::fs::copy(item.path(), dest).unwrap();
            }
        }
    }

    fn build<P1: AsRef<Path>, P2: AsRef<Path>, P3: AsRef<Path> + RefUnwindSafe>(
        &self,
        library_path: &P1,
        library_name: &str,
        header_path: &P2,
        header_name: &str,
        rs_path: &P3,
    ) -> std::thread::Result<()> {
        // Copy all items from the source dir into our temporary dir if their name matches
        // the pattern given in `library_name`.
        self.copy_items_into_temp_dir(library_path, library_name);
        self.copy_items_into_temp_dir(header_path, header_name);
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

fn run_test(
    cxx_code: &str,
    header_code: &str,
    rust_code: TokenStream,
    allowed_funcs: &[&str],
    allowed_pods: &[&str],
) {
    // Step 1: Write the C++ header snippet to a temp file
    let tdir = tempdir().unwrap();
    write_to_file(&tdir, "input.h", &format!("#pragma once\n{}", header_code));
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
    let mut b = autocxx_build::Builder::new(&rs_path, tdir.path().to_str().unwrap()).unwrap();
    b.builder()
        .file(cxx_path)
        .out_dir(&target_dir)
        .host(&target)
        .target(&target)
        .opt_level(1)
        .flag("-std=c++14")
        .include(tdir.path())
        .try_compile("autocxx-demo")
        .unwrap();
    // Step 8: use the trybuild crate to build the Rust file.
    let r = BUILDER.lock().unwrap().build(
        &target_dir,
        "autocxx-demo",
        &tdir.path(),
        "input.h",
        &rs_path,
    );
    if KEEP_TEMPDIRS {
        println!("Tempdir: {:?}", tdir.into_path().to_str());
    }
    r.unwrap()
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
#[ignore] // because we don't yet support std::string by value
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
        assert_eq!(ffi::cxxbridge::give_str_up().to_str().unwrap(), "Bob");
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
#[ignore] // because we don't yet support std::string by value
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
#[ignore] // need field access to opaque types; need to pass Rust-side unique pointer
          // into C++ functions which take something by value
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
        struct Bob {
            Bob(uint32_t a, uint32_t b);
            uint32_t a;
            uint32_t b;
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
#[ignore] // because we don't yet support std::string by value
fn test_cycle_nonpod_with_str_by_value() {
    let cxx = indoc! {"
        uint32_t take_bob(Bob a) {
            return a.a;
        }
        std::string get_str() {
            return \"hello\";
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Bob {
            uint32_t a;
            std::string b;
        };
        uint32_t take_bob(Bob a);
        std::string get_str();
    "};
    let rs = quote! {
        let a = ffi::cxxbridge::Bob { a: 12, b: ffi::cxxbridge::get_str() };
        assert_eq!(ffi::cxxbridge::take_bob(a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob", "Bob", "get_str"], &[]);
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
    // TODO rename Bob_make_unique to Bob::make_unique
    let rs = quote! {
        let a = ffi::cxxbridge::Bob_make_unique(); // TODO test with all sorts of arguments.
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
fn test_enum() {
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

#[test]
#[ignore] // works, but causes compile warnings
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

// Yet to test:
// 1. Make UniquePtr<CxxStrings> in Rust
// 3. Constants
// 5. Templated stuff
// 6. Ifdef
// 7. Out params
// 8. Opaque type handling
// 9. Multiple functions in one header
// Stuff which requires much more thought:
// 1. Shared pointers
// Negative tests:
// 1. Private methods
// 2. Private fields
