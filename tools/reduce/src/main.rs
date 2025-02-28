// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#![forbid(unsafe_code)]

use std::{
    borrow::Cow,
    fs::File,
    io::Write,
    os::unix::prelude::PermissionsExt,
    path::{Path, PathBuf},
};

use autocxx_engine::{get_clang_path, make_clang_args, preprocess};
use autocxx_parser::IncludeCppConfig;
use clap::{crate_authors, crate_version, Arg, ArgMatches, Command};
use indexmap::IndexSet;
use indoc::indoc;
use itertools::Itertools;
use quote::ToTokens;
use regex::Regex;
use tempfile::TempDir;

static LONG_HELP: &str = indoc! {"
Command line utility to minimize autocxx bug cases.

This is a wrapper for creduce.

Example command-line:
autocxx-reduce file -I my-inc-dir -h my-header -d 'generate!(\"MyClass\")' -k -- --n 64
"};

fn main() {
    // Assemble some defaults for command line arguments
    let current_exe = std::env::current_exe().unwrap();
    let our_dir = current_exe.parent().unwrap();
    let default_gen_cmd = our_dir.join("autocxx-gen").to_str().unwrap().to_string();
    let rust_libs_path1 = our_dir.to_str().unwrap().to_string();
    let rust_libs_path2 = our_dir.join("deps").to_str().unwrap().to_string();
    let default_rlibs = &[rust_libs_path1.as_str(), rust_libs_path2.as_str()];
    let matches = Command::new("autocxx-reduce")
        .version(crate_version!())
        .author(crate_authors!())
        .about("Reduce a C++ test case")
        .long_about(LONG_HELP)
        .subcommand(Command::new("file")
                                      .about("reduce a header file")

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
                                        Arg::new("define")
                                            .short('D')
                                            .long("define")
                                            .multiple_occurrences(true)
                                            .number_of_values(1)
                                            .value_name("DEFINE")
                                            .help("macro definition")
                                            .takes_value(true),
                                    )
                                    .arg(
                                        Arg::new("header")
                                            .long("header")
                                            .multiple_occurrences(true)
                                            .number_of_values(1)
                                            .required(true)
                                            .value_name("HEADER")
                                            .help("header file name")
                                            .takes_value(true),
                                    )

                                .arg(
                                    Arg::new("directive")
                                        .short('d')
                                        .long("directive")
                                        .multiple_occurrences(true)
                                        .number_of_values(1)
                                        .value_name("DIRECTIVE")
                                        .help("directives to put within include_cpp!")
                                        .takes_value(true),
                                )
                            )
                            .subcommand(Command::new("repro")
                                                          .about("reduce a repro case JSON file")
                                            .arg(
                                                Arg::new("repro")
                                                    .short('r')
                                                    .long("repro")
                                                    .required(true)
                                                    .value_name("REPRODUCTION CASE JSON")
                                                    .help("reproduction case JSON file name")
                                                    .takes_value(true),
                                            )
                                            .arg(
                                        Arg::new("header")
                                            .long("header")
                                            .multiple_occurrences(true)
                                            .number_of_values(1)
                                            .value_name("HEADER")
                                            .help("header file name; specify to resume a part-completed run")
                                            .takes_value(true),
                                    )
                                        )
        .arg(
            Arg::new("problem")
                .short('p')
                .long("problem")
                .required(true)
                .value_name("PROBLEM")
                .help("problem string we're looking for... may be in logs, or in generated C++, or generated .rs")
                .takes_value(true),
        )
        .arg(
            Arg::new("creduce")
                .long("creduce")
                .value_name("PATH")
                .help("creduce binary location")
                .default_value("creduce")
                .takes_value(true),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("OUTPUT")
                .help("where to write minimized output")
                .takes_value(true),
        )
        .arg(
            Arg::new("gen-cmd")
                .short('g')
                .long("gen-cmd")
                .value_name("GEN-CMD")
                .help("where to find autocxx-gen")
                .default_value(&default_gen_cmd)
                .takes_value(true),
        )
        .arg(
            Arg::new("rustc")
                .long("rustc")
                .value_name("RUSTC")
                .help("where to find rustc")
                .default_value("rustc")
                .takes_value(true),
        )
        .arg(
            Arg::new("rlibs")
                .long("rlibs")
                .value_name("LIBDIR")
                .help("where to find rlibs/rmetas for cxx and autocxx")
                .default_values(default_rlibs)
                .multiple_values(true)
                .takes_value(true),
        )
        .arg(
            Arg::new("keep")
                .short('k')
                .long("keep-dir")
                .help("keep the temporary directory for debugging purposes"),
        )
        .arg(
            Arg::new("clang-args")
                .short('c')
                .long("clang-arg")
                .multiple_occurrences(true)
                .value_name("CLANG_ARG")
                .help("Extra arguments to pass to Clang"),
        )
        .arg(
            Arg::new("creduce-args")
                .long("creduce-arg")
                .multiple_occurrences(true)
                .value_name("CREDUCE_ARG")
                .help("Extra arguments to pass to Clang"),
        )
        .arg(
            Arg::new("no-precompile")
                .long("no-precompile")
                .help("Do not precompile the C++ header before passing to autocxxgen"),
        )
        .arg(
            Arg::new("no-postcompile")
                .long("no-postcompile")
                .help("Do not post-compile the C++ generated by autocxxgen"),
        )
        .arg(
            Arg::new("no-rustc")
                .long("no-rustc")
                .help("Do not compile the rust generated by autocxxgen"),
        )
        .arg(
            Arg::new("suppress-cxx-inclusions")
                .long("suppress-cxx-inclusions")
                .takes_value(true)
                .possible_value("yes")
                .possible_value("no")
                .possible_value("auto")
                .default_value("auto")
                .help("Whether the preprocessed header already includes cxx.h. If so, we'll try to suppress the natural behavior of cxx to include duplicate definitions of some of the types within gen0.cc.")
        )
        .arg_required_else_help(true)
        .get_matches();
    run(matches).unwrap();
}

