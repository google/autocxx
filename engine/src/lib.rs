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
mod byvalue_checker;
mod conversion;
mod function_wrapper;
mod known_types;
mod parse;
mod rust_pretty_printer;
mod type_database;
mod typedef_analyzer;
mod types;

#[cfg(any(test, feature = "build"))]
mod builder;

#[cfg(test)]
mod integration_tests;

use conversion::bridge_converter::BridgeConverter;
use proc_macro2::TokenStream as TokenStream2;
use std::{fmt::Display, path::PathBuf};
use type_database::TypeDatabase;

use quote::ToTokens;
use syn::parse::{Parse, ParseStream, Result as ParseResult};
use syn::{parse_quote, ItemMod, Macro};

use additional_cpp_generator::AdditionalCppGenerator;
use itertools::join;
use log::{info, warn};
use types::TypeName;

#[cfg(any(test, feature = "build"))]
pub use builder::{build, expect_build, BuilderError, BuilderResult, BuilderSuccess};
pub use parse::{parse_file, parse_token_stream, ParseError, ParsedFile};

pub use cxx_gen::HEADER;

/// Re-export cxx such that clients can use the same version as
/// us. This doesn't enable clients to avoid depending on the cxx
/// crate too, unfortunately, since generated cxx::bridge code
/// refers explicitly to ::cxx. See
/// https://github.com/google/autocxx/issues/36
pub use cxx;

pub struct CppFilePair {
    pub header: Vec<u8>,
    pub implementation: Vec<u8>,
    pub header_name: String,
}

pub struct GeneratedCpp(pub Vec<CppFilePair>);

/// Errors which may occur in generating bindings for these C++
/// functions.
#[derive(Debug)]
pub enum Error {
    /// Any error reported by bindgen, generating the C++ bindings.
    /// Any C++ parsing errors, etc. would be reported this way.
    Bindgen(()),
    /// Any problem parsing the Rust file.
    Parsing(syn::Error),
    /// No `include_cpp!` macro could be found.
    NoAutoCxxInc,
    /// The include directories specified were incorreect.
    CouldNotCanoncalizeIncludeDir(PathBuf),
    /// Some error occcurred in converting the bindgen-style
    /// bindings to safe cxx bindings.
    Conversion(conversion::bridge_converter::ConvertError),
    /// No 'generate' or 'generate_pod' was specified.
    /// It might be that in future we can simply let things work
    /// without any allowlist, in which case bindgen should generate
    /// bindings for everything. That just seems very unlikely to work
    /// in the common case right now.
    NoGenerationRequested,
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Bindgen(_) => write!(f, "Bindgen was unable to generate the initial .rs bindings for this file. This may indicate a parsing problem with the C++ headers.")?,
            Error::Parsing(err) => write!(f, "The Rust file could not be parsede: {}", err)?,
            Error::NoAutoCxxInc => write!(f, "No C++ include directory was provided. Consider setting AUTOCXX_INC.")?,
            Error::CouldNotCanoncalizeIncludeDir(pb) => write!(f, "One of the C++ include directories provided ({}) did not appear to exist or could otherwise not be made into a canonical path.", pb.to_string_lossy())?,
            Error::Conversion(err) => write!(f, "autocxx could not generate the requested bindings. {}", err)?,
            Error::NoGenerationRequested => write!(f, "No 'generate' or 'generate_pod' directives were found, so we would not generate any Rust bindings despite the inclusion of C++ headers.")?,
        }
        Ok(())
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

pub enum CppInclusion {
    Define(String),
    Header(String),
}

#[allow(clippy::large_enum_variant)] // because this is only used once
enum State {
    NotGenerated,
    ParseOnly,
    NothingGenerated,
    Generated(ItemMod, AdditionalCppGenerator),
}

