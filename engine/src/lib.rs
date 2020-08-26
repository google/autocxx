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

use proc_macro2::TokenStream as TokenStream2;
use std::path::PathBuf;

use quote::ToTokens;
use syn::parse::{Parse, ParseStream, Result as ParseResult};

use cxx_gen::GeneratedCode;
use syn::{ItemMod, Macro};

use log::{debug, warn, info};
use osstrtools::OsStrTools;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Bindgen(()),
    CxxGen(cxx_gen::Error),
    Parsing(syn::Error),
    NoAutoCxxInc,
    CouldNotCanoncalizeIncludeDir(PathBuf),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

pub enum CppInclusion {
    Define(String),
    Header(String),
}

/// Core of the autocxx engine.
/// TODO - consider whether this 'engine' crate should actually be a
/// directory of source symlinked from all the other sub-crates, so that
/// we avoid exposing an external interface from this code.
pub struct IncludeCpp {
    inclusions: Vec<CppInclusion>,
    allowlist: Vec<String>,
    preconfigured_inc_dirs: Option<std::ffi::OsString>,
}

impl Parse for IncludeCpp {
    fn parse(input: ParseStream) -> ParseResult<Self> {
        Self::new_from_parse_stream(input)
    }
}

impl IncludeCpp {
    fn new_from_parse_stream(input: ParseStream) -> syn::Result<Self> {
        // TODO: Takes as inputs:
        // 1. List of headers to include
        // 2. List of #defines to include
        // 3. Allowlist

        let mut inclusions = Vec::new();
        let mut allowlist = Vec::new();

        while !input.is_empty() {
            let ident: syn::Ident = input.parse()?;
            if ident == "Header" {
                let args;
                syn::parenthesized!(args in input);
                let hdr: syn::LitStr = args.parse()?;
                inclusions.push(CppInclusion::Header(hdr.value()));
            } else if ident == "Allow" {
                let args;
                syn::parenthesized!(args in input);
                let allow: syn::LitStr = args.parse()?;
                allowlist.push(allow.value());
            } else {
                return Err(syn::Error::new(ident.span(), "expected Header or Allow"));
            }
            if input.is_empty() {
                break;
            }
            input.parse::<syn::Token![,]>()?;
        }

        Ok(IncludeCpp {
            inclusions,
            allowlist,
            preconfigured_inc_dirs: None,
        })
    }

    pub fn new_from_syn(mac: Macro) -> Result<Self> {
        mac.parse_body::<IncludeCpp>().map_err(Error::Parsing)
    }

    pub fn set_include_dirs<P: AsRef<std::ffi::OsStr>>(&mut self, include_dirs: P) {
        self.preconfigured_inc_dirs = Some(include_dirs.as_ref().into());
    }

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

    fn determine_incdirs(&self) -> Result<Vec<PathBuf>> {
        let inc_dirs = match &self.preconfigured_inc_dirs {
            Some(d) => d.clone(),
            None => std::env::var_os("AUTOCXX_INC").ok_or(Error::NoAutoCxxInc)?,
        };
        // TODO consider if we can or should look up the include path automatically
        // instead of requiring callers always to set AUTOCXX_INC.
        let multi_path_separator = if std::path::MAIN_SEPARATOR == '/' {
            b':'
        } else {
            b';'
        }; // there's probably a crate for this
        let splitter = [multi_path_separator];
        let inc_dirs = inc_dirs.split(&splitter[0..1]);
        let mut inc_dir_paths = Vec::new();
        for inc_dir in inc_dirs {
            let p: PathBuf = inc_dir.into();
            let p = p
                .canonicalize()
                .map_err(|_| Error::CouldNotCanoncalizeIncludeDir(p))?;
            inc_dir_paths.push(p);
        }
        Ok(inc_dir_paths)
    }

    fn make_bindgen_builder(&self) -> Result<bindgen::Builder> {
        let inc_dirs = self.determine_incdirs()?;

        let full_header = self.build_header();
        debug!("Full header: {}", full_header);
        debug!("Inc dir: {:?}", inc_dirs);

        // TODO - pass headers in &self.inclusions into
        // bindgen such that it can include them in the generated
        // extern "C" section as include!
        // TODO work with OsStrs here to avoid the .display()
        let mut builder = bindgen::builder()
            .clang_args(&["-x", "c++", "-std=c++14"])
            .cxx_bridge(true)
            .derive_copy(false)
            .derive_debug(false)
            .layout_tests(false); // TODO revisit later
        for incl in &self.inclusions {
            match incl {
                CppInclusion::Header(ref hdr) => {
                    builder = builder.cxx_bridge_include(hdr);
                }
                CppInclusion::Define(ref def) => {
                    warn!("Not currently able to inform cxx about #define {}", def);
                    // TODO consider enhancing cxx here
                }
            }
        }

        for inc_dir in inc_dirs {
            builder = builder.clang_arg(format!("-I{}", inc_dir.display()));
        }
        builder = builder.header_contents("example.hpp", &full_header);
        // 3. Passes allowlist and other options to the bindgen::Builder equivalent
        //    to --output-style=cxx --allowlist=<as passed in>
        for a in &self.allowlist {
            // TODO - allowlist type/functions/separately
            builder = builder.whitelist_type(a);
            builder = builder.whitelist_function(a);
        }
        Ok(builder)
    }