fn run(matches: ArgMatches) -> Result<(), std::io::Error> {
    let keep_tmp = matches.is_present("keep");
    let tmp_dir = TempDir::new()?;
    let r = do_run(matches, &tmp_dir);
    if keep_tmp {
        println!(
            "Keeping temp dir created at: {}",
            tmp_dir.into_path().to_str().unwrap()
        );
    }
    r
}

#[derive(serde_derive::Deserialize)]
struct ReproCase {
    config: String,
    header: String,
}

fn do_run(matches: ArgMatches, tmp_dir: &TempDir) -> Result<(), std::io::Error> {
    let rs_path = tmp_dir.path().join("input.rs");
    let concat_path = tmp_dir.path().join("concat.h");
    match matches.subcommand_matches("repro") {
        None => {
            let submatches = matches.subcommand_matches("file").unwrap();
            let incs: Vec<_> = submatches
                .values_of("inc")
                .unwrap_or_default()
                .map(PathBuf::from)
                .collect();
            let defs: Vec<_> = submatches.values_of("define").unwrap_or_default().collect();
            let headers: Vec<_> = submatches.values_of("header").unwrap_or_default().collect();
            assert!(!headers.is_empty());
            let listing_path = tmp_dir.path().join("listing.h");
            create_concatenated_header(&headers, &listing_path)?;
            announce_progress(&format!(
                "Preprocessing {listing_path:?} to {concat_path:?}"
            ));
            preprocess(&listing_path, &concat_path, &incs, &defs)?;
            let directives: Vec<_> = std::iter::once("#include \"concat.h\"\n".to_string())
                .chain(
                    submatches
                        .values_of("directive")
                        .unwrap_or_default()
                        .map(|s| format!("{s}\n")),
                )
                .collect();
            create_rs_file(&rs_path, &directives)?;
        }
        Some(submatches) => {
            let case: ReproCase = serde_json::from_reader(File::open(PathBuf::from(
                submatches.value_of("repro").unwrap(),
            ))?)
            .unwrap();
            // Replace the headers in the config
            let mut config: IncludeCppConfig = syn::parse_str(&case.config).unwrap();
            config.replace_included_headers("concat.h");
            create_file(
                &rs_path,
                &format!("autocxx::include_cpp!({});", config.to_token_stream()),
            )?;
            if let Some(header) = submatches.value_of("header") {
                std::fs::copy(PathBuf::from(header), &concat_path)?;
            } else {
                create_file(&concat_path, &case.header)?
            }
        }
    }

    let suppress_cxx_classes = match matches.value_of("suppress-cxx-inclusions").unwrap() {
        "yes" => true,
        "no" => false,
        "auto" => detect_cxx_h(&concat_path)?,
        _ => panic!("unexpected value"),
    };

    let cxx_suppressions = if suppress_cxx_classes {
        get_cxx_suppressions()
    } else {
        Vec::new()
    };

    let extra_clang_args: Vec<_> = matches
        .values_of("clang-args")
        .unwrap_or_default()
        .map(Cow::Borrowed)
        .chain(cxx_suppressions.into_iter().map(Cow::Owned))
        .collect();
    let extra_clang_args: Vec<&str> = extra_clang_args.iter().map(|s| s.as_ref()).collect_vec();

    let gen_cmd = matches.value_of("gen-cmd").unwrap();
    if !Path::new(gen_cmd).exists() {
        panic!(
            "autocxx-gen not found in {gen_cmd}. hint: autocxx-reduce --gen-cmd /path/to/autocxx-gen"
        );
    }

    run_sample_gen_cmd(gen_cmd, &rs_path, tmp_dir.path(), &extra_clang_args)?;
    // Create and run an interestingness test which does not filter its output through grep.
    let demo_interestingness_test_dir = tmp_dir.path().join("demo-interestingness-test");
    std::fs::create_dir(&demo_interestingness_test_dir).unwrap();
    let interestingness_test = demo_interestingness_test_dir.join("test-demo.sh");
    create_interestingness_test(
        &matches,
        gen_cmd,
        &interestingness_test,
        None,
        &rs_path,
        &extra_clang_args,
    )?;
    let demo_dir_concat_path = demo_interestingness_test_dir.join("concat.h");
    std::fs::copy(&concat_path, demo_dir_concat_path).unwrap();
    run_demo_interestingness_test(&demo_interestingness_test_dir, &interestingness_test).unwrap();

    // Now the main interestingness test
    let interestingness_test = tmp_dir.path().join("test.sh");
    create_interestingness_test(
        &matches,
        gen_cmd,
        &interestingness_test,
        Some(matches.value_of("problem").unwrap()),
        &rs_path,
        &extra_clang_args,
    )?;
    run_creduce(
        matches.value_of("creduce").unwrap(),
        &interestingness_test,
        &concat_path,
        matches.values_of("creduce-args").unwrap_or_default(),
    );
    announce_progress("creduce completed");
    let output_path = matches.value_of("output");
    match output_path {
        None => print_minimized_case(&concat_path)?,
        Some(output_path) => {
            std::fs::copy(&concat_path, PathBuf::from(output_path))?;
        }
    };
    Ok(())
}

