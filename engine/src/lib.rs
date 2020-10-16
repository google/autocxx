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

mod additional_cpp_generator;
mod bridge_converter;
mod byvalue_checker;
mod parse;
mod preprocessor_parse_callbacks;
mod rust_pretty_printer;
mod types;

#[cfg(test)]
mod integration_tests;

use proc_macro2::TokenStream as TokenStream2;
use std::path::PathBuf;

use indoc::indoc;
use quote::ToTokens;
use syn::parse::{Parse, ParseStream, Result as ParseResult};
use syn::{parse_quote, ItemMod, Macro};

use additional_cpp_generator::{AdditionalCpp, AdditionalCppGenerator};
use itertools::join;
use log::{debug, info, warn};
use preprocessor_parse_callbacks::{PreprocessorDefinitions, PreprocessorParseCallbacks};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Mutex;
use types::TypeName;

pub use parse::{parse_file, ParseError};

const BINDGEN_BLOCKLIST: &[&str] = &["std.*", "__gnu.*", ".*mbstate_t.*"];
pub struct CppFilePair {
    pub header: Vec<u8>,
    pub implementation: Vec<u8>,
    pub header_name: String,
}

pub struct GeneratedCpp(pub Vec<CppFilePair>);

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

fn dump_generated_code(gen: cxx_gen::GeneratedCode) -> Result<cxx_gen::GeneratedCode> {
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
        join(
            self.inclusions.iter().map(|incl| match incl {
                CppInclusion::Define(symbol) => format!("#define {}\n", symbol),
                CppInclusion::Header(path) => format!("#include \"{}\"\n", path),
            }),
            "",
        )
    }

    fn determine_incdirs(&self) -> Result<Vec<PathBuf>> {
        let inc_dirs = match &self.preconfigured_inc_dirs {
            Some(d) => d.clone(),
            None => std::env::var_os("AUTOCXX_INC").ok_or(Error::NoAutoCxxInc)?,
        };
        let inc_dirs = std::env::split_paths(&inc_dirs);
        // TODO consider if we can or should look up the include path automatically
        // instead of requiring callers always to set AUTOCXX_INC.

        // On Windows, the canonical path begins with a UNC prefix that cannot be passed to
        // the MSVC compiler, so dunce::canonicalize() is used instead of std::fs::canonicalize()
        // See:
        // * https://github.com/dtolnay/cxx/pull/41
        // * https://github.com/alexcrichton/cc-rs/issues/169
        inc_dirs
            .map(|p| dunce::canonicalize(&p).map_err(|_| Error::CouldNotCanoncalizeIncludeDir(p)))
            .collect()
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

    fn inject_header_into_bindgen(
        &self,
        mut builder: bindgen::Builder,
        additional_cpp: Option<AdditionalCpp>,
    ) -> bindgen::Builder {
        let full_header = self.build_header();
        let more_decls = if let Some(additional_cpp) = additional_cpp {
            for a in additional_cpp.extra_allowlist {
                builder = builder.whitelist_function(a);
            }
            format!(
                "#include <memory>\n\n// Extra autocxx insertions:\n\n{}\n\n",
                additional_cpp.declarations
            )
        } else {
            String::new()
        };
        let full_header = format!("{}{}\n\n{}", types::get_prelude(), more_decls, full_header,);
        info!("Full header: {}", full_header);
        builder = builder.header_contents("example.hpp", &full_header);
        builder
    }

    pub fn generate_rs(&self) -> Result<TokenStream2> {
        let results = self.do_generation()?;
        Ok(match results {
            Some((itemmod, _)) => itemmod.to_token_stream(),
            None => TokenStream2::new(),
        })
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
        let bindings = format!("mod bindgen {{ {} }}", bindings);
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

    fn do_generation(&self) -> Result<Option<(ItemMod, AdditionalCppGenerator)>> {
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
        let builder = self.make_bindgen_builder()?;
        let bindings = self
            .inject_header_into_bindgen(builder, None)
            .generate()
            .map_err(Error::Bindgen)?;
        let bindings = self.parse_bindings(bindings)?;

        let mut converter = bridge_converter::BridgeConverter::new(
            self.generate_include_list(),
            self.pod_types.clone(), // TODO take self by value to avoid clone.
        );

        let mut conversion = converter
            .convert(bindings, None, &HashMap::new())
            .map_err(Error::Conversion)?;
        let mut additional_cpp_generator = AdditionalCppGenerator::new(self.build_header());
        additional_cpp_generator.add_needs(conversion.additional_cpp_needs);
        let additional_cpp_items = additional_cpp_generator.generate();
        if let Some(additional_cpp_items) = additional_cpp_items {
            // When processing the bindings the first time, we discovered we wanted to add
            // more C++ (because you can never have too much C++.) Examples are field
            // accessor methods, or make_unique wrappers.
            // So, err, let's start all over again. Fun!
            let mut builder = self.make_bindgen_builder()?;
            info!(
                "Extra blocklist: {:?}",
                additional_cpp_items.extra_blocklist
            );
            for x in &additional_cpp_items.extra_blocklist {
                builder = builder.blacklist_item(x);
            }
            // TODO this clone is tedious
            let renames = additional_cpp_items.renames.clone();
            let bindings = self
                .inject_header_into_bindgen(builder, Some(additional_cpp_items))
                .generate()
                .map_err(Error::Bindgen)?;
            let bindings = self.parse_bindings(bindings)?;
            conversion = converter
                .convert(bindings, Some("autocxxgen.h"), &renames)
                .map_err(Error::Conversion)?;
        }

        let mut items = conversion.items;
        if let Some(itemmod) = self.get_preprocessor_defs_mod() {
            items.push(syn::Item::Mod(itemmod));
        }
        let mut new_bindings: ItemMod = parse_quote! {
            mod ffi {
            }
        };
        new_bindings.content.as_mut().unwrap().1.append(&mut items);
        info!(
            "New bindings:\n{}",
            rust_pretty_printer::pretty_print(&new_bindings.to_token_stream())
        );
        Ok(Some((new_bindings, additional_cpp_generator)))
    }

    /// Generate C++-side bindings for these APIs.
    pub fn generate_h_and_cxx(self) -> Result<GeneratedCpp> {
        let generation = self.do_generation()?;
        let mut files = Vec::new();
        match generation {
            None => {}
            Some((itemmod, additional_cpp_generator)) => {
                let rs = itemmod.into_token_stream();
                let opt = cxx_gen::Opt::default();
                let cxx_generated = cxx_gen::generate_header_and_cc(rs, &opt)
                    .map_err(Error::CxxGen)
                    .and_then(dump_generated_code)?;
                files.push(CppFilePair {
                    header: cxx_generated.header,
                    header_name: "cxxgen.h".to_string(),
                    implementation: cxx_generated.implementation,
                });

                match additional_cpp_generator.generate() {
                    None => {}
                    Some(additional_cpp) => {
                        // TODO should probably replace pragma once below with traditional include guards.
                        let declarations = format!("#pragma once\n{}", additional_cpp.declarations);
                        files.push(CppFilePair {
                            header: declarations.as_bytes().to_vec(),
                            header_name: "autocxxgen.h".to_string(),
                            implementation: additional_cpp.definitions.as_bytes().to_vec(),
                        });
                    }
                }
            }
        };
        Ok(GeneratedCpp(files))
    }

    /// Get the configured include directories.
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
