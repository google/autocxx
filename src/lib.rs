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

#![feature(proc_macro_diagnostic)]

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;

use quote::ToTokens;
use syn::parse::{Parse, ParseStream, Result};

use syn::{parse_macro_input, ItemMod};

use log::debug;

enum CppInclusion {
    Define(String),
    Header(String),
}

struct IncludeCpp<'a> {
    inclusions: Vec<CppInclusion>,
    allowlist: Vec<&'a str>,
    inc_dir: &'a str, // TODO make more versatile
}

impl<'a> Parse for IncludeCpp<'a> {
    fn parse(_input: ParseStream) -> Result<Self> {
        // TODO: Takes as inputs:
        // 1. List of headers to include
        // 2. List of #defines to include
        // 3. Allowlist
        Ok(IncludeCpp {
            inclusions: vec![],
            allowlist: vec![],
            inc_dir: "",
        })
    }
}

impl<'a> IncludeCpp<'a> {
    fn build_header(&self) -> String {
        let mut s = String::new();
        for incl in &self.inclusions {
            let text = match incl {
                CppInclusion::Define(symbol) => format!("#define {}\n", symbol),
                CppInclusion::Header(path) => format!("#include \"{}\"\n", path),
            };
            s.push_str(&text);
        }
        s
    }

    fn make_builder(&self) -> bindgen::Builder {
        let full_header = self.build_header();
        debug!("Full header: {}", full_header);

        // TODO - pass headers in &self.inclusions into
        // bindgen such that it can include them in the generated
        // extern "C" section as include!
        let mut builder = bindgen::builder().clang_arg(format!("-I{}", self.inc_dir))
            .header_contents("example.h", &full_header);

        // 3. Passes allowlist and other options to the bindgen::Builder equivalent
        //    to --output-style=cxx --allowlist=<as passed in>
        for a in &self.allowlist {
            // TODO - allowlist type/functions/separately
            builder = builder.whitelist_type(a);
            builder = builder.whitelist_function(a);
        }
        builder
    }

    fn run(self) -> TokenStream2 {
        // TODO:
        // 4. (also respects environment variables to pick up more headers,
        //     include paths and #defines)
        // Then:
        // 1. Builds an overall C++ header with all those #defines and #includes
        // 2. Passes it to bindgen::Builder::header
        let bindings = self.make_builder().generate().unwrap().to_string();
        debug!("Bindings: {}", bindings);
        let bindings = syn::parse_str::<ItemMod>(&bindings).unwrap();
        let mut ts = TokenStream2::new();
        bindings.to_tokens(&mut ts);
        ts
    }
}

#[proc_macro]
pub fn include_cpp(input: TokenStream) -> TokenStream {
    let include_cpp = parse_macro_input!(input as IncludeCpp);

    let ts = include_cpp.run();
    // TODO: consider that this quoted section invokes a different procedural
    // macro and what that means.
    TokenStream::from(ts)
}

#[cfg(test)]
mod tests {

    use crate::{IncludeCpp, CppInclusion};
    use indoc::indoc;
    use proc_macro2::TokenStream;
    use quote::quote;
    use std::io::Write;
    use std::fs::File;
    use tempfile::{tempdir, TempDir};
    use std::path::PathBuf;
    use test_env_log::test;
    use log::info;
    use std::os::unix::fs::PermissionsExt;

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
        allowed_funcs: Vec<&str>,
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
        let incl_cpp = IncludeCpp {
            inclusions: vec![CppInclusion::Header("input.h".to_string())],
            allowlist: allowed_funcs,
            inc_dir: tdir.path().to_str().unwrap(),
        };
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
        //   TODO - can it cope with nested macros? What is required to allow
        //          cxxbridge to deal with include_cpp generating a cxx::bridge mod?

