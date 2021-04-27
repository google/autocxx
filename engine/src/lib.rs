//! The core of the `autocxx` engine, used by both the
//! `autocxx_macro` and also code generators (e.g. `autocxx_build`).
//! See [IncludeCppEngine] for general description of how this engine works.

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

mod conversion;
mod known_types;
mod parse_callbacks;
mod parse_file;
mod rust_pretty_printer;
mod types;

#[cfg(any(test, feature = "build"))]
mod builder;

#[cfg(test)]
mod integration_tests;

use autocxx_parser::{IncludeCppConfig, UnsafePolicy};
use conversion::BridgeConverter;
use parse_callbacks::AutocxxParseCallbacks;
use proc_macro2::TokenStream as TokenStream2;
use std::{
    collections::hash_map::DefaultHasher,
    fmt::Display,
    hash::{Hash, Hasher},
    path::PathBuf,
};
use std::{fs::File, io::prelude::*, path::Path, process::Command};
use tempfile::NamedTempFile;

use quote::ToTokens;
use syn::Result as ParseResult;
use syn::{
    parse::{Parse, ParseStream},
    parse_quote, ItemMod, Macro,
};

use itertools::join;
use known_types::known_types;
use log::info;

/// We use a forked version of bindgen - for now.
/// We hope to unfork.
use autocxx_bindgen as bindgen;

#[cfg(any(test, feature = "build"))]
pub use builder::{build, expect_build, BuilderBuild, BuilderError, BuilderResult, BuilderSuccess};
pub use parse_file::{parse_file, ParseError, ParsedFile};

pub use cxx_gen::HEADER;

/// Re-export cxx such that clients can use the same version as
/// us. This doesn't enable clients to avoid depending on the cxx
/// crate too, unfortunately, since generated cxx::bridge code
/// refers explicitly to ::cxx. See
/// <https://github.com/google/autocxx/issues/36>
pub use cxx;

#[derive(Clone)]
/// Some C++ content which should be written to disk and built.
pub struct CppFilePair {
    /// Declarations to go into a header file.
    pub header: Vec<u8>,
    /// Implementations to go into a .cpp file, if any.
    pub implementation: Option<Vec<u8>>,
    /// The name which should be used for the header file
    /// (important as it may be `#include`d elsewhere)
    pub header_name: String,
}

/// All generated C++ content which should be written to disk.
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
    /// Some error occcurred in converting the bindgen-style
    /// bindings to safe cxx bindings.
    Conversion(conversion::ConvertError),
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
            Error::NoAutoCxxInc => write!(f, "No C++ include directory was provided.")?,
            Error::Conversion(err) => write!(f, "autocxx could not generate the requested bindings. {}", err)?,
            Error::NoGenerationRequested => write!(f, "No 'generate' or 'generate_pod' directives were found, so we would not generate any Rust bindings despite the inclusion of C++ headers.")?,
        }
        Ok(())
    }
}

/// Result type.
pub type Result<T, E = Error> = std::result::Result<T, E>;

struct GenerationResults {
    item_mod: ItemMod,
    cpp: Option<CppFilePair>,
    inc_dirs: Vec<PathBuf>,
}
enum State {
    NotGenerated,
    ParseOnly,
    Generated(Box<GenerationResults>),
}

const AUTOCXX_CLANG_ARGS: &[&str; 4] = &["-x", "c++", "-std=c++14", "-DBINDGEN"];

