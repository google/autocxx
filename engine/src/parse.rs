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

use crate::Error as EngineError;
use crate::IncludeCpp;
use std::io::Read;
use std::path::Path;
use syn::Item;

/// Errors which may occur when parsing a Rust source file to discover
/// and interpret include_cxx macros.
#[derive(Debug)]
pub enum ParseError {
    /// Unable to open the source file
    FileOpen(std::io::Error),
    /// The .rs file couldn't be read.
    FileRead(std::io::Error),
    /// The .rs file couldn't be parsed.
    Syntax(syn::Error),
    /// The include CPP macro could not be parsed.
    MacroParseFail(EngineError),
}

pub fn parse_file<P1: AsRef<Path>>(
    rs_file: P1,
    autocxx_inc: Option<&str>,
) -> Result<Vec<IncludeCpp>, ParseError> {
    let mut source = String::new();
    let mut file = std::fs::File::open(rs_file).map_err(ParseError::FileOpen)?;
    file.read_to_string(&mut source)
        .map_err(ParseError::FileRead)?;
    let source = syn::parse_file(&source).map_err(ParseError::Syntax)?;
    let mut results = Vec::new();
    for item in source.items {
        if let Item::Macro(mac) = item {
            if mac.mac.path.is_ident("include_cxx") {
                let mut include_cpp =
                    crate::IncludeCpp::new_from_syn(mac.mac).map_err(ParseError::MacroParseFail)?;
                if let Some(autocxx_inc) = autocxx_inc {
                    include_cpp.set_include_dirs(autocxx_inc);
                }
                results.push(include_cpp);
            }
        }
    }
    Ok(results)
}
