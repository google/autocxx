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
mod byvalue_checker;
mod cpp_postprocessor;
mod preprocessor_parse_callbacks;

#[cfg(test)]
mod integration_tests;

use proc_macro2::Span;
use proc_macro2::TokenStream as TokenStream2;
use std::{fmt::Display, path::PathBuf};

use cxx_gen::GeneratedCode;
use indoc::indoc;
use quote::ToTokens;
use syn::parse::{Parse, ParseStream, Result as ParseResult};
use syn::{parse_quote, Ident, ItemMod, Macro, TypePath};

use cpp_postprocessor::CppPostprocessor;
use log::{debug, info, warn};
use osstrtools::OsStrTools;
use preprocessor_parse_callbacks::{PreprocessorDefinitions, PreprocessorParseCallbacks};
use std::rc::Rc;
use std::sync::Mutex;

/// Turn off and remove this constant when
/// https://github.com/rust-lang/rust/pull/75857 is fixed.
/// This should allow removal of the entire cpp_postprocessor module.
const TEMPORARY_HACK_TO_AVOID_REDEFINITIONS: bool = true;

const BINDGEN_BLOCKLIST: &[&str] = &["std.*", ".*mbstate_t.*"];

/// Any time we store a type name, we should use this.
/// At the moment it's just a string, but one day it will need to become
/// sufficiently intelligent to handle namespaces.
#[derive(Debug, PartialEq, PartialOrd, Eq, Hash, Clone)]
pub struct TypeName(String);

impl TypeName {
    fn from_ident(id: &Ident) -> Self {
        TypeName(id.to_string())
    }

    fn from_type_path(p: &TypePath) -> Self {
        // TODO better handle generics, multi-segment paths, etc.
        TypeName::from_ident(&p.path.segments.last().unwrap().ident)
    }

    fn new(id: &str) -> Self {
        TypeName(id.to_string())
    }

    fn to_ident(&self) -> Ident {
        Ident::new(&self.0, Span::call_site())
    }
}

impl Display for TypeName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

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
    allowlist: Vec<String>, // not TypeName as it may be functions or whatever.
    pod_types: Vec<TypeName>,
    preconfigured_inc_dirs: Option<std::ffi::OsString>,
    parse_only: bool,
    preprocessor_definitions: Rc<Mutex<PreprocessorDefinitions>>,
}

impl Parse for IncludeCpp {
    fn parse(input: ParseStream) -> ParseResult<Self> {
        Self::new_from_parse_stream(input)
    }
}

