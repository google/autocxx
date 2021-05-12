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

use crate::{
    cxxbridge::CxxBridge, Error as EngineError, GeneratedCpp, IncludeCppEngine,
    RebuildDependencyRecorder,
};
use proc_macro2::TokenStream;
use quote::ToTokens;
use std::{collections::HashSet, fmt::Display, io::Read, path::PathBuf};
use std::{panic::UnwindSafe, path::Path, rc::Rc};
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
    /// The include CPP macro could not be expanded into
    /// Rust bindings to C++, because of some problem during the conversion
    /// process. This could be anything from a C++ parsing error to some
    /// C++ feature that autocxx can't yet handle and isn't able to skip
    /// over. It could also cover errors in your syntax of the `include_cpp`
    /// macro or the directives inside.
    AutocxxCodegenError(EngineError),
    /// There are two or more [autocxx::include_cpp] macros with the same
    /// mod name.
    ConflictingModNames,
}

impl Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::FileOpen(err) => write!(f, "Unable to open file: {}", err)?,
            ParseError::FileRead(err) => write!(f, "Unable to read file: {}", err)?,
            ParseError::Syntax(err) => write!(f, "Syntax error parsing Rust file: {}", err)?,
            ParseError::AutocxxCodegenError(err) =>
                write!(f, "Unable to parse include_cpp! macro: {}", err)?,
            ParseError::ConflictingModNames =>
                write!(f, "There are two or more include_cpp! macros with the same output mod name. Use name!")?,
        }
        Ok(())
    }
}

/// Parse a Rust file, and spot any include_cpp macros within it.
pub fn parse_file<P1: AsRef<Path>>(rs_file: P1) -> Result<ParsedFile, ParseError> {
    let mut source = String::new();
    let mut file = std::fs::File::open(rs_file).map_err(ParseError::FileOpen)?;
    file.read_to_string(&mut source)
        .map_err(ParseError::FileRead)?;
    proc_macro2::fallback::force();
    let source = syn::parse_file(&source).map_err(ParseError::Syntax)?;
    parse_file_contents(source)
}

fn parse_file_contents(source: syn::File) -> Result<ParsedFile, ParseError> {
    let mut results = Vec::new();
    for item in source.items {
        results.push(match item {
            Item::Macro(mac)
                if mac
                    .mac
                    .path
                    .segments
                    .last()
                    .map(|s| s.ident == "include_cpp")
                    .unwrap_or(false) =>
            {
                Segment::Autocxx(
                    crate::IncludeCppEngine::new_from_syn(mac.mac.clone())
                        .map_err(ParseError::AutocxxCodegenError)?,
                )
            }
            Item::Mod(itm)
                if itm
                    .attrs
                    .iter()
                    .any(|attr| attr.path.to_token_stream().to_string() == "cxx :: bridge") =>
            {
                Segment::Cxx(CxxBridge::from(itm))
            }
            _ => Segment::Other(item),
        });
    }
    Ok(ParsedFile(results))
}

/// A Rust file parsed by autocxx. May contain zero or more autocxx 'engines',
/// i.e. the `IncludeCpp` class, corresponding to zero or more include_cpp
/// macros within this file. Also contains `syn::Item` structures for all
/// the rest of the Rust code, such that it can be reconstituted if necessary.
pub struct ParsedFile(Vec<Segment>);

#[allow(clippy::large_enum_variant)]
enum Segment {
    Autocxx(IncludeCppEngine),
    Cxx(CxxBridge),
    Other(Item),
}

pub trait CppBuildable {
    fn generate_h_and_cxx(&self) -> Result<GeneratedCpp, cxx_gen::Error>;
}

impl ParsedFile {
    /// Get all the autocxxes in this parsed file.
    pub fn get_rs_buildables(&self) -> impl Iterator<Item = &IncludeCppEngine> {
        self.0.iter().filter_map(|s| match s {
            Segment::Autocxx(includecpp) => Some(includecpp),
            _ => None,
        })
    }

    /// Get all items which can result in C++ code
    pub fn get_cpp_buildables(&self) -> impl Iterator<Item = &dyn CppBuildable> {
        self.0.iter().filter_map(|s| match s {
            Segment::Autocxx(includecpp) => Some(includecpp as &dyn CppBuildable),
            Segment::Cxx(cxxbridge) => Some(cxxbridge as &dyn CppBuildable),
            _ => None,
        })
    }

    fn get_autocxxes_mut(&mut self) -> impl Iterator<Item = &mut IncludeCppEngine> {
        self.0.iter_mut().filter_map(|s| match s {
            Segment::Autocxx(includecpp) => Some(includecpp),
            _ => None,
        })
    }

    pub fn include_dirs(&self) -> impl Iterator<Item = &PathBuf> {
        self.0
            .iter()
            .filter_map(|s| match s {
                Segment::Autocxx(includecpp) => Some(includecpp.include_dirs()),
                _ => None,
            })
            .flatten()
    }

    pub fn resolve_all(
        &mut self,
        autocxx_inc: Vec<PathBuf>,
        extra_clang_args: &[&str],
        dep_recorder: Option<Box<dyn RebuildDependencyRecorder>>,
    ) -> Result<(), ParseError> {
        let mut mods_found = HashSet::new();
        let inner_dep_recorder: Option<Rc<dyn RebuildDependencyRecorder>> =
            dep_recorder.map(Rc::from);
        for include_cpp in self.get_autocxxes_mut() {
            #[allow(clippy::manual_map)] // because of dyn shenanigans
            let dep_recorder: Option<Box<dyn RebuildDependencyRecorder>> = match &inner_dep_recorder
            {
                None => None,
                Some(inner_dep_recorder) => Some(Box::new(CompositeDepRecorder::new(
                    inner_dep_recorder.clone(),
                ))),
            };
            if !mods_found.insert(include_cpp.get_mod_name()) {
                return Err(ParseError::ConflictingModNames);
            }
            include_cpp
                .generate(autocxx_inc.clone(), extra_clang_args, dep_recorder)
                .map_err(ParseError::AutocxxCodegenError)?
        }
        Ok(())
    }
}

impl ToTokens for ParsedFile {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        for seg in &self.0 {
            match seg {
                Segment::Other(item) => item.to_tokens(tokens),
                Segment::Autocxx(autocxx) => {
                    let these_tokens = autocxx.generate_rs();
                    tokens.extend(these_tokens);
                }
                Segment::Cxx(itemmod) => itemmod.to_tokens(tokens),
            }
        }
    }
}

/// Shenanigans required to share the same RebuildDependencyRecorder
/// with all of the include_cpp instances in this one file.
#[derive(Debug, Clone)]
struct CompositeDepRecorder(Rc<dyn RebuildDependencyRecorder>);

impl CompositeDepRecorder {
    fn new(inner: Rc<dyn RebuildDependencyRecorder>) -> Self {
        CompositeDepRecorder(inner)
    }
}

impl UnwindSafe for CompositeDepRecorder {}

impl RebuildDependencyRecorder for CompositeDepRecorder {
    fn record_header_file_dependency(&self, filename: &str) {
        self.0.record_header_file_dependency(filename);
    }
}
