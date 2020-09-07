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


mod bridge_converter;
mod preprocessor_parse_callbacks;

#[cfg(test)]
mod integration_tests;

use proc_macro2::TokenStream as TokenStream2;
use std::path::PathBuf;

use cxx_gen::GeneratedCode;
use indoc::indoc;
use quote::ToTokens;
use syn::parse::{Parse, ParseStream, Result as ParseResult};
use syn::{ItemMod, Macro};

use log::{debug, info, warn};
use osstrtools::OsStrTools;
use preprocessor_parse_callbacks::{PreprocessorDefinitions, PreprocessorParseCallbacks};
use std::sync::Mutex;
use std::rc::Rc;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Bindgen(()),
    CxxGen(cxx_gen::Error),
    Parsing(syn::Error),
    NoAutoCxxInc,
    CouldNotCanoncalizeIncludeDir(PathBuf),
    Conversion(bridge_converter::ConvertError),
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
    parse_only: bool,
    preprocessor_definiitions: Rc<Mutex<PreprocessorDefinitions>>,
}

impl Parse for IncludeCpp {
    fn parse(input: ParseStream) -> ParseResult<Self> {
        Self::new_from_parse_stream(input)
    }
}

/// Prelude of C++ for squirting into bindgen. This configures
/// bindgen to output simpler types to replace some STL types
/// that bindgen just can't cope with. Although we then replace
/// those types with cxx types (e.g. UniquePtr), this intermediate
/// step is still necessary because bindgen can't otherwise
/// give us the templated types (e.g. when faced with the STL
/// unique_ptr, bindgen would normally give us std_unique_ptr
/// as opposed to std_unique_ptr<T>.)
static PRELUDE: &str = indoc! {"
    /**
    * <div rustbindgen=\"true\" replaces=\"std::unique_ptr\">
    */
    template<typename T> class UniquePtr {
        T* ptr;
    };

    /**
    * <div rustbindgen=\"true\" replaces=\"std::string\">
    */
    class CxxString {
        char* str_data;
    };
    \n"};

impl IncludeCpp {
    fn new_from_parse_stream(input: ParseStream) -> syn::Result<Self> {
        // Takes as inputs:
        // 1. List of headers to include
        // 2. List of #defines to include
        // 3. Allowlist

        let mut inclusions = Vec::new();
        let mut allowlist = Vec::new();
        let mut parse_only = false;

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
            } else if ident == "ParseOnly" {
                parse_only = true;
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
            parse_only,
            preprocessor_definiitions: Rc::new(Mutex::new(PreprocessorDefinitions::new())),
        })
    }

    pub fn new_from_syn(mac: Macro) -> Result<Self> {
        mac.parse_body::<IncludeCpp>().map_err(Error::Parsing)
    }

    pub fn set_include_dirs<P: AsRef<std::ffi::OsStr>>(&mut self, include_dirs: P) {
        self.preconfigured_inc_dirs = Some(include_dirs.as_ref().into());
    }

    fn build_header(&self) -> String {
        let mut s = PRELUDE.to_string();
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
        // TODO get rid of this huge blocklist. It exists because
        // even if we replace a given STL type (per the PRELUDE, above)
        // bindgen still recurses into all the other definitions it needs.
        // It's probably desirable to fix this in bindgen, though I expect
        // the current behavior is there for a reason. Meanwhile, make
        // this list less hard-coded and nasty as best we can - TODO.
        let mut builder = bindgen::builder()
            .clang_args(&["-x", "c++", "-std=c++14"])
            .blacklist_item(".*default.*")
            .blacklist_item(".*unique_ptr.*")
            .blacklist_item(".*string.*")
            .blacklist_item(".*std_.*")
            .blacklist_item("std_.*")
            .blacklist_item("std.*")
            .blacklist_item(".*compressed_pair.*")
            .blacklist_item(".*allocator.*")
            .blacklist_item(".*wrap_iter.*")
            .blacklist_item(".*reverse_iterator.*")
            .blacklist_item(".*propagate_on_container.*")
            .blacklist_item(".*char_traits.*")
            .blacklist_item(".*size_t.*")
            .blacklist_item(".*mbstate_t.*")
            .derive_copy(false)
            .derive_debug(false)
            .parse_callbacks(Box::new(PreprocessorParseCallbacks::new(self.preprocessor_definiitions.clone())))
            .default_enum_style(bindgen::EnumVariation::Rust {
                non_exhaustive: false,
            })
            .layout_tests(false); // TODO revisit later

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
        let another_ref = self.preprocessor_definiitions.clone();
        let ppdefs = another_ref.try_lock().unwrap().to_tokenstream();
        
        let mut ts = self.do_generation(true)?;
        ts.extend(ppdefs);
        Ok(ts)
    }

    fn do_generation(self, old_rust: bool) -> Result<TokenStream2> {
        // If we are in parse only mode, do nothing. This is used for
        // doc tests to ensure the parsing is valid, but we can't expect
        // valid C++ header files or linkers to allow a complete build.
        if self.parse_only {
            return Ok(TokenStream2::new());
        }
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
        // Manually add the mod ffi {} so that we can ask syn to parse
        // into a single construct.
        let bindings = format!("#[cxx::bridge] mod ffi {{ {} }}", bindings);
        info!("Bindings: {}", bindings);
        let bindings = syn::parse_str::<ItemMod>(&bindings).map_err(Error::Parsing)?;

        let mut include_list = Vec::new();
        for incl in &self.inclusions {
            match incl {
                CppInclusion::Header(ref hdr) => {
                    include_list.push(hdr.clone());
                }
                CppInclusion::Define(_) => warn!("Currently no way to define! within cxx"),
            }
        }

        let mut converter = bridge_converter::BridgeConverter::new(include_list, old_rust);
        let new_bindings = converter.convert(bindings).map_err(Error::Conversion)?;
        let new_bindings = new_bindings.to_token_stream();
        info!("New bindings: {}", new_bindings.to_string());
        Ok(new_bindings)
    }

    pub fn generate_h_and_cxx(self) -> Result<GeneratedCode> {
        let rs = self.do_generation(false)?;
        let opt = cxx_gen::Opt::default();
        let results = cxx_gen::generate_header_and_cc(rs, &opt).map_err(Error::CxxGen);
        if let Ok(ref gen) = results {
            info!(
                "CXX: {}",
                String::from_utf8(gen.implementation.clone()).unwrap()
            );
            info!("header: {}", String::from_utf8(gen.header.clone()).unwrap());
        }
        results
    }

    pub fn include_dirs(&self) -> Result<Vec<PathBuf>> {
        self.determine_incdirs()
    }
}