/// Implement to learn of header files which get included
/// by this build process, such that your build system can choose
/// to rerun the build process if any such file changes in future.
pub trait RebuildDependencyRecorder: std::fmt::Debug {
    /// Records that this autocxx build depends on the given
    /// header file. Full paths will be provided.
    fn record_header_file_dependency(&self, filename: &str);
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Core of the autocxx engine.
///
/// The basic idea is this. We will run `bindgen` which will spit
/// out a ton of Rust code corresponding to all the types and functions
/// defined in C++. We'll then post-process that bindgen output
/// into a form suitable for ingestion by `cxx`.
/// (It's the `BridgeConverter` mod which does that.)
/// Along the way, the `bridge_converter` might tell us of additional
/// C++ code which we should generate, e.g. wrappers to move things
/// into and out of `UniquePtr`s.
///
/// ```mermaid
/// flowchart TB
///     s[(C++ headers)]
///     s --> lc
///     rss[(.rs input)]
///     rss --> parser
///     parser --> include_cpp_conf
///     cpp_output[(C++ output)]
///     rs_output[(.rs output)]
///     subgraph autocxx[autocxx_engine]
///     parser[File parser]
///     subgraph bindgen[autocxx_bindgen]
///     lc[libclang parse]
///     bir(bindgen IR)
///     lc --> bir
///     end
///     bgo(bindgen generated bindings)
///     bir --> bgo
///     include_cpp_conf(Config from include_cpp)
///     syn[Parse with syn]
///     bgo --> syn
///     conv[['conversion' mod: see below]]
///     syn --> conv
///     rsgen(Generated .rs TokenStream)
///     conv --> rsgen
///     subgraph cxx_gen
///     cxx_codegen[cxx_gen C++ codegen]
///     end
///     rsgen --> cxx_codegen
///     end
///     conv -- autocxx C++ codegen --> cpp_output
///     rsgen -- autocxx .rs codegen --> rs_output
///     cxx_codegen -- cxx C++ codegen --> cpp_output
///     subgraph rustc [rustc build]
///     subgraph autocxx_macro
///     include_cpp[autocxx include_cpp macro]
///     end
///     subgraph cxx
///     cxxm[cxx procedural macro]
///     end
///     comprs(Fully expanded Rust code)
///     end
///     rs_output-. included .->include_cpp
///     include_cpp --> cxxm
///     cxxm --> comprs
///     rss --> rustc
///     include_cpp_conf -. used to configure .-> bindgen
///     include_cpp_conf --> conv
///     link[linker]
///     cpp_output --> link
///     comprs --> link
/// ```
///
/// Here's a zoomed-in view of the "conversion" part:
///
/// ```mermaid
/// flowchart TB
///     syn[(syn parse)]
///     apis(Unanalyzed APIs)
///     subgraph parse
///     tc(TypeConverter)
///     syn ==> parse_bindgen
///     end
///     parse_bindgen ==> apis
///     parse_bindgen -.-> tc
///     subgraph analysis
///     pod[POD analysis]
///     tc -.-> pod
///     apis ==> pod
///     podapis(APIs with POD analysis)
///     pod ==> podapis
///     fun[Function materialization analysis]
///     tc -.-> fun
///     podapis ==> fun
///     funapis(APIs with function analysis)
///     fun ==> funapis
///     gc[Garbage collection]
///     funapis ==> gc
///     ctypes[C int analysis]
///     gc ==> ctypes
///     ctypes ==> finalapis
///     end
///     finalapis(Analyzed APIs)
///     codegenrs(.rs codegen)
///     codegencpp(.cpp codegen)
///     finalapis ==> codegenrs
///     finalapis ==> codegencpp
/// ```
pub struct IncludeCppEngine {
    config: IncludeCppConfig,
    state: State,
}

impl Parse for IncludeCppEngine {
    fn parse(input: ParseStream) -> ParseResult<Self> {
        let config = input.parse::<IncludeCppConfig>()?;
        let state = if config.parse_only {
            State::ParseOnly
        } else {
            State::NotGenerated
        };
        Ok(Self { config, state })
    }
}

impl IncludeCppEngine {
    pub fn new_from_syn(mac: Macro) -> Result<Self> {
        mac.parse_body::<IncludeCppEngine>().map_err(Error::Parsing)
    }

    fn build_header(&self) -> String {
        join(
            self.config
                .inclusions
                .iter()
                .map(|path| format!("#include \"{}\"\n", path)),
            "",
        )
    }

    fn make_bindgen_builder(
        &self,
        inc_dirs: &[PathBuf],
        extra_clang_args: &[&str],
    ) -> bindgen::Builder {
        let mut builder = bindgen::builder()
            .clang_args(make_clang_args(inc_dirs, extra_clang_args))
            .derive_copy(false)
            .derive_debug(false)
            .default_enum_style(bindgen::EnumVariation::Rust {
                non_exhaustive: false,
            })
            .enable_cxx_namespaces()
            .generate_inline_functions(true)
            .layout_tests(false); // TODO revisit later
        for item in known_types::get_initial_blocklist() {
            builder = builder.blocklist_item(item);
        }

        // 3. Passes allowlist and other options to the bindgen::Builder equivalent
        //    to --output-style=cxx --allowlist=<as passed in>
        for a in self.config.type_config.allowlist() {
            // TODO - allowlist type/functions/separately
            builder = builder
                .allowlist_type(&a)
                .allowlist_function(&a)
                .allowlist_var(&a);
        }

        builder
    }

    pub fn get_rs_filename(&self) -> String {
        let mut hasher = DefaultHasher::new();
        self.config.hash(&mut hasher);
        let id = hasher.finish();
        format!("{}.rs", id)
    }

