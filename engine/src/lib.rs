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

use log::debug;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Bindgen(()),
    CxxGen(cxx_gen::Error),
    Parsing(syn::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

pub enum CppInclusion {
    Define(String),
    Header(String),
}

/// Core of the autocxx engine.
/// TODO - consider merging this 'engine' sub-crate with the main crate.
/// TODO - consider whether this 'engine' crate should actually be a
/// directory of source symlinked from all the other sub-crates, so that
/// we avoid exposing an external interface from this code.
pub struct IncludeCpp {
    inclusions: Vec<CppInclusion>,
    allowlist: Vec<String>,
    inc_dir: PathBuf, // TODO make more versatile
}

impl Parse for IncludeCpp {
    fn parse(input: ParseStream) -> ParseResult<Self> {
        Self::new_from_parse_stream(input)
    }
}

impl IncludeCpp {
    /// Only used from test code, but as the test code is not currently
    /// in this binary, we can't use #cfg(test). TODO - fix by moving test code to here.
    pub fn new(inclusions: Vec<CppInclusion>, allowlist: Vec<String>, inc_dir: PathBuf) -> Self {
        IncludeCpp {
            inclusions,
            allowlist,
            inc_dir,
        }
    }

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

        // TODO AUTOCXX_INC handling should not panic, and
        // we probably want better behavior
        // Multiple include dirs will of course be necessary too.
        let sourcedir: PathBuf = std::env::var_os("AUTOCXX_INC").unwrap().into();
        let sourcedir = sourcedir.canonicalize().unwrap();
        debug!("Including dir {:?}", sourcedir);
        Ok(IncludeCpp {
            inclusions,
            allowlist,
            inc_dir: sourcedir,
        })
    }

    pub fn new_from_syn(mac: Macro) -> Result<Self> {
        mac.parse_body::<IncludeCpp>().map_err(Error::Parsing)
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

    fn make_bindgen_builder(&self) -> bindgen::Builder {
        let full_header = self.build_header();
        debug!("Full header: {}", full_header);
        debug!("Inc dir: {}", self.inc_dir.display());

        // TODO - pass headers in &self.inclusions into
        // bindgen such that it can include them in the generated
        // extern "C" section as include!
        // The .hpp below is important so bindgen works in C++ mode
        // TODO work with OsStrs here to avoid the .display()
        let mut builder = bindgen::builder()
            .clang_arg(format!("-I{}", self.inc_dir.display()))
            .header_contents("example.hpp", &full_header);
        // 3. Passes allowlist and other options to the bindgen::Builder equivalent
        //    to --output-style=cxx --allowlist=<as passed in>
        for a in &self.allowlist {
            // TODO - allowlist type/functions/separately
            builder = builder.whitelist_type(a);
            builder = builder.whitelist_function(a);
        }
        builder
    }

    pub fn generate_rs(self) -> Result<TokenStream2> {
        // TODO:
        // 4. (also respects environment variables to pick up more headers,
        //     include paths and #defines)
        // Then:
        // 1. Builds an overall C++ header with all those #defines and #includes
        // 2. Passes it to bindgen::Builder::header
        let bindings = self
            .make_bindgen_builder()
            .generate()
            .map_err(Error::Bindgen)?;
        // TODO see what that type is and whether we can avoid reparsing.
        let bindings = bindings.to_string();
        debug!("Bindings: {}", bindings);
        let bindings = syn::parse_str::<ItemMod>(&bindings).map_err(Error::Parsing)?;
        let mut ts = TokenStream2::new();
        bindings.to_tokens(&mut ts);
        Ok(ts)
    }

    pub fn generate_h_and_cxx(self) -> Result<GeneratedCode> {
        let rs = self.generate_rs()?;
        cxx_gen::generate_header_and_cc(rs).map_err(Error::CxxGen)
    }

    pub fn include_dir(&self) -> &PathBuf {
        &self.inc_dir
    }
}
