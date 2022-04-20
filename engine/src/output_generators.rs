// Copyright 2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use autocxx_parser::{IncludeCppConfig, MultiBindings};
use itertools::Itertools;
use proc_macro2::TokenStream;

use crate::{
    conversion::{ExtraCpp, Header},
    strip_system_headers, CppFilePair, GeneratedCpp,
};

use indexmap::set::IndexSet as HashSet;

/// Options for C++ codegen
#[derive(Default)]
pub struct CppCodegenOptions {
    /// Whether to avoid generating `#include <some-system-header>`.
    /// You may wish to do this to make a hermetic test case with no
    /// external dependencies.
    pub suppress_system_headers: bool,
    /// Optionally, a prefix to go at `#include "<here>cxx.h". This is a header file from the `cxx`
    /// crate.
    pub path_to_cxx_h: Option<String>,
    /// Optionally, a prefix to go at `#include "<here>cxxgen.h". This is a header file which we
    /// generate.
    pub path_to_cxxgen_h: Option<String>,
    /// An annotation optionally to include on each C++ function.
    /// For example to export the symbol from a library.
    pub cxx_impl_annotations: Option<String>,
}

/// Opaque structure representing the C++ which needs to be generated for
/// a given `include_cpp!` macro or `cxx::bridge` mod.
#[derive(Default)]
pub struct CppOutput {
    pub(crate) extra_cpp: Vec<ExtraCpp>,
    pub(crate) cxx_bridge: TokenStream,
}

/// Opaque structure representing the Rust which needs to be generated
/// for a given `include_cpp!` macro.
pub struct RsOutput<'a> {
    pub(crate) config: &'a IncludeCppConfig,
    pub(crate) rs: TokenStream,
}

/// Creates an on-disk archive (actually a JSON file) of the Rust side of the bindings
/// for multiple `include_cpp` macros. If you use this, you will want to tell
/// `autocxx_macro` how to find this file using the `AUTOCXX_RS_ARCHIVE`
/// environment variable.
pub fn generate_rs_archive<'a>(rs_outputs: impl Iterator<Item = RsOutput<'a>>) -> String {
    let mut multi_bindings = MultiBindings::default();
    for rs in rs_outputs {
        multi_bindings.insert(rs.config, rs.rs);
    }
    serde_json::to_string(&multi_bindings).expect("Unable to encode JSON archive")
}

/// A single Rust file to be written to disk.
pub struct RsInclude {
    pub code: String,
    pub filename: String,
}

/// Gets the Rust code corresponding to a single [`RsOutput`]. You can write this
/// to a file which can simply be `include!`ed by `autocxx_macro` when you give
/// it the `AUTOCXX_RS_FILE` environment variable.
pub fn generate_rs_single(rs_output: RsOutput) -> RsInclude {
    RsInclude {
        code: rs_output.rs.to_string(),
        filename: rs_output.config.get_rs_filename(),
    }
}

/// Generates the C++ code required by the provided bindings. Thiw will generate
/// one or possibly two pairs of (cc,h) files.
pub fn generate_cpp(
    cpp_outputs: impl Iterator<Item = CppOutput>,
    cpp_codegen_options: &CppCodegenOptions,
) -> Result<GeneratedCpp, cxx_gen::Error> {
    // Merge all the information from the diffetent include_cpp or cxx::bridge macros we found.
    let all_output = cpp_outputs.fold(CppOutput::default(), |mut accumulator, mut element| {
        accumulator.extra_cpp.append(&mut element.extra_cpp);
        accumulator.cxx_bridge.extend(element.cxx_bridge);
        accumulator
    });
    // Prepare to generate the C++ files from cxx.
    let mut opt = cxx_gen::Opt::default();
    opt.cxx_impl_annotations = cpp_codegen_options.cxx_impl_annotations.clone();
    let cxx_generated = cxx_gen::generate_header_and_cc(all_output.cxx_bridge, &opt)?;
    let first = CppFilePair {
        header: strip_system_headers(
            cxx_generated.header,
            cpp_codegen_options.suppress_system_headers,
        ),
        header_name: "cxxgen.h".into(),
        implementation: Some(strip_system_headers(
            cxx_generated.implementation,
            cpp_codegen_options.suppress_system_headers,
        )),
    };
    let extra_cpp_combiner = ExtraCppCombiner {
        cpp_codegen_options,
        additional_functions: &all_output.extra_cpp,
    };
    let second = extra_cpp_combiner.generate();
    Ok(GeneratedCpp { first, second })
}
struct ExtraCppCombiner<'a> {
    cpp_codegen_options: &'a CppCodegenOptions,
    additional_functions: &'a [ExtraCpp],
}

impl<'a> ExtraCppCombiner<'a> {
    fn generate(&self) -> Option<CppFilePair> {
        if self.additional_functions.is_empty() {
            None
        } else {
            let headers = self.collect_headers(|additional_need| &additional_need.headers);
            let cpp_headers = self.collect_headers(|additional_need| &additional_need.cpp_headers);
            let type_definitions = self.concat_additional_items(|x| x.type_definition.as_ref());
            let declarations = self.concat_additional_items(|x| x.declaration.as_ref());
            let declarations = format!(
                "#ifndef __AUTOCXXGEN_H__\n#define __AUTOCXXGEN_H__\n\n{}\n{}\n{}#endif // __AUTOCXXGEN_H__\n",
                headers, type_definitions, declarations
            );
            log::info!("Additional C++ decls:\n{}", declarations);
            let header_name = "autocxxgen.h".to_string();
            let implementation = if self
                .additional_functions
                .iter()
                .any(|x| x.definition.is_some())
            {
                let definitions = self.concat_additional_items(|x| x.definition.as_ref());
                let definitions = format!(
                    "#include \"{}\"\n{}\n{}",
                    header_name, cpp_headers, definitions
                );
                log::info!("Additional C++ defs:\n{}", definitions);
                Some(definitions.into_bytes())
            } else {
                None
            };
            Some(CppFilePair {
                header: declarations.into_bytes(),
                implementation,
                header_name,
            })
        }
    }

    fn collect_headers<F>(&self, filter: F) -> String
    where
        F: Fn(&ExtraCpp) -> &[Header],
    {
        let cpp_headers: HashSet<_> = self
            .additional_functions
            .iter()
            .flat_map(|x| filter(x).iter())
            .filter(|x| !self.cpp_codegen_options.suppress_system_headers || !x.is_system())
            .collect(); // uniqify
        cpp_headers
            .iter()
            .map(|x| x.include_stmt(self.cpp_codegen_options))
            .join("\n")
    }

    fn concat_additional_items<F>(&self, field_access: F) -> String
    where
        F: FnMut(&ExtraCpp) -> Option<&String>,
    {
        let mut s = self
            .additional_functions
            .iter()
            .flat_map(field_access)
            .join("\n");
        s.push('\n');
        s
    }
}