    pub fn generate_rs(self) -> Result<TokenStream2> {
        // 4. (also respects environment variables to pick up more headers,
        //     include paths and #defines)
        // Then:
        // 1. Builds an overall C++ header with all those #defines and #includes
        // 2. Passes it to bindgen::Builder::header
        let bindings = self
            .make_bindgen_builder()?
            .generate()
            .map_err(Error::Bindgen)?;
        let bindings = bindings.to_string();
        info!("Bindings: {}", bindings);
        let bindings = syn::parse_str::<ItemMod>(&bindings).map_err(Error::Parsing)?;
        let mut ts = TokenStream2::new();
        bindings.to_tokens(&mut ts);
        Ok(ts)
    }

    pub fn generate_h_and_cxx(self) -> Result<GeneratedCode> {
        let rs = self.generate_rs()?;
        let mut opt = cxx_gen::Opt::default();
        opt.omit_type_definitions = true;
        let results = cxx_gen::generate_header_and_cc(rs, opt).map_err(Error::CxxGen);
        if let Ok(ref gen) = results {
            info!("CXX: {}", String::from_utf8(gen.cxx.clone()).unwrap());
            info!("header: {}", String::from_utf8(gen.header.clone()).unwrap());
        }
        results
    }

    pub fn include_dirs(&self) -> Result<Vec<PathBuf>> {
        self.determine_incdirs()
    }
}

/// This outermost crate currently just contains integration tests
/// for all the other crates. That's a bit of an odd arrangement, and
/// maybe needs revisiting.
#[cfg(test)]
mod tests {

    use indoc::indoc;
    use log::info;
    use proc_macro2::TokenStream;
    use quote::quote;
    use std::fs::File;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use tempfile::{tempdir, TempDir};
    use test_env_log::test;

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
        test_cases: trybuild::TestCases,
    }

    impl LinkableTryBuilder {
        fn new() -> Self {
            LinkableTryBuilder {
                temp_dir: tempdir().unwrap(),
                test_cases: trybuild::TestCases::new(),
            }
        }

        fn copy_libraries_into_temp_dir<P1: AsRef<Path>>(
            &self,
            library_path: &P1,
            library_name: &str,
        ) {
            for item in std::fs::read_dir(library_path).unwrap() {
                let item = item.unwrap();
                if item
                    .file_name()
                    .into_string()
                    .unwrap()
                    .contains(library_name)
                {
                    let dest_lib = self.temp_dir.path().join(item.file_name());
                    std::fs::copy(item.path(), dest_lib).unwrap();
                }
            }
        }

        fn build<P1: AsRef<Path>, P2: AsRef<Path>>(
            &self,
            library_path: &P1,
            library_name: &str,
            rs_path: &P2,
        ) {
            // Copy all items from the source dir into our temporary dir if their name matches
            // the pattern given in `library_name`.
            self.copy_libraries_into_temp_dir(library_path, library_name);
            std::env::set_var(
                "RUSTFLAGS",
                format!("-L {}", self.temp_dir.path().to_str().unwrap()),
            );
            self.test_cases.pass(rs_path)
        }
    }

    fn write_to_file(tdir: &TempDir, filename: &str, content: &str) -> PathBuf {
        let path = tdir.path().join(filename);
        let mut f = File::create(&path).unwrap();
        info!("Writing to {:?}: {}", path, content);
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    fn run_test(cxx_code: &str, header_code: &str, rust_code: TokenStream, allowed_funcs: &[&str]) {
        // Step 1: Write the C++ header snippet to a temp file
        let tdir = tempdir().unwrap();
        write_to_file(&tdir, "input.h", header_code);
        // Step 2: Expand the snippet of Rust code into an entire
        //         program including include_cxx!
        // TODO - we're not quoting #s below (in the "" sense), and it's not entirely
        // clear how we're getting away with it, but quoting it doesn't work.
        let allowed_funcs = allowed_funcs.iter().map(|s| {
            quote! {
                Allow(#s)
            }
        });
        let expanded_rust = quote! {
            use autocxx_macro::include_cxx;

            include_cxx!(
                Header("input.h"),
                #(#allowed_funcs),*
            );

            fn main() {
                #rust_code
            }

            #[link(name="autocxx-demo")]
            extern {}
        };
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
        let mut b =
            autocxx_build::Builder::new(&rs_path, tdir.path().to_str().unwrap())
                .unwrap();
        b.builder()
            .file(cxx_path)
            .out_dir(&target_dir)
            .host(&target)
            .target(&target)
            .opt_level(1)
            .flag("-std=c++11")
            .include(tdir.path())
            .try_compile("autocxx-demo")
            .unwrap();
        // Step 8: use the trybuild crate to build the Rust file.
        BUILDER
            .lock()
            .unwrap()
            .build(&target_dir, "autocxx-demo", &rs_path);
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
            uint32_t take_bob(Bob a);
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
