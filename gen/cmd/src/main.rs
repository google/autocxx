// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#![forbid(unsafe_code)]

mod depfile;

use autocxx_engine::{
    generate_cpp, generate_rs_archive, generate_rs_single, parse_file, RebuildDependencyRecorder,
};
use clap::{crate_authors, crate_version, Arg, ArgGroup, Command};
use depfile::Depfile;
use miette::IntoDiagnostic;
use std::cell::RefCell;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::rc::Rc;
use std::{fs::File, path::Path};

pub(crate) static BLANK: &str = "// Blank autocxx placeholder";

static LONG_HELP: &str = "
Command line utility to expand the Rust 'autocxx' include_cpp! directive.

This tool can generate both the C++ and Rust side binding code for
a Rust file containing an include_cpp! directive.

If you're using cargo, don't use this: use autocxx_build instead,
which is much easier to include in build.rs build scripts. You'd likely
use this tool only if you're using some non-Cargo build system. If
that's you, read on.

This tool has three modes: generate the C++; or generate
a Rust file which can be included by the autocxx_macro; or generate an archive
containing multiple Rust files to be expanded by different autocxx macros.
You may specify multiple modes, or of course, invoke the tool multiple times.

In any mode, you'll need to pass the source Rust file name and the C++
include path. You may pass multiple Rust files, each of which may contain
multiple include_cpp! or cxx::bridge macros.

There are three basic ways to use this tool, depending on the flexibility
of your build system.

Does your build system require fixed output filenames, or can it enumerate
whatever files are generated?

If it's flexible, then use
  --gen-rs-include --gen-cpp
Either one or two .h files will be generated; either one or two .cc files
will be generated, and an arbitrary number of .rs files will be generated
(one for each input macro across all the input .rs files). When building
the rust code, simply ensure that AUTOCXX_RS or OUT_DIR is set to teach
rustc where to find these .rs files.

If your build system needs to be told exactly what C++ files are generated,
additionally use --generate-exactly-two. You are then guaranteed to get
two .h files and two .cc files, named as follows:
  cxxgen.h
  autocxxgen.h
  gen0.cc
  gen1.cc

Some of them may sometimes be blank, but that's OK, that's what your build
system requires.

If your build system additionally requires that Rust files have fixed
filenames, then you should use
  --gen-rs-archive
instead of
  --gen-rs-include
and you will need to give AUTOCXX_RS_JSON_ARCHIVE when building the Rust code.
The output filename is named gen.rs.json.

This teaches rustc (and the autocxx macro) that all the different Rust bindings
for multiple different autocxx macros have been archived into this single file.
";

