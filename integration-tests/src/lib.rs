// Copyright 2022 Google LLC
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

use std::{
    ffi::OsStr,
    fs::File,
    io::{Read, Write},
    panic::RefUnwindSafe,
    path::{Path, PathBuf},
    sync::Mutex,
};

use autocxx_engine::{Builder, BuilderContext, BuilderError, RebuildDependencyRecorder, HEADER};
use log::info;
use once_cell::sync::OnceCell;
use proc_macro2::{Span, TokenStream};
use quote::{quote, TokenStreamExt};
use syn::Token;
use tempfile::{tempdir, TempDir};

const KEEP_TEMPDIRS: bool = false;

/// API to run a documentation test. Panics if the test fails.
/// Guarantees not to emit anything to stdout and so can be run in an mdbook context.
pub fn doctest(
    cxx_code: &str,
    header_code: &str,
    rust_code: TokenStream,
    manifest_dir: &OsStr,
) -> Result<(), TestError> {
    let stdout_gag = gag::BufferRedirect::stdout().unwrap();
    std::env::set_var("CARGO_PKG_NAME", "autocxx-integration-tests");
    std::env::set_var("CARGO_MANIFEST_DIR", manifest_dir);
    let r = do_run_test_manual(cxx_code, header_code, rust_code, None, None);
    let mut stdout_str = String::new();
    stdout_gag
        .into_inner()
        .read_to_string(&mut stdout_str)
        .unwrap();
    if !stdout_str.is_empty() {
        eprintln!("Stdout from test:\n{}", stdout_str);
    }
    r
}

fn get_builder() -> &'static Mutex<LinkableTryBuilder> {
    static INSTANCE: OnceCell<Mutex<LinkableTryBuilder>> = OnceCell::new();
    INSTANCE.get_or_init(|| Mutex::new(LinkableTryBuilder::new()))
}

/// TryBuild which maintains a directory of libraries to link.
/// This is desirable because otherwise, if we alter the RUSTFLAGS
/// then trybuild rebuilds *everything* including all the dev-dependencies.
/// This object exists purely so that we use the same RUSTFLAGS for every
/// test case.
struct LinkableTryBuilder {
    /// Directory in which we'll keep any linkable libraries
    temp_dir: TempDir,
}

impl LinkableTryBuilder {
    fn new() -> Self {
        LinkableTryBuilder {
            temp_dir: tempdir().unwrap(),
        }
    }

    fn move_items_into_temp_dir<P1: AsRef<Path>>(&self, src_path: &P1, pattern: &str) {
        for item in std::fs::read_dir(src_path).unwrap() {
            let item = item.unwrap();
            if item.file_name().into_string().unwrap().contains(pattern) {
                let dest = self.temp_dir.path().join(item.file_name());
                if dest.exists() {
                    std::fs::remove_file(&dest).unwrap();
                }
                if KEEP_TEMPDIRS {
                    std::fs::copy(item.path(), dest).unwrap();
                } else {
                    std::fs::rename(item.path(), dest).unwrap();
                }
            }
        }
    }

    fn build<P1: AsRef<Path>, P2: AsRef<Path>, P3: AsRef<Path> + RefUnwindSafe>(
        &self,
        library_path: &P1,
        library_name: &str,
        header_path: &P2,
        header_names: &[&str],
        rs_path: &P3,
        generated_rs_files: Vec<PathBuf>,
    ) -> std::thread::Result<()> {
        // Copy all items from the source dir into our temporary dir if their name matches
        // the pattern given in `library_name`.
        self.move_items_into_temp_dir(library_path, library_name);
        for header_name in header_names {
            self.move_items_into_temp_dir(header_path, header_name);
        }
        for generated_rs in generated_rs_files {
            self.move_items_into_temp_dir(
                &generated_rs.parent().unwrap(),
                generated_rs.file_name().unwrap().to_str().unwrap(),
            );
        }
        let temp_path = self.temp_dir.path().to_str().unwrap();
        std::env::set_var("RUSTFLAGS", format!("-L {}", temp_path));
        std::env::set_var("AUTOCXX_RS", temp_path);
        std::panic::catch_unwind(|| {
            let test_cases = trybuild::TestCases::new();
            test_cases.pass(rs_path)
        })
    }
}

fn write_to_file(tdir: &TempDir, filename: &str, content: &str) -> PathBuf {
    let path = tdir.path().join(filename);
    let mut f = File::create(&path).unwrap();
    f.write_all(content.as_bytes()).unwrap();
    path
}

