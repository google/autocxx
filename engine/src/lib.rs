//! The core of the `autocxx` engine, used by both the
//! `autocxx_macro` and also code generators (e.g. `autocxx_build`).
//! See [IncludeCppEngine] for general description of how this engine works.

// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#![forbid(unsafe_code)]
#![cfg_attr(feature = "nightly", feature(doc_cfg))]

mod ast_discoverer;
mod conversion;
mod cxxbridge;
mod known_types;
mod minisyn;
mod output_generators;
mod parse_callbacks;
mod parse_file;
mod rust_pretty_printer;
mod types;

#[cfg(any(test, feature = "build"))]
mod builder;

use autocxx_bindgen::BindgenError;
use autocxx_parser::{IncludeCppConfig, UnsafePolicy};
use conversion::BridgeConverter;
use miette::{SourceOffset, SourceSpan};
use parse_callbacks::{AutocxxParseCallbacks, ParseCallbackResults, UnindexedParseCallbackResults};
use parse_file::CppBuildable;
use proc_macro2::TokenStream as TokenStream2;
use regex::Regex;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::{
    fs::File,
    io::prelude::*,
    path::Path,
    process::{Command, Stdio},
};
use tempfile::NamedTempFile;

use quote::ToTokens;
use syn::Result as ParseResult;
use syn::{
    parse::{Parse, ParseStream},
    parse_quote, ItemMod, Macro,
};
use thiserror::Error;

use itertools::{join, Itertools};
use known_types::known_types;
use log::info;
use miette::Diagnostic;

/// We use a forked version of bindgen - for now.
/// We hope to unfork.
use autocxx_bindgen as bindgen;

#[cfg(any(test, feature = "build"))]
pub use builder::{
    Builder, BuilderBuild, BuilderContext, BuilderError, BuilderResult, BuilderSuccess,
};
pub use output_generators::{generate_rs_archive, generate_rs_single, RsOutput};
pub use parse_file::{parse_file, ParseError, ParsedFile};

pub use cxx_gen::HEADER;

#[derive(Clone)]
/// Some C++ content which should be written to disk and built.
pub struct CppFilePair {
    /// Declarations to go into a header file.
    pub header: Vec<u8>,
    /// Implementations to go into a .cpp file.
    pub implementation: Option<Vec<u8>>,
    /// The name which should be used for the header file
    /// (important as it may be `#include`d elsewhere)
    pub header_name: String,
}

/// All generated C++ content which should be written to disk.
pub struct GeneratedCpp(pub Vec<CppFilePair>);

/// A [`syn::Error`] which also implements [`miette::Diagnostic`] so can be pretty-printed
/// to show the affected span of code.
#[derive(Error, Debug, Diagnostic)]
#[error("{err}")]
pub struct LocatedSynError {
    err: syn::Error,
    #[source_code]
    file: String,
    #[label("error here")]
    span: SourceSpan,
}

impl LocatedSynError {
    fn new(err: syn::Error, file: &str) -> Self {
        let span = proc_macro_span_to_miette_span(&err.span());
        Self {
            err,
            file: file.to_string(),
            span,
        }
    }
}

/// Errors which may occur in generating bindings for these C++
/// functions.
#[derive(Debug, Error, Diagnostic)]
pub enum Error {
    #[error("Bindgen was unable to generate the initial .rs bindings for this file. This may indicate a parsing problem with the C++ headers.")]
    Bindgen(BindgenError),
    #[error(transparent)]
    #[diagnostic(transparent)]
    MacroParsing(LocatedSynError),
    #[error(transparent)]
    #[diagnostic(transparent)]
    BindingsParsing(LocatedSynError),
    #[error("no C++ include directory was provided.")]
    NoAutoCxxInc,
    #[error(transparent)]
    #[diagnostic(transparent)]
    Conversion(conversion::ConvertError),
    #[error("Using `unsafe_references_wrapped` requires the Rust nightly `arbitrary_self_types` feature")]
    WrappedReferencesButNoArbitrarySelfTypes,
}

/// Result type.
pub type Result<T, E = Error> = std::result::Result<T, E>;

