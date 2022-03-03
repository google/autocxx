// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#![forbid(unsafe_code)]

use autocxx_engine::{BuilderBuild, BuilderContext, BuilderError, RebuildDependencyRecorder};
use std::{collections::HashSet, io::Write, sync::Mutex};
use std::{ffi::OsStr, path::Path};

pub type Builder = autocxx_engine::Builder<CargoBuilderContext>;

#[deprecated]
/// Use [`Builder::new`] instead
pub fn build<P1, I, T>(
    rs_file: P1,
    autocxx_incs: I,
    extra_clang_args: &[&str],
) -> Result<BuilderBuild, BuilderError>
where
    P1: AsRef<Path>,
    I: IntoIterator<Item = T>,
    T: AsRef<OsStr>,
{
    Builder::new(rs_file, autocxx_incs)
        .extra_clang_args(extra_clang_args)
        .build()
}

#[deprecated]
/// Use [`Builder::new`] instead
pub fn expect_build<P1, I, T>(
    rs_file: P1,
    autocxx_incs: I,
    extra_clang_args: &[&str],
) -> BuilderBuild
where
    P1: AsRef<Path>,
    I: IntoIterator<Item = T>,
    T: AsRef<OsStr>,
{
    Builder::new(rs_file, autocxx_incs)
        .extra_clang_args(extra_clang_args)
        .expect_build()
}

#[doc(hidden)]
pub struct CargoBuilderContext;

impl BuilderContext for CargoBuilderContext {
    fn setup() {
        env_logger::builder()
            .format(|buf, record| writeln!(buf, "cargo:warning=MESSAGE:{}", record.args()))
            .init();
    }
    fn get_dependency_recorder() -> Option<Box<dyn RebuildDependencyRecorder>> {
        Some(Box::new(CargoRebuildDependencyRecorder::new()))
    }
}

#[derive(Debug)]
struct CargoRebuildDependencyRecorder {
    printed_already: Mutex<HashSet<String>>,
}

impl CargoRebuildDependencyRecorder {
    fn new() -> Self {
        Self {
            printed_already: Mutex::new(HashSet::new()),
        }
    }
}

impl RebuildDependencyRecorder for CargoRebuildDependencyRecorder {
    fn record_header_file_dependency(&self, filename: &str) {
        let mut already = self.printed_already.lock().unwrap();
        if already.insert(filename.into()) {
            println!("cargo:rerun-if-changed={}", filename);
        }
    }
}