/// Try to detect whether the preprocessed source code already contains
/// a preprocessed version of cxx.h. This is hard because all the comments
/// and preprocessor symbols may have been removed, and in fact if we're
/// part way through reduction, parts of the code may have been removed too.
fn detect_cxx_h(concat_path: &Path) -> Result<bool, std::io::Error> {
    let haystack = std::fs::read_to_string(concat_path)?;
    Ok(["class Box", "class Vec", "class Slice"]
        .iter()
        .all(|needle| haystack.contains(needle)))
}

fn announce_progress(msg: &str) {
    println!("=== {msg} ===");
}

fn print_minimized_case(concat_path: &Path) -> Result<(), std::io::Error> {
    announce_progress("Completed. Minimized test case:");
    let contents = std::fs::read_to_string(concat_path)?;
    println!("{contents}");
    Ok(())
}

/// Arguments we pass to creduce if supported. This pass always seems to cause a crash
/// as far as I can tell, so always exclude it. It may be environment-dependent,
/// of course, but as I'm the primary user of this tool I am ruthlessly removing it.
const REMOVE_PASS_LINE_MARKERS: &[&str] = &["--remove-pass", "pass_line_markers", "*"];
const SKIP_INITIAL_PASSES: &[&str] = &["--skip-initial-passes"];