struct GenerationResults {
    item_mod: ItemMod,
    cpp: Option<CppFilePair>,
    #[allow(dead_code)]
    inc_dirs: Vec<PathBuf>,
    cxxgen_header_name: String,
}
enum State {
    NotGenerated,
    ParseOnly,
    Generated(Box<GenerationResults>),
}

/// Code generation options.
#[derive(Default)]
pub struct CodegenOptions<'a> {
    // An option used by the test suite to force a more convoluted
    // route through our code, to uncover bugs.
    pub force_wrapper_gen: bool,
    /// Options about the C++ code generation.
    pub cpp_codegen_options: CppCodegenOptions<'a>,
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
///     syn ==> parse_bindgen
///     end
///     parse_bindgen ==> apis
///     subgraph analysis
///     typedef[typedef analysis]
///     pod[POD analysis]
///     apis ==> typedef
///     typedef ==> pod
///     podapis(APIs with POD analysis)
///     pod ==> podapis
///     fun[Function materialization analysis]
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
    source_code: Option<Rc<String>>, // so we can create diagnostics
}

impl Parse for IncludeCppEngine {
    fn parse(input: ParseStream) -> ParseResult<Self> {
        let config = input.parse::<IncludeCppConfig>()?;
        let state = if config.parse_only {
            State::ParseOnly
        } else {
            State::NotGenerated
        };
        Ok(Self {
            config,
            state,
            source_code: None,
        })
    }
}

impl IncludeCppEngine {
    pub fn new_from_syn(mac: Macro, file_contents: Rc<String>) -> Result<Self> {
        let mut this = mac
            .parse_body::<IncludeCppEngine>()
            .map_err(|e| Error::MacroParsing(LocatedSynError::new(e, &file_contents)))?;
        this.source_code = Some(file_contents);
        Ok(this)
    }

    /// Used if we find that we're asked to auto-discover extern_rust_type and similar
    /// but didn't have any include_cpp macro at all.
    pub fn new_for_autodiscover() -> Self {
        Self {
            config: IncludeCppConfig::default(),
            state: State::NotGenerated,
            source_code: None,
        }
    }

    pub fn config_mut(&mut self) -> &mut IncludeCppConfig {
        assert!(
            matches!(self.state, State::NotGenerated),
            "Can't alter config after generation commenced"
        );
        &mut self.config
    }

    fn build_header(&self) -> String {
        join(
            self.config
                .inclusions
                .iter()
                .map(|path| format!("#include \"{path}\"\n")),
            "",
        )
    }

    fn make_bindgen_builder(
        &self,
        inc_dirs: &[PathBuf],
        extra_clang_args: &[&str],
    ) -> bindgen::Builder {
        let bindgen_marker_types = ["Opaque", "Reference", "RValueReference"];
        let raw_line = bindgen_marker_types
            .iter()
            .map(|t| format!("#[repr(transparent)] pub struct __bindgen_marker_{t}<T: ?Sized>(T);"))
            .join(" ");
        let use_list = bindgen_marker_types
            .iter()
            .map(|t| format!("__bindgen_marker_{t}"))
            .join(", ");
        let all_module_raw_line = format!("#[allow(unused_imports)] use super::{{{use_list}}}; #[allow(unused_imports)] use autocxx::c_char16_t as bindgen_cchar16_t;");

        let mut builder = bindgen::builder()
            .clang_args(make_clang_args(inc_dirs, extra_clang_args))
            .derive_copy(false)
            .derive_debug(false)
            .default_enum_style(bindgen::EnumVariation::Rust {
                non_exhaustive: false,
            })
            .formatter(if log::log_enabled!(log::Level::Info) {
                bindgen::Formatter::Rustfmt
            } else {
                bindgen::Formatter::None
            })
            .size_t_is_usize(true)
            .enable_cxx_namespaces()
            .generate_inline_functions(true)
            .respect_cxx_access_specs(true)
            .use_specific_virtual_function_receiver(true)
            .use_opaque_newtype_wrapper(true)
            .use_reference_newtype_wrapper(true)
            .represent_cxx_operators(true)
            .use_distinct_char16_t(true)
            .generate_deleted_functions(true)
            .generate_pure_virtuals(true)
            .raw_line(raw_line)
            .every_module_raw_line(all_module_raw_line)
            .generate_private_functions(true)
            .layout_tests(false); // TODO revisit later

        // 3. Passes allowlist and other options to the bindgen::Builder equivalent
        //    to --output-style=cxx --allowlist=<as passed in>
        if let Some(allowlist) = self.config.bindgen_allowlist() {
            for a in allowlist {
                // TODO - allowlist type/functions/separately
                builder = builder
                    .allowlist_type(&a)
                    .allowlist_function(&a)
                    .allowlist_function(format!("{a}_bindgen_original"))
                    .allowlist_var(&a);
            }
        }

        for item in &self.config.opaquelist {
            builder = builder.opaque_type(item);
        }

        // At this point it woul be great to use `Builder::opaque_type` for
        // everything which is on the allowlist but not on the POD list.
        // This would free us from a large proportion of bindgen bugs which
        // are dealing with obscure templated types. Unfortunately, even
        // for types which we expose to the user as opaque (non-POD), autocxx
        // internally still cares about seeing what fields they've got because
        // we make decisions about implicit constructors on that basis.
        // So, for now, we can't do that. Perhaps in future bindgen could
        // gain an option to generate any implicit constructors, if that
        // information is exposed by clang. That would remove a lot of
        // autocxx complexity and would allow us to request opaque types.

        log::info!(
            "Bindgen flags would be: {}",
            builder
                .command_line_flags()
                .into_iter()
                .map(|f| format!("\"{f}\""))
                .join(" ")
        );
        builder
    }

