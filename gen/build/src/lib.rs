
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

mod engine;

use std::path::Path;
use std::fs;
use syn::{Item, File};


pub enum Error {
    FileReadError(std::io::Error),
    Syntax(syn::Error),
    NoIdent,
}

pub fn bridge(rs_file: impl AsRef<Path>) -> Result<cc::Build,Error> {
    let source = fs::read_to_string(rs_file).map_err(|e| Error::FileReadError(e))?;
    let source = syn::parse_file(&source).map_err(|e| Error::Syntax(e))?;
    for item in source.items {
        if let Item::Macro(mac) = item {
            if let Some(name) = mac.ident {
                if name.to_string() == "include_cxx" {
                    let include_cpp = engine::IncludeCpp::fromSyn();
                    include_cpp.expand()
                }
            }
        }
    }

}