fn creduce_supports_remove_pass(creduce_cmd: &str) -> bool {
    let cmd = std::process::Command::new(creduce_cmd)
        .arg("--help")
        .output();
    let msg = match cmd {
        Err(error) => panic!("failed to run creduce. creduce_cmd = {creduce_cmd}. hint: autocxx-reduce --creduce /path/to/creduce. error = {error}"),
        Ok(result) => result.stdout
    };
    let msg = std::str::from_utf8(&msg).unwrap();
    msg.contains("--remove-pass")
}

fn run_creduce<'a>(
    creduce_cmd: &str,
    interestingness_test: &'a Path,
    concat_path: &'a Path,
    creduce_args: impl Iterator<Item = &'a str>,
) {
    announce_progress("creduce");
    let args = std::iter::once(interestingness_test.to_str().unwrap())
        .chain(std::iter::once(concat_path.to_str().unwrap()))
        .chain(creduce_args)
        .chain(
            if creduce_supports_remove_pass(creduce_cmd) {
                REMOVE_PASS_LINE_MARKERS
            } else {
                SKIP_INITIAL_PASSES
            }
            .iter()
            .copied(),
        )
        .collect::<Vec<_>>();
    println!("Command: {} {}", creduce_cmd, args.join(" "));
    std::process::Command::new(creduce_cmd)
        .args(args)
        .status()
        .expect("failed to creduce");
}

fn run_sample_gen_cmd(
    gen_cmd: &str,
    rs_file: &Path,
    tmp_dir: &Path,
    extra_clang_args: &[&str],
) -> Result<(), std::io::Error> {
    let args = format_gen_cmd(rs_file, tmp_dir.to_str().unwrap(), extra_clang_args);
    let args = args.collect::<Vec<_>>();
    let args_str = args.join(" ");
    announce_progress(&format!("Running sample gen cmd: {gen_cmd} {args_str}"));
    std::process::Command::new(gen_cmd).args(args).status()?;
    Ok(())
}

fn run_demo_interestingness_test(demo_dir: &Path, test: &Path) -> Result<(), std::io::Error> {
    announce_progress(&format!(
        "Running demo interestingness test in {}",
        demo_dir.to_string_lossy()
    ));
    std::process::Command::new(test)
        .current_dir(demo_dir)
        .status()?;
    Ok(())
}

fn format_gen_cmd<'a>(
    rs_file: &Path,
    dir: &str,
    extra_clang_args: &'a [&str],
) -> impl Iterator<Item = String> + 'a {
    let args = [
        "-o".to_string(),
        dir.to_string(),
        "-I".to_string(),
        dir.to_string(),
        rs_file.to_str().unwrap().to_string(),
        "--gen-rs-include".to_string(),
        "--gen-cpp".to_string(),
        "--suppress-system-headers".to_string(),
        "--".to_string(),
    ]
    .to_vec();
    args.into_iter()
        .chain(extra_clang_args.iter().map(|s| s.to_string()))
}