/// A positive test, we expect to pass.
pub fn run_test(
    cxx_code: &str,
    header_code: &str,
    rust_code: TokenStream,
    generate: &[&str],
    generate_pods: &[&str],
) {
    do_run_test(
        cxx_code,
        header_code,
        rust_code,
        directives_from_lists(generate, generate_pods, None),
        None,
        None,
        None,
    )
    .unwrap()
}

// A trait for objects which can check the output of the code creation
// process.
pub trait CodeCheckerFns {
    fn check_rust(&self, _rs: syn::File) -> Result<(), TestError> {
        Ok(())
    }
    fn check_cpp(&self, _cpp: &[PathBuf]) -> Result<(), TestError> {
        Ok(())
    }
    fn skip_build(&self) -> bool {
        false
    }
}

// A function applied to the resultant generated Rust code
// which can be used to inspect that code.
pub type CodeChecker = Box<dyn CodeCheckerFns>;

// A trait for objects which can modify builders for testing purposes.
pub trait BuilderModifierFns {
    fn modify_autocxx_builder(
        &self,
        builder: Builder<TestBuilderContext>,
    ) -> Builder<TestBuilderContext>;
    fn modify_cc_builder<'a>(&self, builder: &'a mut cc::Build) -> &'a mut cc::Build {
        builder
    }
}

pub type BuilderModifier = Box<dyn BuilderModifierFns>;

/// A positive test, we expect to pass.
#[allow(clippy::too_many_arguments)] // least typing for each test
pub fn run_test_ex(
    cxx_code: &str,
    header_code: &str,
    rust_code: TokenStream,
    directives: TokenStream,
    builder_modifier: Option<BuilderModifier>,
    code_checker: Option<CodeChecker>,
    extra_rust: Option<TokenStream>,
) {
    do_run_test(
        cxx_code,
        header_code,
        rust_code,
        directives,
        builder_modifier,
        code_checker,
        extra_rust,
    )
    .unwrap()
}

pub fn run_test_expect_fail(
    cxx_code: &str,
    header_code: &str,
    rust_code: TokenStream,
    generate: &[&str],
    generate_pods: &[&str],
) {
    do_run_test(
        cxx_code,
        header_code,
        rust_code,
        directives_from_lists(generate, generate_pods, None),
        None,
        None,
        None,
    )
    .expect_err("Unexpected success");
}

pub fn run_test_expect_fail_ex(
    cxx_code: &str,
    header_code: &str,
    rust_code: TokenStream,
    directives: TokenStream,
    builder_modifier: Option<BuilderModifier>,
    code_checker: Option<CodeChecker>,
    extra_rust: Option<TokenStream>,
) {
    do_run_test(
        cxx_code,
        header_code,
        rust_code,
        directives,
        builder_modifier,
        code_checker,
        extra_rust,
    )
    .expect_err("Unexpected success");
}

/// In the future maybe the tests will distinguish the exact type of failure expected.
#[derive(Debug)]
pub enum TestError {
    AutoCxx(BuilderError),
    CppBuild(cc::Error),
    RsBuild,
    NoRs,
    RsFileOpen(std::io::Error),
    RsFileRead(std::io::Error),
    RsFileParse(syn::Error),
    RsCodeExaminationFail,
    CppCodeExaminationFail,
}

