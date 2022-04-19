// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#![forbid(unsafe_code)]

mod depfile;
mod output;
mod output_regular;
mod output_single;

use autocxx_engine::{parse_file, HeaderNamer, RebuildDependencyRecorder};
use clap::{crate_authors, crate_version, App, Arg, ArgGroup};
use depfile::Depfile;
use miette::IntoDiagnostic;
use output::Output;
use output_regular::RegularOutput;
use output_single::SingleFileOutput;
use proc_macro2::TokenStream;
use quote::ToTokens;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

static LONG_HELP: &str = "
Command line utility to expand the Rust 'autocxx' include_cpp! directive.

This tool can generate both the C++ and Rust side binding code for
a Rust file containing an include_cpp! directive.

If you're using cargo, don't use this: use autocxx_build instead,
which is much easier to include in build.rs build scripts. You'd likely
use this tool only if you're using some non-Cargo build system. If
that's you, read on.

This tool has three modes: generate the C++; generate a new Rust file where
the include_cpp! directive is *replaced* with bindings, or generate
a Rust file which can be included by the autocxx_macro. You may specify
multiple modes, or of course, invoke the tool multiple times.

In any mode, you'll need to pass the source Rust file name and the C++
include path.

For generation of the Rust side bindings, here's how to choose between
the two modes. If you're copying the entire Rust crate to a different
location during your build process, you may as well use --gen-rs-complete
to generate a whole new replacement .rs file with the autocxx
include_cpp! macro expanded.

But in most build systems, you won't be copying all the crate source
to a new location. In such a case, you should use --gen-rs-include
which will generate a file that will be included by the autocxx_macro
crate.

The second decision you must make is naming of the output files.
If your build system is able to cope with autocxx_gen building
unpredictable filenames, then:
a) set AUTOCXX_RS when using autocxx_macro
b) build all *.cc files produced by this tool.

If your build system requires each build rule to make precise filenames
known in advance, then you will need to use --single, in which case output
filenames are completely deterministic.
";