    pub fn get_rs_filename(&self) -> String {
        self.config.get_rs_filename()
    }

    /// Generate the Rust bindings. Call `generate` first.
    pub fn get_rs_output(&self) -> RsOutput {
        RsOutput {
            config: &self.config,
            rs: match &self.state {
                State::NotGenerated => panic!("Generate first"),
                State::Generated(gen_results) => gen_results.item_mod.to_token_stream(),
                State::ParseOnly => TokenStream2::new(),
            },
        }
    }

    /// Returns the name of the mod which this `include_cpp!` will generate.
    /// Can and should be used to ensure multiple mods in a file don't conflict.
    pub fn get_mod_name(&self) -> String {
        self.config.get_mod_name().to_string()
    }

    fn parse_bindings(&self, bindings: bindgen::Bindings) -> Result<ItemMod> {
        // This bindings object is actually a TokenStream internally and we're wasting
        // effort converting to and from string. We could enhance the bindgen API
        // in future.
        let bindings = bindings.to_string();
        // Manually add the mod ffi {} so that we can ask syn to parse
        // into a single construct.
        let bindings = format!("mod bindgen {{ {bindings} }}");
        info!("Bindings: {}", bindings);
        syn::parse_str::<ItemMod>(&bindings)
            .map_err(|e| Error::BindingsParsing(LocatedSynError::new(e, &bindings)))
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
        codegen_options: &CodegenOptions,
    ) -> Result<()> {
        // If we are in parse only mode, do nothing. This is used for
        // doc tests to ensure the parsing is valid, but we can't expect
        // valid C++ header files or linkers to allow a complete build.
        match self.state {
            State::ParseOnly => return Ok(()),
            State::NotGenerated => {}
            State::Generated(_) => panic!("Only call generate once"),
        }

        if matches!(
            self.config.unsafe_policy,
            UnsafePolicy::ReferencesWrappedAllFunctionsSafe
        ) && !rustversion::cfg!(nightly)
        {
            return Err(Error::WrappedReferencesButNoArbitrarySelfTypes);
        }

        let parse_callback_results =
            Rc::new(RefCell::new(UnindexedParseCallbackResults::default()));
        let mod_name = self.config.get_mod_name();
        let mut builder = self
            .make_bindgen_builder(&inc_dirs, extra_clang_args)
            .parse_callbacks(Box::new(AutocxxParseCallbacks::new(
                dep_recorder,
                parse_callback_results.clone(),
            )));
        let header_contents = self.build_header();
        self.dump_header_if_so_configured(&header_contents, &inc_dirs, extra_clang_args);
        let header_and_prelude = format!("{}\n\n{}", known_types().get_prelude(), header_contents);
        log::info!("Header and prelude for bindgen:\n{}", header_and_prelude);
        builder = builder.header_contents("example.hpp", &header_and_prelude);

        let bindings = builder.generate().map_err(Error::Bindgen)?;
        let bindings = self.parse_bindings(bindings)?;
        let parse_callback_results = parse_callback_results.take();
        log::info!("Parse callback results: {:?}", parse_callback_results);

        // Source code contents just used for diagnostics - if we don't have it,
        // use a blank string and miette will not attempt to annotate it nicely.
        let source_file_contents = self
            .source_code
            .as_ref()
            .cloned()
            .unwrap_or_else(|| Rc::new("".to_string()));

        let converter = BridgeConverter::new(&self.config.inclusions, &self.config);

        let conversion = converter
            .convert(
                bindings,
                parse_callback_results.index(),
                self.config.unsafe_policy.clone(),
                header_contents,
                codegen_options,
                &source_file_contents,
            )
            .map_err(Error::Conversion)?;
        let items = conversion.rs;
        let new_bindings: ItemMod = parse_quote! {
            #[allow(non_snake_case)]
            #[allow(dead_code)]
            #[allow(non_upper_case_globals)]
            #[allow(non_camel_case_types)]
            #[allow(unsafe_op_in_unsafe_fn)]
            #[doc = "Generated using autocxx - do not edit directly"]
            #[doc = "@generated"]
            mod #mod_name {
                #(#items)*
            }
        };
        info!(
            "New bindings:\n{}",
            rust_pretty_printer::pretty_print(&new_bindings)
        );
        self.state = State::Generated(Box::new(GenerationResults {
            item_mod: new_bindings,
            cpp: conversion.cpp,
            inc_dirs,
            cxxgen_header_name: conversion.cxxgen_header_name,
        }));
        Ok(())
    }

    /// Return the include directories used for this include_cpp invocation.
    #[cfg(any(test, feature = "build"))]
    fn include_dirs(&self) -> impl Iterator<Item = &PathBuf> {
        match &self.state {
            State::Generated(gen_results) => gen_results.inc_dirs.iter(),
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
            self.make_preprocessed_file(
                &PathBuf::from(output_path),
                header,
                inc_dirs,
                extra_clang_args,
            );
        }
        #[cfg(feature = "reproduction_case")]
        if let Ok(output_path) = std::env::var("AUTOCXX_REPRO_CASE") {
            let tf = NamedTempFile::new().unwrap();
            self.make_preprocessed_file(
                &PathBuf::from(tf.path()),
                header,
                inc_dirs,
                extra_clang_args,
            );
            let header = std::fs::read(tf.path()).unwrap();
            let header = String::from_utf8_lossy(&header);
            let output_path = PathBuf::from(output_path);
            let config = self.config.to_token_stream().to_string();
            let json = serde_json::json!({
                "header": header,
                "config": config
            });
            let f = File::create(output_path).unwrap();
            serde_json::to_writer(f, &json).unwrap();
        }
    }

    fn make_preprocessed_file(
        &self,
        output_path: &Path,
        header: &str,
        inc_dirs: &[PathBuf],
        extra_clang_args: &[&str],
    ) {
        // Include a load of system headers at the end of the preprocessed output,
        // because we would like to be able to generate bindings from the
        // preprocessed header, and then build those bindings. The C++ parts
        // of those bindings might need things inside these various headers;
        // we make sure all these definitions and declarations are inside
        // this one header file so that the reduction process does not have
        // to refer to local headers on the reduction machine too.
        let suffix = ALL_KNOWN_SYSTEM_HEADERS
            .iter()
            .map(|hdr| format!("#include <{hdr}>\n"))
            .join("\n");
        let input = format!("/*\nautocxx config:\n\n{:?}\n\nend autocxx config.\nautocxx preprocessed input:\n*/\n\n{}\n\n/* autocxx: extra headers added below for completeness. */\n\n{}\n{}\n",
            self.config, header, suffix, cxx_gen::HEADER);
        let mut tf = NamedTempFile::new().unwrap();
        write!(tf, "{input}").unwrap();
        let tp = tf.into_temp_path();
        preprocess(&tp, &PathBuf::from(output_path), inc_dirs, extra_clang_args).unwrap();
    }
}