fn main() -> miette::Result<()> {
    let matches = Command::new("autocxx-gen")
        .version(crate_version!())
        .author(crate_authors!())
        .about("Generates bindings files from Rust files that contain include_cpp! macros")
        .long_about(LONG_HELP)
        .arg(
            Arg::new("INPUT")
                .help("Sets the input .rs files to use")
                .required(true)
                .multiple_occurrences(true)
        )
        .arg(
            Arg::new("outdir")
                .short('o')
                .long("outdir")
                .allow_invalid_utf8(true)
                .value_name("PATH")
                .help("output directory path")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::new("inc")
                .short('I')
                .long("inc")
                .multiple_occurrences(true)
                .number_of_values(1)
                .value_name("INCLUDE DIRS")
                .help("include path")
                .takes_value(true),
        )
        .arg(
            Arg::new("cpp-extension")
                .long("cpp-extension")
                .value_name("EXTENSION")
                .default_value("cc")
                .help("C++ filename extension")
                .takes_value(true),
        )
        .arg(
            Arg::new("gen-cpp")
                .long("gen-cpp")
                .help("whether to generate C++ implementation and header files")
        )
        .arg(
            Arg::new("gen-rs-include")
                .long("gen-rs-include")
                .help("whether to generate Rust files for inclusion using autocxx_macro (suffix will be .include.rs)")
        )
        .arg(
            Arg::new("gen-rs-archive")
                .long("gen-rs-archive")
                .help("whether to generate an archive of multiple sets of Rust bindings for use by autocxx_macro (suffix will be .rs.json)")
        )
        .group(ArgGroup::new("mode")
            .required(true)
            .multiple(true)
            .arg("gen-cpp")
            .arg("gen-rs-include")
            .arg("gen-rs-archive")
        )
        .arg(
            Arg::new("generate-exactly-two")
                .long("generate-exactly-two")
                .help("generate exactly two .h files (called cxxgen.h and autocxxgen.h) and two .cc files (called gen0.cc and gen1.cc) even if we don't need to generate all this. This helps with build systems that require a predictable number of files.")
        )
        .arg(
            Arg::new("fix-rs-include-name")
                .long("fix-rs-include-name")
                .help("Make the name of the .rs file predictable. You must set AUTOCXX_RS_FILE during Rust build time to educate autocxx_macro about your choice.")
                .requires("gen-rs-include")
        )
        .arg(
            Arg::new("auto-allowlist")
                .long("auto-allowlist")
                .help("Dynamically construct allowlist from real uses of APIs.")
        )
        .arg(
            Arg::new("suppress-system-headers")
                .long("suppress-system-headers")
                .help("Do not refer to any system headers from generated code. May be useful for minimization.")
        )
        .arg(
            Arg::new("cxx-impl-annotations")
                .long("cxx-impl-annotations")
                .value_name("ANNOTATION")
                .help("prefix for symbols to be exported from C++ bindings, e.g. __attribute__ ((visibility (\"default\")))")
                .takes_value(true),
        )
        .arg(
            Arg::new("cxx-h-path")
                .long("cxx-h-path")
                .value_name("PREFIX")
                .help("prefix for path to cxx.h (from the cxx crate) within #include statements. Must end in /")
                .takes_value(true),
        )
        .arg(
            Arg::new("cxxgen-h-path")
                .long("cxxgen-h-path")
                .value_name("PREFIX")
                .help("prefix for path to cxxgen.h (which we generate into the output directory) within #include statements. Must end in /")
                .takes_value(true),
        )
        .arg(
            Arg::new("depfile")
                .long("depfile")
                .value_name("DEPFILE")
                .help("A .d file to write")
                .takes_value(true),
        )
        .arg(
            Arg::new("clang-args")
                .last(true)
                .multiple_occurrences(true)
                .help("Extra arguments to pass to Clang"),
        )
        .get_matches();

    env_logger::builder().init();
    let incs = matches
        .values_of("inc")
        .unwrap_or_default()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let extra_clang_args: Vec<_> = matches
        .values_of("clang-args")
        .unwrap_or_default()
        .collect();
    let suppress_system_headers = matches.is_present("suppress-system-headers");
    let cpp_codegen_options = autocxx_engine::CppCodegenOptions {
        suppress_system_headers,
        cxx_impl_annotations: get_option_string("cxx-impl-annotations", &matches),
        path_to_cxx_h: get_option_string("cxx-h-path", &matches),
        path_to_cxxgen_h: get_option_string("cxxgen-h-path", &matches),
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
    let auto_allowlist = matches.is_present("auto-allowlist");
    let generate_exactly_two = matches.is_present("generate-exactly-two");

    let mut parsed_files = Vec::new();
    for input in matches.values_of("INPUT").expect("No INPUT was provided") {
        // Parse all the .rs files we're asked to process, first.
        // Spot any fundamental parsing or command line problems before we start
        // to do the complex processing.
        let parsed_file = parse_file(input, auto_allowlist)?;
        parsed_files.push(parsed_file);
    }

    for parsed_file in parsed_files.iter_mut() {
        // Now actually handle all the include_cpp directives we found,
        // which is the complex bit where we interpret all the C+.
        let dep_recorder: Option<Box<dyn RebuildDependencyRecorder>> = depfile
            .as_ref()
            .map(|rc| get_dependency_recorder(rc.clone()));
        parsed_file.resolve_all(incs.clone(), &extra_clang_args, dep_recorder)?;
    }

    // Finally start to write the C++ and Rust out.
    let outdir: PathBuf = matches.value_of_os("outdir").unwrap().into();
    if matches.is_present("gen-cpp") {
        let cpp = matches.value_of("cpp-extension").unwrap();
        let cpp_buildables = parsed_files
            .iter()
            .flat_map(|parsed_file| parsed_file.get_cpp_outputs());
        let generated_cpp = generate_cpp(cpp_buildables, &cpp_codegen_options).into_diagnostic()?;
        write_to_file(
            &depfile,
            &outdir,
            format!("gen0.{}", cpp),
            &generated_cpp.first.implementation.unwrap_or_default(),
        );
        write_to_file(
            &depfile,
            &outdir,
            generated_cpp.first.header_name,
            &generated_cpp.first.header,
        );
        if let Some(second) = generated_cpp.second {
            write_to_file(
                &depfile,
                &outdir,
                format!("gen1.{}", cpp),
                &second.implementation.unwrap_or_default(),
            );
            write_to_file(&depfile, &outdir, second.header_name, &second.header);
        } else if generate_exactly_two {
            write_placeholder(&depfile, &outdir, 1, cpp);
            write_placeholder(&depfile, &outdir, 1, "h");
        }
    }
    if matches.is_present("gen-rs-include") {
        let rust_buildables = parsed_files
            .iter()
            .flat_map(|parsed_file| parsed_file.get_rs_outputs());
        for (counter, include_cxx) in rust_buildables.enumerate() {
            let rs_code = generate_rs_single(include_cxx);
            let fname = if matches.is_present("fix-rs-include-name") {
                format!("gen{}.include.rs", counter)
            } else {
                rs_code.filename
            };
            write_to_file(&depfile, &outdir, fname, rs_code.code.as_bytes());
        }
    }
    if matches.is_present("gen-rs-archive") {
        let rust_buildables = parsed_files
            .iter()
            .flat_map(|parsed_file| parsed_file.get_rs_outputs());
        let json = generate_rs_archive(rust_buildables);
        eprintln!("Writing to gen.rs.json in {:?}", outdir);
        write_to_file(&depfile, &outdir, "gen.rs.json".into(), json.as_bytes());
    }
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

fn write_placeholder(
    depfile: &Option<Rc<RefCell<Depfile>>>,
    outdir: &Path,
    counter: usize,
    extension: &str,
) {
    let fname = format!("gen{}.{}", counter, extension);
    write_to_file(depfile, outdir, fname, BLANK.as_bytes());
}

fn write_to_file(
    depfile: &Option<Rc<RefCell<Depfile>>>,
    dir: &Path,
    filename: String,
    content: &[u8],
) {
    let path = dir.join(filename);
    if let Some(depfile) = depfile {
        depfile.borrow_mut().add_output(&path);
    }
    {
        let f = File::open(&path);
        if let Ok(mut f) = f {
            let mut existing_content = Vec::new();
            let r = f.read_to_end(&mut existing_content);
            if r.is_ok() && existing_content == content {
                eprintln!("bailing");
                return; // don't change timestamp on existing file unnecessarily
            }
        }
    }
    let mut f = File::create(&path).expect("Unable to create file");
    f.write_all(content).expect("Unable to write file");
    eprintln!("written to {:?}", path);
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
