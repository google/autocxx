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
use syn::parse::{Parse, ParseStream, Result};

use syn::{ItemMod, ItemMacro};

use log::debug;

pub enum CppInclusion {
    Define(String),
    Header(String),
}

pub struct IncludeCpp {
    inclusions: Vec<CppInclusion>,
    allowlist: Vec<String>,
    inc_dir: PathBuf, // TODO make more versatile
}

impl Parse for IncludeCpp {
    fn parse(input: ParseStream) -> Result<Self> {
        input.parse::<syn::Token![<]>()?;
        let hdr = input.parse::<syn::Ident>()?;
        input.parse::<syn::Token![>]>()?;
        input.parse::<syn::Token![,]>()?;
        input.parse::<syn::Token![<]>()?;
        let allow = input.parse::<syn::Ident>()?;
        input.parse::<syn::Token![>]>()?;
        // TODO: Takes as inputs:
        // 1. List of headers to include
        // 2. List of #defines to include
        // 3. Allowlist
        // TODO don't do this include dir nonsense. Take it from
        // some external configuration.
        // TODO the syntax above is insane.
        let full_header_name = format!("{}.h", hdr.to_string());
        let sourcedir = hdr.span().unwrap().source_file().path().parent().unwrap().to_path_buf().canonicalize().unwrap();
        debug!("Including dir {:?}", sourcedir);
        println!("Allowlist {}", allow.to_string());
        Ok(IncludeCpp {
            inclusions: vec![CppInclusion::Header(full_header_name)],
            allowlist: vec![allow.to_string()],
            inc_dir: sourcedir,
        })
    }
}

impl IncludeCpp {
    #[cfg(test)]
    pub fn new(inclusions: Vec<CppInclusion>,
        allowlist: Vec<String>,
        inc_dir: PathBuf) -> Self {
        IncludeCpp {
            inclusions,
            allowlist,
            inc_dir
        }
    }

    pub fn fromSyn(mac: ItemMacro) -> Self {
        // TODO populate fields
        IncludeCpp {
            inclusions: vec![],
            allowlist: vec![],
            inc_dir: PathBuf::new(),
        }
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

    fn make_builder(&self) -> bindgen::Builder {
        let full_header = self.build_header();
        println!("Full header: {}", full_header);
        println!("Inc dir: {}", self.inc_dir.display());

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

    pub fn run(self) -> TokenStream2 {
        // TODO:
        // 4. (also respects environment variables to pick up more headers,
        //     include paths and #defines)
        // Then:
        // 1. Builds an overall C++ header with all those #defines and #includes
        // 2. Passes it to bindgen::Builder::header
        let bindings = self.make_builder().generate().unwrap().to_string();
        println!("Bindings: {}", bindings);
        let bindings = syn::parse_str::<ItemMod>(&bindings).unwrap();
        let mut ts = TokenStream2::new();
        bindings.to_tokens(&mut ts);
        ts
    }

    pub fn generate(self) -> std::result::Result<(Vec<u8>, Vec<u8>),String> {
        cxx_gen::generate_header_and_cc(self.run())
    }
}