/// This is a list of all the headers known to be included in generated
/// C++ by cxx. We only use this when `AUTOCXX_PERPROCESS` is set to true,
/// in an attempt to make the resulting preprocessed header more hermetic.
/// We clearly should _not_ use this in any other circumstance; obviously
/// we'd then want to add an API to cxx_gen such that we could retrieve
/// that information from source.
static ALL_KNOWN_SYSTEM_HEADERS: &[&str] = &[
    "memory",
    "string",
    "algorithm",
    "array",
    "cassert",
    "cstddef",
    "cstdint",
    "cstring",
    "exception",
    "functional",
    "initializer_list",
    "iterator",
    "memory",
    "new",
    "stdexcept",
    "type_traits",
    "utility",
    "vector",
    "sys/types.h",
];

pub fn do_cxx_cpp_generation(
    rs: TokenStream2,
    cpp_codegen_options: &CppCodegenOptions,
    cxxgen_header_name: String,
) -> Result<CppFilePair, cxx_gen::Error> {
    let mut opt = cxx_gen::Opt::default();
    opt.cxx_impl_annotations
        .clone_from(&cpp_codegen_options.cxx_impl_annotations);
    let cxx_generated = cxx_gen::generate_header_and_cc(rs, &opt)?;
    Ok(CppFilePair {
        header: strip_system_headers(
            cxx_generated.header,
            cpp_codegen_options.suppress_system_headers,
        ),
        header_name: cxxgen_header_name,
        implementation: Some(strip_system_headers(
            cxx_generated.implementation,
            cpp_codegen_options.suppress_system_headers,
        )),
    })
}