/// Core of the autocxx engine. See `generate` for most details
/// on how this works.
///
/// TODO - consider whether this 'engine' crate should actually be a
/// directory of source symlinked from all the other sub-crates, so that
/// we avoid exposing an external interface from this code.
pub struct IncludeCpp {
    inclusions: Vec<CppInclusion>,
    type_database: TypeDatabase,
    preconfigured_inc_dirs: Option<std::ffi::OsString>,
    exclude_utilities: bool,
    state: State,
}

impl Parse for IncludeCpp {
    fn parse(input: ParseStream) -> ParseResult<Self> {
        Self::new_from_parse_stream(input)
    }
}

impl IncludeCpp {
    fn new_from_parse_stream(input: ParseStream) -> syn::Result<Self> {
        // Takes as inputs:
        // 1. List of headers to include
        // 2. List of #defines to include
        // 3. Allowlist

        let mut inclusions = Vec::new();
        let mut parse_only = false;
        let mut exclude_utilities = false;
        let mut type_database = TypeDatabase::new();

        while !input.is_empty() {
            if input.parse::<Option<syn::Token![#]>>()?.is_some() {
                let ident: syn::Ident = input.parse()?;
                if ident != "include" {
                    return Err(syn::Error::new(ident.span(), "expected include"));
                }
                let hdr: syn::LitStr = input.parse()?;
                inclusions.push(CppInclusion::Header(hdr.value()));
            } else {
                let ident: syn::Ident = input.parse()?;
                input.parse::<Option<syn::Token![!]>>()?;
                if ident == "generate" || ident == "generate_pod" {
                    let args;
                    syn::parenthesized!(args in input);
                    let generate: syn::LitStr = args.parse()?;
                    type_database.add_to_allowlist(generate.value());
                    if ident == "generate_pod" {
                        type_database
                            .note_pod_request(TypeName::new_from_user_input(&generate.value()));
                    }
                } else if ident == "nested_type" {
                    let args;
                    syn::parenthesized!(args in input);
                    let nested: syn::LitStr = args.parse()?;
                    args.parse::<syn::Token![,]>()?;
                    let nested_in: syn::LitStr = args.parse()?;
                    type_database.note_nested_type(
                        TypeName::new_from_user_input(&nested.value()),
                        TypeName::new_from_user_input(&nested_in.value()),
                    );
                } else if ident == "block" {
                    let args;
                    syn::parenthesized!(args in input);
                    let generate: syn::LitStr = args.parse()?;
                    type_database.add_to_blocklist(generate.value());
                } else if ident == "parse_only" {
                    parse_only = true;
                } else if ident == "exclude_utilities" {
                    exclude_utilities = true;
                } else {
                    return Err(syn::Error::new(
                        ident.span(),
                        "expected generate, generate_pod, nested_type or exclude_utilities",
                    ));
                }
            }
            if input.is_empty() {
                break;
            }
        }
        if !exclude_utilities {
            type_database.add_to_allowlist("make_string".to_string());
        }

        Ok(IncludeCpp {
            inclusions,
            preconfigured_inc_dirs: None,
            exclude_utilities,
            type_database,
            state: if parse_only {
                State::ParseOnly
            } else {
                State::NotGenerated
            },
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
        // TODO support different C++ versions
        let mut builder = bindgen::builder()
            .clang_args(&["-x", "c++", "-std=c++14"])
            .derive_copy(false)
            .derive_debug(false)
            .default_enum_style(bindgen::EnumVariation::Rust {
                non_exhaustive: false,
            })
            .enable_cxx_namespaces()
            .disable_nested_struct_naming()
            .generate_inline_functions(true)
            .layout_tests(false); // TODO revisit later
        for item in known_types::get_initial_blocklist() {
            builder = builder.blacklist_item(item);
        }

        for inc_dir in self.determine_incdirs()? {
            // TODO work with OsStrs here to avoid the .display()
            builder = builder.clang_arg(format!("-I{}", inc_dir.display()));
        }

        // 3. Passes allowlist and other options to the bindgen::Builder equivalent
        //    to --output-style=cxx --allowlist=<as passed in>
        for a in self.type_database.allowlist() {
            // TODO - allowlist type/functions/separately
            builder = builder
                .whitelist_type(a)
                .whitelist_function(a)
                .whitelist_var(a);
        }

        Ok(builder)
    }

    fn inject_header_into_bindgen(&self, mut builder: bindgen::Builder) -> bindgen::Builder {
        let full_header = self.build_header();
        let full_header = format!("{}\n\n{}", known_types::get_prelude(), full_header,);
        builder = builder.header_contents("example.hpp", &full_header);
        builder
    }

    /// Generate the Rust bindings. Call `generate` first.
    pub fn generate_rs(&self) -> TokenStream2 {
        match &self.state {
            State::NotGenerated => panic!("Call generate() first"),
            State::Generated(itemmod, _) => itemmod.to_token_stream(),
            State::NothingGenerated | State::ParseOnly => TokenStream2::new(),
        }
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

    /// Actually examine the headers to find out what needs generating.
    /// Most errors occur at this stage as we fail to interpret the C++
    /// headers properly.
    ///
    /// The basic idea is this. We will run `bindgen` which will spit
    /// out a ton of Rust code corresponding to all the types and functions
    /// defined in C++. We'll then post-process that bindgen output
    /// into a form suitable for ingestion by `cxx`.
    /// (It's the `bridge_converter` mod which does that.)
    /// Along the way, the `bridge_converter` might tell us of additional
    /// C++ code which we should generate, e.g. wrappers to move things
    /// into and out of `UniquePtr`s.
    pub fn generate(&mut self) -> Result<()> {
        // If we are in parse only mode, do nothing. This is used for
        // doc tests to ensure the parsing is valid, but we can't expect
        // valid C++ header files or linkers to allow a complete build.
        match self.state {
            State::ParseOnly => return Ok(()),
            State::NotGenerated => {}
            State::Generated(_, _) | State::NothingGenerated => panic!("Only call generate once"),
        }

        if self.type_database.allowlist_is_empty() {
            return Err(Error::NoGenerationRequested);
        }

        let builder = self.make_bindgen_builder()?;
        let bindings = self
            .inject_header_into_bindgen(builder)
            .generate()
            .map_err(Error::Bindgen)?;
        let bindings = self.parse_bindings(bindings)?;

        let include_list = self.generate_include_list();
        let mut converter = BridgeConverter::new(&include_list, &self.type_database);

        let conversion = converter
            .convert(bindings, self.exclude_utilities)
            .map_err(Error::Conversion)?;
        let mut additional_cpp_generator = AdditionalCppGenerator::new(self.build_header());
        additional_cpp_generator.add_needs(conversion.additional_cpp_needs, &self.type_database);
        let mut items = conversion.items;
        let mut new_bindings: ItemMod = parse_quote! {
            #[allow(non_snake_case)]
            #[allow(dead_code)]
            #[allow(non_upper_case_globals)]
            #[allow(non_camel_case_types)]
            mod ffi {
            }
        };
        new_bindings.content.as_mut().unwrap().1.append(&mut items);
        info!(
            "New bindings:\n{}",
            rust_pretty_printer::pretty_print(&new_bindings.to_token_stream())
        );
        self.state = State::Generated(new_bindings, additional_cpp_generator);
        Ok(())
    }

    /// Generate C++-side bindings for these APIs. Call `generate` first.
    pub fn generate_h_and_cxx(&self) -> Result<GeneratedCpp, cxx_gen::Error> {
        let mut files = Vec::new();
        match &self.state {
            State::ParseOnly => panic!("Cannot generate C++ in parse-only mode"),
            State::NotGenerated => panic!("Call generate() first"),
            State::NothingGenerated => {}
            State::Generated(itemmod, additional_cpp_generator) => {
                let rs = itemmod.into_token_stream();
                let opt = cxx_gen::Opt::default();
                let cxx_generated = cxx_gen::generate_header_and_cc(rs, &opt)?;
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
                        info!("Additional C++ decls:\n{}", declarations);
                        info!("Additional C++ defs:\n{}", additional_cpp.definitions);
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
