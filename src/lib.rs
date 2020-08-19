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

#![feature(proc_macro_span)]

mod engine;

#[cfg(test)]
mod tests {

    use super::engine::{CppInclusion, IncludeCpp};
    use indoc::indoc;
    use log::info;
    use proc_macro2::TokenStream;
    use quote::quote;
    use std::fs::File;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use tempfile::{tempdir, TempDir};
    use test_env_log::test;

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
    ) {
        //println!("C++ is {}, Rust is {}", cxx_code, rust_code);
        // To do...
        // Step 1: Write the C++ header snippet to a temp file
        let tdir = tempdir().unwrap();
        write_to_file(&tdir, "input.h", header_code);
        // Step 2: Modify the Rust code to add:
        //         include_cpp!(path to header, allowlist)
        //         and also #[link(name="foo", kind="static")]
        //         Ensure that the TokenStream passed to us is contained
        //         within a main() function.
        // TODO - at the moment we're expanding this in the test
        // code, which is cheating. In the real world, cxxbridge
        // would need to know how to expand include_cpp!
        // to make a #[cxx::bridge] mod. Because that's not currently
        // possible, there's no way that any of this can work outside
        // of this test code environment. Yet.
        let allowed_funcs = allowed_funcs.iter().map(|s| (*s).to_string()).collect();
        let incl_cpp = IncludeCpp::new(
            vec![CppInclusion::Header("input.h".to_string())],
            allowed_funcs,
            tdir.path().to_path_buf());
        let bindings = incl_cpp.run();
        let expanded_rust = quote! {
            #bindings

            fn main() {
                #rust_code
            }
        };
        // Step 3: Write the Rust code to a temp file
        let rs_code = format!("{}", expanded_rust);
        let rs_path = write_to_file(&tdir, "input.rs", &rs_code);

        // Step 4: Write the C++ code snippet to a .cc file, along with a #include
        //         of the header emitted in step 5.
        let cxx_code = format!("#include \"{}\"\n{}", "input.h", cxx_code);
        let cxx_path = write_to_file(&tdir, "input.cxx", &cxx_code);

        // Step 5: Run cxxbridge over the Rust file (or the programmatic equivalent)
        //         It should emit a .cc and a .h file

        info!("Path is {:?}", tdir.path());
        // TODO - find a better way to feed the OUT_DIR to cxx than this.
        let target_dir = tdir.path().join("target");
        std::fs::create_dir(&target_dir).unwrap();
        std::env::set_var("OUT_DIR", &target_dir);
        let target = rust_info::get().target_triple.unwrap();
        let (hdr, cc) = cxx_gen::generate_header_and_cc(expanded_rust).unwrap();
        write_to_file(&tdir, "output.h", std::str::from_utf8(&hdr).unwrap());
        let gen_cxx_path = write_to_file(&tdir, "output.cxx", std::str::from_utf8(&cc).unwrap());

        // Step 7: Use the cc crate to build a static library from both .cc files
        //         ensuring it matches the name you gave in step 2
        cc::Build::new()
            .file(gen_cxx_path)
            .file(cxx_path)
            .cpp(true)
            .host(&target)
            .target(&target)
            .opt_level(1)
            .flag("-std=c++11")
            .include(tdir.path())
            .try_compile("autocxx-demo")
            .unwrap();
        // Step 8: use the trybuild crate to build the Rust file.
        // TODO - find a better way to persuade trybuild to link against
        // our static library.
        let wrapper_path = write_to_file(
            &tdir,
            "wrapper.sh",
            indoc! {"
            #!/bin/bash
            set -e
            RUSTC=$1
            shift
            $RUSTC -C link-arg=-L$AUTOCXX_LIBRARY_PATH -C link-arg=-lautocxx-demo $@
            "},
        );
        std::fs::set_permissions(&wrapper_path, PermissionsExt::from_mode(0o755)).unwrap();
        std::env::set_var("AUTOCXX_LIBRARY_PATH", target_dir);
        std::env::set_var("RUSTC_WRAPPER", wrapper_path);
        let t = trybuild::TestCases::new();
        t.pass(rs_path);

        // Details: the allowlist might need to be split into functions/types/etc. TBD
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
        run_test(cxx, hdr, rs, &["do_nothing"]);
    }

    #[test]
    fn test_return_i32() {
        let cxx = indoc! {"
            uint32_t give_int() {
                return 4;
            }
        "};
        let hdr = indoc! {"
            #include <cstdint>
            uint32_t give_int();
        "};
        let rs = quote! {
            assert_eq!(ffi::give_int(), 4);
        };
        run_test(cxx, hdr, rs, &["give_int"]);
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
        run_test(cxx, hdr, rs, &["take_int"]);
    }

    #[test]
    fn test_give_up() {
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
        run_test(cxx, hdr, rs, &["give_up"]);
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
        run_test(cxx, hdr, rs, &["give_str_up"]);
    }

    #[test]
    fn test_give_string() {
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
            assert_eq!(ffi::give_str_up().to_str().unwrap(), "Bob");
        };
        run_test(cxx, hdr, rs, &["give_str"]);
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
            let s = ffi::give_str();
            assert_eq!(ffi::take_str(s), 3);
        };
        run_test(cxx, hdr, rs, &["give_str_up", "take_str_up"]);
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
        let allowed_funcs = &["give_str", "take_str"];
        run_test(cxx, hdr, rs, allowed_funcs);
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
        run_test(cxx, hdr, rs, &["give_bob", "Bob"]);
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
        run_test(cxx, hdr, rs, &["give_bob", "Bob"]);
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
            int take_bob(Bob a);
        "};
        let rs = quote! {
            let a = ffi::Bob { a: 12, b: 13 };
            assert_eq!(ffi::take_bob(a), 12);
        };
        run_test(cxx, hdr, rs, &["take_bob", "Bob"]);
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
            int take_bob(const Bob& a);
        "};
        let rs = quote! {
            let a = ffi::Bob { a: 12, b: 13 };
            assert_eq!(ffi::take_bob(&a), 12);
        };
        let allowed_funcs = &["take_bob", "Bob"];
        run_test(cxx, hdr, rs, allowed_funcs);
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
            int take_bob(const Bob& a);
        "};
        let rs = quote! {
            let mut a = ffi::Bob { a: 12, b: 13 };
            assert_eq!(ffi::take_bob(&mut a), 12);
            assert_eq!(a.b, 14);
        };
        run_test(cxx, hdr, rs, &["take_bob", "Bob"]);
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
            int take_bob(Bob a);
        "};
        let rs = quote! {
            let a = ffi::Bob { a: 12, b: 13, c: ffi::Phil { d: 4 } };
            assert_eq!(ffi::take_bob(a), 12);
        };
        // Should be no need to allowlist Phil below
        let allowed_funcs = &["take_bob", "Bob"];
        run_test(cxx, hdr, rs, allowed_funcs);
    }

    #[test]
    fn test_cycle_pod_with_str_by_value() {
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
            int take_bob(Bob a);
            std::string get_str();
        "};
        let rs = quote! {
            let a = ffi::Bob { a: 12, b: ffi::get_str() };
            assert_eq!(ffi::take_bob(a), 12);
        };
        run_test(cxx, hdr, rs, &["take_bob", "Bob", "get_str"]);
    }

    #[test]
    fn test_cycle_pod_with_str_by_ref() {
        let cxx = indoc! {"
            uint32_t take_bob(const Bob& a) {
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
            int take_bob(const Bob& a);
            std::string get_str();
        "};
        let rs = quote! {
            let a = ffi::Bob { a: 12, b: ffi::get_str() };
            assert_eq!(ffi::take_bob(&a), 12);
        };
        run_test(cxx, hdr, rs, &["take_bob", "Bob", "get_str"]);
    }

    #[test]
    fn test_make_up() {
        let cxx = indoc! {"
            Bob::Bob() : a(3) {
            }
        "};
        let hdr = indoc! {"
            #include <cstdint>
            class Bob {
            public:
                Bob();
                uint32_t a;
            };
        "};
        let rs = quote! {
            let a = ffi::Bob::make_unique();
            assert_eq!(a.as_ref().unwrap().a, 3);
        };
        run_test(cxx, hdr, rs, &["Bob"]);
    }

    #[test]
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
        run_test(cxx, hdr, rs, &["Bob"]);
    }

    // Yet to test:
    // 1. Make UniquePtr<CxxStrings> in Rust
    // 2. Enums
    // 3. Constants
    // 4. Call methods
    // 5. Templated stuff
    // 6. Preprocessor directives
    // 7. Out params
    // 8. Opaque type handling
    // Stuff which requires much more thought:
    // 1. Shared pointers
    // Negative tests:
    // 1. Private methods
    // 2. Private fields
}