pub fn get_cxx_header_bytes(suppress_system_headers: bool) -> Vec<u8> {
    strip_system_headers(cxx_gen::HEADER.as_bytes().to_vec(), suppress_system_headers)
}

fn strip_system_headers(input: Vec<u8>, suppress_system_headers: bool) -> Vec<u8> {
    if suppress_system_headers {
        std::str::from_utf8(&input)
            .unwrap()
            .lines()
            .filter(|l| !l.starts_with("#include <"))
            .join("\n")
            .as_bytes()
            .to_vec()
    } else {
        input
    }
}

impl CppBuildable for IncludeCppEngine {
    /// Generate C++-side bindings for these APIs. Call `generate` first.
    fn generate_h_and_cxx(
        &self,
        cpp_codegen_options: &CppCodegenOptions,
    ) -> Result<GeneratedCpp, cxx_gen::Error> {
        let mut files = Vec::new();
        match &self.state {
            State::ParseOnly => panic!("Cannot generate C++ in parse-only mode"),
            State::NotGenerated => panic!("Call generate() first"),
            State::Generated(gen_results) => {
                let rs = gen_results.item_mod.to_token_stream();
                files.push(do_cxx_cpp_generation(
                    rs,
                    cpp_codegen_options,
                    gen_results.cxxgen_header_name.clone(),
                )?);
                if let Some(cpp_file_pair) = &gen_results.cpp {
                    files.push(cpp_file_pair.clone());
                }
            }
        };
        Ok(GeneratedCpp(files))
    }
}

/// Get clang args as if we were operating clang the same way as we operate
/// bindgen.
pub fn make_clang_args<'a>(
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
    let mut cmd = Command::new(get_clang_path());
    cmd.arg("-E");
    cmd.arg("-C");
    cmd.args(make_clang_args(incs, extra_clang_args));
    cmd.arg(listing_path.to_str().unwrap());
    cmd.stderr(Stdio::inherit());
    let result = cmd.output().expect("failed to execute clang++");
    assert!(result.status.success(), "failed to preprocess");
    let mut file = File::create(preprocess_path)?;
    file.write_all(&result.stdout)?;
    Ok(())
}

/// Get the path to clang which is effective for any preprocessing
/// operations done by autocxx.
pub fn get_clang_path() -> String {
    // `CLANG_PATH` is the environment variable that clang-sys uses to specify
    // the path to Clang, so in most cases where someone is using a compiler
    // that's not on the path, things should just work. We also check `CXX`,
    // since some users may have set that.
    std::env::var("CLANG_PATH")
        .or_else(|_| std::env::var("CXX"))
        .unwrap_or_else(|_| "clang++".to_string())
}

/// Function to generate the desired name of the header containing autocxx's
/// extra generated C++.
/// Newtype wrapper so we can give it a [`Default`].
pub struct AutocxxgenHeaderNamer<'a>(pub Box<dyn 'a + Fn(String) -> String>);