    /// Generate the Rust bindings. Call `generate` first.
    pub fn generate_rs(&self) -> TokenStream2 {
        match &self.state {
            State::NotGenerated => panic!("Generate first"),
            State::Generated(gen_results) => gen_results.item_mod.to_token_stream(),
            State::ParseOnly => TokenStream2::new(),
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

    /// Actually examine the headers to find out what needs generating.
    /// Most errors occur at this stage as we fail to interpret the C++
    /// headers properly.
    ///
    /// See documentation for this type for flow diagrams and more details.
    pub fn generate(
        &mut self,
        inc_dirs: Vec<PathBuf>,
        extra_clang_args: &[&str],
        dep_recorder: Option<Box<dyn RebuildDependencyRecorder>>,
    ) -> Result<()> {
        // If we are in parse only mode, do nothing. This is used for
        // doc tests to ensure the parsing is valid, but we can't expect
        // valid C++ header files or linkers to allow a complete build.
        match self.state {
            State::ParseOnly => return Ok(()),
            State::NotGenerated => {}
            State::Generated(_) => panic!("Only call generate once"),
        }

        if self.config.type_config.allowlist_is_empty() {
            return Err(Error::NoGenerationRequested);
        }

        let mut builder = self.make_bindgen_builder(&inc_dirs, &extra_clang_args);
        if let Some(dep_recorder) = dep_recorder {
            builder = builder.parse_callbacks(Box::new(AutocxxParseCallbacks(dep_recorder)));
        }
        let header_contents = self.build_header();
        self.dump_header_if_so_configured(&header_contents, &inc_dirs, &extra_clang_args);
        let header_and_prelude = format!("{}\n\n{}", known_types().get_prelude(), header_contents);
        builder = builder.header_contents("example.hpp", &header_and_prelude);

        let bindings = builder.generate().map_err(Error::Bindgen)?;
        let bindings = self.parse_bindings(bindings)?;

        let converter = BridgeConverter::new(&self.config.inclusions, &self.config.type_config);

        let conversion = converter
            .convert(bindings, self.config.unsafe_policy.clone(), header_contents)
            .map_err(Error::Conversion)?;
        let mut items = conversion.rs;
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
        self.state = State::Generated(Box::new(GenerationResults {
            item_mod: new_bindings,
            cpp: conversion.cpp,
            inc_dirs,
        }));
        Ok(())
    }

    /// Generate C++-side bindings for these APIs. Call `generate` first.
    pub fn generate_h_and_cxx(&self) -> Result<GeneratedCpp, cxx_gen::Error> {
        let mut files = Vec::new();
        match &self.state {
            State::ParseOnly => panic!("Cannot generate C++ in parse-only mode"),
            State::NotGenerated => panic!("Call generate() first"),
            State::Generated(gen_results) => {
                let rs = gen_results.item_mod.to_token_stream();
                let opt = cxx_gen::Opt::default();
                let cxx_generated = cxx_gen::generate_header_and_cc(rs, &opt)?;
                files.push(CppFilePair {
                    header: cxx_generated.header,
                    header_name: "cxxgen.h".to_string(),
                    implementation: Some(cxx_generated.implementation),
                });
                if let Some(cpp_file_pair) = &gen_results.cpp {
                    files.push(cpp_file_pair.clone());
                }
            }
        };
        Ok(GeneratedCpp(files))
    }

    /// Return the include directories used for this include_cpp invocation.
    pub fn include_dirs(&self) -> &Vec<PathBuf> {
        match &self.state {
            State::Generated(gen_results) => &gen_results.inc_dirs,
            _ => panic!("Must call generate() before include_dirs()"),
        }
    }

    fn dump_header_if_so_configured(
        &self,
        header: &str,
        inc_dirs: &[PathBuf],
        extra_clang_args: &[&str],
    ) {
        if let Ok(output_path) = std::env::var("AUTOCXX_PREPROCESS") {
            let input = format!("/*\nautocxx config:\n\n{:?}\n\nend autocxx config.\nautocxx preprocessed input:\n*/\n\n{}", self.config, header);
            let mut tf = NamedTempFile::new().unwrap();
            write!(tf, "{}", input).unwrap();
            let tp = tf.into_temp_path();
            preprocess(&tp, &PathBuf::from(output_path), inc_dirs, extra_clang_args).unwrap();
        }
    }
}

fn make_clang_args<'a>(
    incs: &'a [PathBuf],
    extra_args: &'a [&str],
) -> impl Iterator<Item = String> + 'a {
    // AUTOCXX_CLANG_ARGS come first so that any defaults defined there(e.g. for the `-std`
    // argument) can be overridden by extra_args.
    AUTOCXX_CLANG_ARGS
        .iter()
        .map(|s| s.to_string())
        .chain(incs.iter().map(|i| format!("-I{}", i.to_str().unwrap())))
        .chain(extra_args.iter().map(|s| s.to_string()))
}

/// Preprocess a file using the same options
/// as is used by autocxx. Input: listing_path, output: preprocess_path.
pub fn preprocess(
    listing_path: &Path,
    preprocess_path: &Path,
    incs: &[PathBuf],
    extra_clang_args: &[&str],
) -> Result<(), std::io::Error> {
    let mut cmd = Command::new("clang++");
    cmd.arg("-E");
    cmd.arg("-C");
    cmd.args(make_clang_args(incs, extra_clang_args));
    cmd.arg(listing_path.to_str().unwrap());
    let output = cmd.output().expect("failed to preprocess").stdout;
    let mut file = File::create(preprocess_path)?;
    file.write_all(&output)?;
    Ok(())
}