fn main() -> miette::Result<()> {
    let matches = App::new("autocxx-gen")
        .version(crate_version!())
        .author(crate_authors!())
        .about("Generates bindings files from Rust files that contain include_cpp! macros")
        .long_about(LONG_HELP)
        .arg(
            Arg::with_name("INPUT")
                .help("Sets the input .rs file to use")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::with_name("outdir")
                .short("o")
                .long("outdir")
                .value_name("PATH")
                .help("output directory path")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name("inc")
                .short("I")
                .long("inc")
                .multiple(true)
                .number_of_values(1)
                .value_name("INCLUDE DIRS")
                .help("include path")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("cpp-extension")
                .long("cpp-extension")
                .value_name("EXTENSION")
                .default_value("cc")
                .help("C++ filename extension")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("gen-cpp")
                .long("gen-cpp")
                .help("whether to generate C++ implementation and header files")
        )
        .arg(
            Arg::with_name("gen-rs-complete")
                .long("gen-rs-complete")
                .help("whether to generate a Rust file replacing the original file (suffix will be .complete.rs)")
        )
        .arg(
            Arg::with_name("gen-rs-include")
                .long("gen-rs-include")
                .help("whether to generate Rust files for inclusion using autocxx_macro (suffix will be .include.rs)")
        )
        .group(ArgGroup::with_name("mode")
            .required(true)
            .multiple(true)
            .arg("gen-cpp")
            .arg("gen-rs-complete")
            .arg("gen-rs-include")
        )
        .arg(
            Arg::with_name("single")
                .long("single")
                .help("Do not generate multiple files of each type. Generate a single .cc, and/or single .h and/or a single .rs.json file for all the inputs")
        )
        .arg(
            Arg::with_name("auto-allowlist")
                .long("auto-allowlist")
                .help("Dynamically construct allowlist from real uses of APIs.")
        )
        .arg(
            Arg::with_name("suppress-system-headers")
                .long("suppress-system-headers")
                .help("Do not refer to any system headers from generated code. May be useful for minimization.")
        )
        .arg(
            Arg::with_name("cxx-impl-annotations")
                .long("cxx-impl-annotations")
                .value_name("ANNOTATION")
                .help("prefix for symbols to be exported from C++ bindings, e.g. __attribute__ ((visibility (\"default\")))")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("cxx-h-path")
                .long("cxx-h-path")
                .value_name("PREFIX")
                .help("prefix for path to cxx.h (from the cxx crate) within #include statements. Must end in /")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("cxxgen-h-path")
                .long("cxxgen-h-path")
                .value_name("PREFIX")
                .help("prefix for path to cxxgen.h (which we generate into the output directory) within #include statements. Must end in /")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("depfile")
                .long("depfile")
                .value_name("DEPFILE")
                .help("A .d file to write")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("clang-args")
                .last(true)
                .multiple(true)
                .help("Extra arguments to pass to Clang"),
        )
        .get_matches();

    env_logger::builder().init();
    let mut parsed_file = parse_file(
        matches.value_of("INPUT").unwrap(),
        matches.is_present("auto-allowlist"),
    )?;
    let incs = matches
        .values_of("inc")
        .unwrap_or_default()
        .map(PathBuf::from)
        .collect();
    let extra_clang_args: Vec<_> = matches
        .values_of("clang-args")
        .unwrap_or_default()
        .collect();
    let suppress_system_headers = matches.is_present("suppress-system-headers");
    let single_file_mode = matches.is_present("single");
    let header_namer = if single_file_mode {
        HeaderNamer(Box::new(|_| "autocxx_gen.h".to_string()))
    } else {
        Default::default()
    };
    let cpp_codegen_options = autocxx_engine::CppCodegenOptions {
        suppress_system_headers,
        cxx_impl_annotations: get_option_string("cxx-impl-annotations", &matches),
        path_to_cxx_h: get_option_string("cxx-h-path", &matches),
        path_to_cxxgen_h: get_option_string("cxxgen-h-path", &matches),
        header_namer,
    };
    let depfile = match matches.value_of("depfile") {
        None => None,
        Some(depfile_path) => {
            let depfile_path = PathBuf::from(depfile_path);
            Some(Rc::new(RefCell::new(
                Depfile::new(&depfile_path).into_diagnostic()?,
            )))
        }
    };
    let dep_recorder: Option<Box<dyn RebuildDependencyRecorder>> = depfile
        .as_ref()
        .map(|rc| get_dependency_recorder(rc.clone()));
    parsed_file.resolve_all(incs, &extra_clang_args, dep_recorder, &cpp_codegen_options)?;

    let outdir: PathBuf = matches.value_of_os("outdir").unwrap().into();
    let mut output: Box<dyn Output> = if single_file_mode {
        Box::new(SingleFileOutput::new(depfile.clone(), &outdir))
    } else {
        Box::new(RegularOutput::new(depfile.clone(), &outdir))
    };
    if matches.is_present("gen-cpp") {
        let cpp = matches.value_of("cpp-extension").unwrap();
        let mut counter = 0usize;
        for include_cxx in parsed_file.get_cpp_buildables() {
            let generations = include_cxx
                .generate_h_and_cxx(&cpp_codegen_options)
                .expect("Unable to generate header and C++ code");
            for pair in generations.0 {
                let cppname = format!("gen{}.{}", counter, cpp);
                output.write_cpp(cppname, &pair.implementation.unwrap_or_default());
                output.write_cpp(pair.header_name, &pair.header);
                counter += 1;
            }
        }
    }
    drop(cpp_codegen_options);
    if matches.is_present("gen-rs-complete") {
        let mut ts = TokenStream::new();
        parsed_file.to_tokens(&mut ts);
        output.write_rs("gen.complete.rs".to_string(), ts.to_string().as_bytes());
    }
    if matches.is_present("gen-rs-include") {
        let autocxxes = parsed_file.get_rs_buildables();
        for include_cxx in autocxxes {
            let ts = include_cxx.generate_rs();
            let fname = include_cxx.get_rs_filename();
            output.write_rs(fname, ts.to_string().as_bytes());
        }
    }
    output.finalize();
    if let Some(depfile) = depfile {
        depfile.borrow_mut().write().into_diagnostic()?;
    }
    Ok(())
}

fn get_dependency_recorder(depfile: Rc<RefCell<Depfile>>) -> Box<dyn RebuildDependencyRecorder> {
    Box::new(RecordIntoDepfile(depfile))
}

fn get_option_string(option: &str, matches: &clap::ArgMatches) -> Option<String> {
    let cxx_impl_annotations = matches.value_of(option).map(|s| s.to_string());
    cxx_impl_annotations
}

struct RecordIntoDepfile(Rc<RefCell<Depfile>>);

impl RebuildDependencyRecorder for RecordIntoDepfile {
    fn record_header_file_dependency(&self, filename: &str) {
        self.0.borrow_mut().add_dependency(&PathBuf::from(filename))
    }
}

impl std::fmt::Debug for RecordIntoDepfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<depfile>")
    }
}