impl Default for AutocxxgenHeaderNamer<'static> {
    fn default() -> Self {
        Self(Box::new(|mod_name| format!("autocxxgen_{mod_name}.h")))
    }
}

impl AutocxxgenHeaderNamer<'_> {
    fn name_header(&self, mod_name: String) -> String {
        self.0(mod_name)
    }
}

/// Function to generate the desired name of the header containing cxx's
/// declarations.
/// Newtype wrapper so we can give it a [`Default`].
pub struct CxxgenHeaderNamer<'a>(pub Box<dyn 'a + Fn() -> String>);

impl Default for CxxgenHeaderNamer<'static> {
    fn default() -> Self {
        // The default implementation here is to name these headers
        // cxxgen.h, cxxgen1.h, cxxgen2.h etc.
        // These names are not especially predictable by callers and this
        // behavior is not tested anywhere - so this is considered semi-
        // supported, at best. This only comes into play in the rare case
        // that you're generating bindings to multiple include_cpp!
        // or a mix of include_cpp! and #[cxx::bridge] bindings.
        let header_counter = Rc::new(RefCell::new(0));
        Self(Box::new(move || {
            let header_counter = header_counter.clone();
            let header_counter_cell = header_counter.as_ref();
            let mut header_counter = header_counter_cell.borrow_mut();
            if *header_counter == 0 {
                *header_counter += 1;
                "cxxgen.h".into()
            } else {
                let count = *header_counter;
                *header_counter += 1;
                format!("cxxgen{count}.h")
            }
        }))
    }
}

impl CxxgenHeaderNamer<'_> {
    fn name_header(&self) -> String {
        self.0()
    }
}

/// Options for C++ codegen
#[derive(Default)]
pub struct CppCodegenOptions<'a> {
    /// Whether to avoid generating `#include <some-system-header>`.
    /// You may wish to do this to make a hermetic test case with no
    /// external dependencies.
    pub suppress_system_headers: bool,
    /// Optionally, a prefix to go at `#include "*here*cxx.h". This is a header file from the `cxx`
    /// crate.
    pub path_to_cxx_h: Option<String>,
    /// Optionally, a prefix to go at `#include "*here*cxxgen.h". This is a header file which we
    /// generate.
    pub path_to_cxxgen_h: Option<String>,
    /// Optionally, a function called to determine the name that will be used
    /// for the autocxxgen.h file.
    /// The function is passed the name of the module generated by each `include_cpp`,
    /// configured via `name`. These will be unique.
    pub autocxxgen_header_namer: AutocxxgenHeaderNamer<'a>,
    /// A function to generate the name of the cxxgen.h header that should be output.
    pub cxxgen_header_namer: CxxgenHeaderNamer<'a>,
    /// An annotation optionally to include on each C++ function.
    /// For example to export the symbol from a library.
    pub cxx_impl_annotations: Option<String>,
}

fn proc_macro_span_to_miette_span(span: &proc_macro2::Span) -> SourceSpan {
    // A proc_macro2::Span stores its location as a byte offset. But there are
    // no APIs to get that offset out.
    // We could use `.start()` and `.end()` to get the line + column numbers, but it appears
    // they're a little buggy. Hence we do this, to get the offsets directly across into
    // miette.
    struct Err;
    let r: Result<(usize, usize), Err> = (|| {
        let span_desc = format!("{span:?}");
        let re = Regex::new(r"(\d+)..(\d+)").unwrap();
        let captures = re.captures(&span_desc).ok_or(Err)?;
        let start = captures.get(1).ok_or(Err)?;
        let start: usize = start.as_str().parse().map_err(|_| Err)?;
        let start = start.saturating_sub(1); // proc_macro::Span offsets seem to be off-by-one
        let end = captures.get(2).ok_or(Err)?;
        let end: usize = end.as_str().parse().map_err(|_| Err)?;
        let end = end.saturating_sub(1); // proc_macro::Span offsets seem to be off-by-one
        Ok((start, end.saturating_sub(start)))
    })();
    let (start, end) = r.unwrap_or((0, 0));
    SourceSpan::new(SourceOffset::from(start), SourceOffset::from(end))
}