pub fn directives_from_lists(
    generate: &[&str],
    generate_pods: &[&str],
    extra_directives: Option<TokenStream>,
) -> TokenStream {
    let generate = generate.iter().map(|s| {
        quote! {
            generate!(#s)
        }
    });
    let generate_pods = generate_pods.iter().map(|s| {
        quote! {
            generate_pod!(#s)
        }
    });
    quote! {
        #(#generate)*
        #(#generate_pods)*
        #extra_directives
    }
}

#[allow(clippy::too_many_arguments)] // least typing for each test
pub fn do_run_test(
    cxx_code: &str,
    header_code: &str,
    rust_code: TokenStream,
    directives: TokenStream,
    builder_modifier: Option<BuilderModifier>,
    rust_code_checker: Option<CodeChecker>,
    extra_rust: Option<TokenStream>,
) -> Result<(), TestError> {
    let hexathorpe = Token![#](Span::call_site());
    let unexpanded_rust = quote! {
            use autocxx::prelude::*;

            include_cpp!(
                #hexathorpe include "input.h"
                safety!(unsafe_ffi)
                #directives
            );

            #extra_rust

            fn main() {
                #rust_code
            }

    };
    do_run_test_manual(
        cxx_code,
        header_code,
        unexpanded_rust,
        builder_modifier,
        rust_code_checker,
    )
}

/// The [`BuilderContext`] used in autocxx's integration tests.
pub struct TestBuilderContext;

impl BuilderContext for TestBuilderContext {
    fn get_dependency_recorder() -> Option<Box<dyn RebuildDependencyRecorder>> {
        None
    }
}

pub fn do_run_test_manual(
    cxx_code: &str,
    header_code: &str,
    mut rust_code: TokenStream,
    builder_modifier: Option<BuilderModifier>,
    rust_code_checker: Option<CodeChecker>,
) -> Result<(), TestError> {
    const HEADER_NAME: &str = "input.h";
    // Step 2: Write the C++ header snippet to a temp file
    let tdir = tempdir().unwrap();
    write_to_file(
        &tdir,
        HEADER_NAME,
        &format!("#pragma once\n{}", header_code),
    );
    write_to_file(&tdir, "cxx.h", HEADER);

    rust_code.append_all(quote! {
        #[link(name="autocxx-demo")]
        extern {}
    });
    info!("Unexpanded Rust: {}", rust_code);

    let write_rust_to_file = |ts: &TokenStream| -> PathBuf {
        // Step 3: Write the Rust code to a temp file
        let rs_code = format!("{}", ts);
        write_to_file(&tdir, "input.rs", &rs_code)
    };

    let target_dir = tdir.path().join("target");
    std::fs::create_dir(&target_dir).unwrap();

    let rs_path = write_rust_to_file(&rust_code);

    info!("Path is {:?}", tdir.path());
    let builder = Builder::<TestBuilderContext>::new(&rs_path, &[tdir.path()])
        .custom_gendir(target_dir.clone());
    let builder = if let Some(builder_modifier) = &builder_modifier {
        builder_modifier.modify_autocxx_builder(builder)
    } else {
        builder
    };
    let build_results = builder.build_listing_files().map_err(TestError::AutoCxx)?;
    let mut b = build_results.0;
    let generated_rs_files = build_results.1;

    if let Some(code_checker) = &rust_code_checker {
        let mut file = File::open(generated_rs_files.get(0).ok_or(TestError::NoRs)?)
            .map_err(TestError::RsFileOpen)?;
        let mut content = String::new();
        file.read_to_string(&mut content)
            .map_err(TestError::RsFileRead)?;

        let ast = syn::parse_file(&content).map_err(TestError::RsFileParse)?;
        code_checker.check_rust(ast)?;
        code_checker.check_cpp(&build_results.2)?;
        if code_checker.skip_build() {
            return Ok(());
        }
    }

    let target = rust_info::get().target_triple.unwrap();

    if !cxx_code.is_empty() {
        // Step 4: Write the C++ code snippet to a .cc file, along with a #include
        //         of the header emitted in step 5.
        let cxx_code = format!("#include \"input.h\"\n#include \"cxxgen.h\"\n{}", cxx_code);
        let cxx_path = write_to_file(&tdir, "input.cxx", &cxx_code);
        b.file(cxx_path);
    }

    let b = b
        .out_dir(&target_dir)
        .host(&target)
        .target(&target)
        .opt_level(1)
        .flag("-std=c++14") // For clang
        .flag_if_supported("/GX"); // Enable C++ exceptions for msvc
    let b = if let Some(builder_modifier) = builder_modifier {
        builder_modifier.modify_cc_builder(b)
    } else {
        b
    };
    b.include(tdir.path())
        .try_compile("autocxx-demo")
        .map_err(TestError::CppBuild)?;
    if KEEP_TEMPDIRS {
        println!("Generated .rs files: {:?}", generated_rs_files);
    }
    // Step 8: use the trybuild crate to build the Rust file.
    let r = get_builder().lock().unwrap().build(
        &target_dir,
        "autocxx-demo",
        &tdir.path(),
        &["input.h", "cxx.h"],
        &rs_path,
        generated_rs_files,
    );
    if KEEP_TEMPDIRS {
        println!("Tempdir: {:?}", tdir.into_path().to_str());
    }
    if r.is_err() {
        return Err(TestError::RsBuild); // details of Rust panic are a bit messy to include, and
                                        // not important at the moment.
    }
    Ok(())
}