fn create_interestingness_test(
    matches: &ArgMatches,
    gen_cmd: &str,
    test_path: &Path,
    problem: Option<&str>,
    rs_file: &Path,
    extra_clang_args: &[&str],
) -> Result<(), std::io::Error> {
    announce_progress("Creating interestingness test");
    let precompile = !matches.is_present("no-precompile");
    let postcompile = !matches.is_present("no-postcompile");
    let rustc = !matches.is_present("no-rustc");

    let rustc_path = matches.value_of("rustc").unwrap();

    let rust_libs_path: Vec<String> = matches
        .get_many::<String>("rlibs")
        .expect("No rlib path specified")
        .cloned()
        .collect();

    // Ensure we refer to the input header by relative path
    // because creduce will invoke us in some other directory with
    // a copy thereof.
    let mut args = format_gen_cmd(rs_file, "$(pwd)", extra_clang_args);
    let args = args.join(" ");
    let precompile_step = make_compile_step(precompile, "concat.h", extra_clang_args);
    // For the compile afterwards, we have to avoid including any system headers.
    // We rely on equivalent content being hermetically inside concat.h.
    let postcompile_step = make_compile_step(postcompile, "gen0.cc", extra_clang_args);
    let rustc_step = if rustc {
        let rust_libs_path = rust_libs_path.iter().map(|p| format!(" -L{p}")).join(" ");
        format!("{rustc_path} --extern cxx --extern autocxx {rust_libs_path} --crate-type rlib --emit=metadata --edition=2021 autocxx-ffi-default-gen.rs 2>&1")
    } else {
        "echo Skipping rustc".to_string()
    };
    // -q below to exit immediately as soon as a match is found, to avoid
    // extra compile/codegen steps
    let problem_grep = problem
        .map(|problem| format!("| grep -q \"{problem}\"  >/dev/null  2>&1"))
        .unwrap_or_default();
    // We formerly had a 'trap' below but it seems to have caused problems
    // (trap \"if [[ \\$? -eq 139 ]]; then echo Segfault; fi\" CHLD; {} {} 2>&1 && cat autocxx-ffi-default-gen.rs && cat autocxxgen*.h && {} && {} 2>&1 ) {}
    let content = format!(
        indoc! {"
        #!/bin/bash
        set -e
        echo Precompile
        {}
        echo Move
        mv concat.h concat-body.h
        (echo \"#ifndef __CONCAT_H__\"; echo \"#define __CONCAT_H__\"; echo '#include \"concat-body.h\"'; echo \"#endif\") > concat.h
        echo Codegen
        ({} {} 2>&1 && cat autocxx-ffi-default-gen.rs && cat autocxxgen*.h && {} && {} 2>&1) {}
        echo Remove
        rm concat.h
        echo Swap back
        mv concat-body.h concat.h
        echo Done
    "},
        precompile_step, gen_cmd, args, rustc_step, postcompile_step, problem_grep
    );
    println!("Interestingness test:\n{content}");
    {
        let mut file = File::create(test_path)?;
        file.write_all(content.as_bytes())?;
    }

    let mut perms = std::fs::metadata(test_path)?.permissions();
    perms.set_mode(0o700);
    std::fs::set_permissions(test_path, perms)?;
    Ok(())
}

fn make_compile_step(enabled: bool, file: &str, extra_clang_args: &[&str]) -> String {
    if enabled {
        format!(
            "{} {} -c {}",
            get_clang_path(),
            make_clang_args(&[PathBuf::from(".")], extra_clang_args).join(" "),
            file,
        )
    } else {
        "echo 'Skipping compilation'".into()
    }
}

fn create_rs_file(rs_path: &Path, directives: &[String]) -> Result<(), std::io::Error> {
    announce_progress("Creating Rust input file");
    let mut file = File::create(rs_path)?;
    file.write_all("use autocxx::include_cpp;\ninclude_cpp! (\n".as_bytes())?;
    for directive in directives {
        file.write_all(directive.as_bytes())?;
    }
    file.write_all(");\n".as_bytes())?;
    Ok(())
}

fn create_concatenated_header(headers: &[&str], listing_path: &Path) -> Result<(), std::io::Error> {
    announce_progress("Creating preprocessed header");
    let mut file = File::create(listing_path)?;
    for header in headers {
        file.write_all(format!("#include \"{header}\"\n").as_bytes())?;
    }
    Ok(())
}

fn create_file(path: &Path, content: &str) -> Result<(), std::io::Error> {
    let mut file = File::create(path)?;
    write!(file, "{content}")?;
    Ok(())
}

fn get_cxx_suppressions() -> Vec<String> {
    let defines: IndexSet<_> = Regex::new(r"\bCXXBRIDGE1_\w+\b")
        .unwrap()
        .find_iter(cxx_gen::HEADER)
        .map(|m| m.as_str())
        .collect(); // for uniqueness
    defines.into_iter().map(|def| format!("-D{def}")).collect()
}

#[test]
fn test_get_cxx_suppressions() {
    let defines = get_cxx_suppressions();
    assert!(defines.contains(&"-DCXXBRIDGE1_RUST_BITCOPY_T".to_string()));
    assert!(defines.contains(&"-DCXXBRIDGE1_RUST_STR".to_string()));
}