        // Step 7: Use the cc crate to build a static library from both .cc files
        //         ensuring it matches the name you gave in step 2
        info!("Path is {:?}", tdir.path());
        // TODO - find a better way to feed the OUT_DIR to cxx than this.
        let target_dir = tdir.path().join("target");
        std::fs::create_dir(&target_dir).unwrap();
        std::env::set_var("OUT_DIR", &target_dir);
        // TODO - oh dear oh dear.
        let target = rust_info::get().target_triple.unwrap();
        std::env::set_var("TARGET", &target);
        std::env::set_var("HOST", target);
        std::env::set_var("OPT_LEVEL", "1");
        cxx_build::bridge(&rs_path)
            .file(cxx_path)
            .flag("-std=c++11")
            .include(tdir.path())
            .compile("autocxx-demo");
        // Step 8: use the trybuild crate to build the Rust file.
        // TODO - find a better way to persuade trybuild to link against
        // our static library.
        let wrapper_path = write_to_file(&tdir, "wrapper.sh", indoc! {"
            #!/bin/bash
            set -e
            RUSTC=$1
            shift
            $RUSTC -C link-arg=-L$AUTOCXX_LIBRARY_PATH -C link-arg=-lautocxx-demo $@
            "});
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
        run_test(cxx, hdr, rs, vec!["do_nothing"]);
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
            assert_eq!(ffi::give_int(), 7);
        };
        run_test(cxx, hdr, rs, vec!["give_int"]);
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
            assert_eq!(ffi::take_int(3), 7);
        };
        run_test(cxx, hdr, rs, vec!["take_int"]);
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
        run_test(cxx, hdr, rs, vec!["give_up"]);
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
        run_test(cxx, hdr, rs, vec!["give_str_up"]);
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
        run_test(cxx, hdr, rs, vec!["give_str"]);
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
        run_test(cxx, hdr, rs, vec!["give_str_up", "take_str_up"]);
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
        let allowed_funcs = vec!["give_str", "take_str"];
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
            }
            Bob give_bob();
        "};
        let rs = quote! {
            assert_eq!(ffi::give_bob().b, 4);
        };
        run_test(cxx, hdr, rs, vec!["give_bob", "Bob"]);
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
            }
            std::unique_ptr<Bob> give_bob();
        "};
        let rs = quote! {
            assert_eq!(ffi::give_bob().as_ref().unwrap().b, 4);
        };
        run_test(cxx, hdr, rs, vec!["give_bob", "Bob"]);
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
            }
            int take_bob(Bob a);
        "};
        let rs = quote! {
            let a = ffi::Bob { a: 12, b: 13 };
            assert_eq!(ffi::take_bob(a), 12);
        };
        run_test(cxx, hdr, rs, vec!["take_bob", "Bob"]);
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
            }
            int take_bob(const Bob& a);
        "};
        let rs = quote! {
            let a = ffi::Bob { a: 12, b: 13 };
            assert_eq!(ffi::take_bob(&a), 12);
        };
        let allowed_funcs = vec!["take_bob", "Bob"];
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
            }
            int take_bob(const Bob& a);
        "};
        let rs = quote! {
            let mut a = ffi::Bob { a: 12, b: 13 };
            assert_eq!(ffi::take_bob(&mut a), 12);
            assert_eq!(a.b, 14);
        };
        run_test(cxx, hdr, rs, vec!["take_bob", "Bob"]);
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
            }
            struct Bob {
                uint32_t a;
                uint32_t b;
                Phil c;
            }
            int take_bob(Bob a);
        "};
        let rs = quote! {
            let a = ffi::Bob { a: 12, b: 13, c: ffi::Phil { d: 4 } };
            assert_eq!(ffi::take_bob(a), 12);
        };
        // Should be no need to allowlist Phil below
        let allowed_funcs = vec!["take_bob", "Bob"];
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
            }
            int take_bob(Bob a);
            std::string get_str();
        "};
        let rs = quote! {
            let a = ffi::Bob { a: 12, b: ffi::get_str() };
            assert_eq!(ffi::take_bob(a), 12);
        };
        run_test(cxx, hdr, rs, vec!["take_bob", "Bob", "get_str"]);
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
            }
            int take_bob(const Bob& a);
            std::string get_str();
        "};
        let rs = quote! {
            let a = ffi::Bob { a: 12, b: ffi::get_str() };
            assert_eq!(ffi::take_bob(&a), 12);
        };
        run_test(cxx, hdr, rs, vec!["take_bob", "Bob", "get_str"]);
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
        run_test(cxx, hdr, rs, vec!["Bob"]);
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
        run_test(cxx, hdr, rs, vec!["Bob"]);
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