fn dump_generated_code(gen: GeneratedCode) -> Result<GeneratedCode> {
    info!(
        "CXX:\n{}",
        String::from_utf8(gen.implementation.clone()).unwrap()
    );
    info!(
        "header:\n{}",
        String::from_utf8(gen.header.clone()).unwrap()
    );
    Ok(gen)
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
        let mut pod_types = Vec::new();
        let mut parse_only = false;

        while !input.is_empty() {
            let ident: syn::Ident = input.parse()?;
            if ident == "Header" {
                let args;
                syn::parenthesized!(args in input);
                let hdr: syn::LitStr = args.parse()?;
                inclusions.push(CppInclusion::Header(hdr.value()));
            } else if ident == "Allow" || ident == "AllowPOD" {
                let args;
                syn::parenthesized!(args in input);
                let allow: syn::LitStr = args.parse()?;
                allowlist.push(allow.value());
                if ident == "AllowPOD" {
                    pod_types.push(TypeName::new(&allow.value()));
                }
            } else if ident == "ParseOnly" {
                parse_only = true;
            } else {
                return Err(syn::Error::new(
                    ident.span(),
                    "expected Header, Allow or AllowPOD",
                ));
            }
            if input.is_empty() {
                break;
            }
            input.parse::<syn::Token![,]>()?;
        }

        Ok(IncludeCpp {
            inclusions,
            allowlist,
            pod_types,
            preconfigured_inc_dirs: None,
            parse_only,
            preprocessor_definitions: Rc::new(Mutex::new(PreprocessorDefinitions::new())),
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

        debug!("Inc dir: {:?}", inc_dirs);

        // TODO support different C++ versions
        let mut builder = bindgen::builder()
            .clang_args(&["-x", "c++", "-std=c++14"])
            .derive_copy(false)
            .derive_debug(false)
            .parse_callbacks(Box::new(PreprocessorParseCallbacks::new(
                self.preprocessor_definitions.clone(),
            )))
            .default_enum_style(bindgen::EnumVariation::Rust {
                non_exhaustive: false,
            })
            .layout_tests(false); // TODO revisit later
        for item in BINDGEN_BLOCKLIST.iter() {
            builder = builder.blacklist_item(*item);
        }

        for inc_dir in inc_dirs {
            // TODO work with OsStrs here to avoid the .display()
            builder = builder.clang_arg(format!("-I{}", inc_dir.display()));
        }

        // 3. Passes allowlist and other options to the bindgen::Builder equivalent
        //    to --output-style=cxx --allowlist=<as passed in>
        for a in &self.allowlist {
            // TODO - allowlist type/functions/separately
            builder = builder
                .whitelist_type(a)
                .whitelist_function(a)
                .whitelist_var(a);
        }

        Ok(builder)
    }

    fn inject_original_header_into_bindgen(&self, mut builder: bindgen::Builder) -> bindgen::Builder {
        let full_header = self.build_header();
        debug!("Full header: {}", full_header);
        builder.header_contents("example.hpp", &full_header)
    }

    fn inject_enhanced_header_into_bindgen(&self, mut builder: bindgen::Builder, more_decls: String, more_allowlist: Vec<String>) -> bindgen::Builder {
        let full_header = self.build_header();
        let full_header = format!("{}\n\n// Extra autocxx insertions:\n\n{}", full_header, more_decls);
        debug!("Full (enhanced) header: {}", full_header);
        builder = builder.header_contents("example.hpp", &full_header);
        for a in more_allowlist {
            builder = builder
                .whitelist_function(a);
        }
        builder
    }

    pub fn generate_rs(&self) -> Result<TokenStream2> {
        let itemmod = self.do_generation(&mut CppPostprocessor::new())?;
        Ok(itemmod.to_token_stream())
    }

    fn get_preprocessor_defs_mod(&self) -> Option<ItemMod> {
        let another_ref = self.preprocessor_definitions.clone();
        let m = another_ref.try_lock().unwrap().to_mod();
        m
    }

    fn parse_bindings(&self, bindings: bindgen::Bindings) -> Result<ItemMod> {
        // This bindings object is actually a TokenStream internally and we're wasting
        // effort converting to and from string. We could enhance the bindgen API
        // in future.
        let bindings = bindings.to_string();
        // Manually add the mod ffi {} so that we can ask syn to parse
        // into a single construct.
        let bindings = format!("mod ffi {{ {} }}", bindings);
        info!("Bindings: {}", bindings);
        syn::parse_str::<ItemMod>(&bindings).map_err(Error::Parsing)
    }

    fn generate_include_list(&self) -> Vec<String> {
        let mut include_list = Vec::new();
        for incl in &self.inclusions {
            match incl {
                CppInclusion::Header(ref hdr) => {
                    include_list.push(hdr.clone());
                }
                CppInclusion::Define(_) => warn!("Currently no way to define! within cxx"),
            }
        }
        include_list
    }

    fn do_generation(
        &self,
        cpp_postprocessor: &mut CppPostprocessor,
    ) -> Result<Option<ItemMod>> {
        // If we are in parse only mode, do nothing. This is used for
        // doc tests to ensure the parsing is valid, but we can't expect
        // valid C++ header files or linkers to allow a complete build.
        if self.parse_only {
            return Ok(None);
        }

        // 4. (also respects environment variables to pick up more headers,
        //     include paths and #defines)
        // Then:
        // 1. Builds an overall C++ header with all those #defines and #includes
        // 2. Passes it to bindgen::Builder::header
        let builder = self
            .make_bindgen_builder()?;
        let bindings = self.inject_original_header_into_bindgen(builder)
            .generate()
            .map_err(Error::Bindgen)?;
        let bindings = self.parse_bindings(bindings)?;

        let mut converter = bridge_converter::BridgeConverter::new(
            self.generate_include_list(),
            self.pod_types.clone(), // TODO take self by value to avoid clone.
            cpp_postprocessor,
            TEMPORARY_HACK_TO_AVOID_REDEFINITIONS,
        );
        
        let mut items = converter.convert(bindings).map_err(Error::Conversion)?;
        let additional_cpp_items = cpp_postprocessor.additional_items_generated();
        if let Some((additional_declarations, additional_definitions, extra_allowlist)) = additional_cpp_items {
            // When processing the bindings the first time, we discovered we wanted to add
            // more C++ (because you can never have too much C++.) Examples are field
            // accessor methods, or make_unique wrappers.
            // So, err, let's start all over again. Fun!
            let builder = self
                .make_bindgen_builder()?;
            let bindings = self.inject_enhanced_header_into_bindgen(builder, additional_declarations, extra_allowlist)
                .generate()
                .map_err(Error::Bindgen)?;
            let bindings = self.parse_bindings(bindings)?;

            let mut converter = bridge_converter::BridgeConverter::new(
                self.generate_include_list(),
                self.pod_types.clone(), // TODO take self by value to avoid clone.
                cpp_postprocessor,
                TEMPORARY_HACK_TO_AVOID_REDEFINITIONS,
            );
            
            items = converter.convert(bindings).map_err(Error::Conversion)?;
        }

        if let Some(itemmod) = self.get_preprocessor_defs_mod() {
            items.push(syn::Item::Mod(itemmod));
        }
        let new_bindings: ItemMod = parse_quote! {
            mod ffi {
                #(#items)*
            }
        };
        info!(
            "New bindings: {}",
            new_bindings.to_token_stream().to_string()
        );
        Ok(Some(new_bindings))
    }

    pub fn generate_h_and_cxx(self) -> Result<GeneratedCode> {
        let mut cpp_postprocessor = CppPostprocessor::new();
        let rs = self.do_generation(&mut cpp_postprocessor)?;
        let rs = match rs {
            Some(itemmod) => itemmod.into_token_stream(),
            None => TokenStream2::new(),
        };
        let opt = cxx_gen::Opt::default();
        cxx_gen::generate_header_and_cc(rs, &opt)
            .map_err(Error::CxxGen)
            .and_then(dump_generated_code)
            .and_then(|gen| {
                Ok(GeneratedCode {
                    header: cpp_postprocessor.post_process(gen.header, false),
                    implementation: cpp_postprocessor.post_process(gen.implementation, true),
                })
            })
            .and_then(dump_generated_code)
    }

    pub fn include_dirs(&self) -> Result<Vec<PathBuf>> {
        self.determine_incdirs()
    }
}

#[cfg(test)]
mod tests {
    use crate::TypeName;

    #[test]
    fn test_typename() {
        let s = proc_macro2::Span::call_site();
        let id = syn::Ident::new("Bob", s);
        let tn = TypeName::from_ident(&id);
        assert_eq!(tn.to_ident(), id);
    }
}
