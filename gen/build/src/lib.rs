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

pub use autocxx_engine::Builder;

use autocxx_engine::{BuilderBuild, BuilderError, RebuildDependencyRecorder};
use std::{collections::HashSet, io::Write, sync::Mutex};
use std::{ffi::OsStr, path::Path};

#[deprecated]
/// Use [`builder`] instead
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
    builder(rs_file, autocxx_incs)
        .extra_clang_args(extra_clang_args)
        .build()
}

#[deprecated]
/// Use [`builder`] instead
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
    builder(rs_file, autocxx_incs)
        .extra_clang_args(extra_clang_args)
        .expect_build()
}

fn setup_logging() {
    env_logger::builder()
        .format(|buf, record| writeln!(buf, "cargo:warning=MESSAGE:{}", record.args()))
        .init();
}

/// Create a builder object.
/// You'll need to call `build` on this twice, effectively...
/// the first time, this will return a [`cc::Build`] and then
/// you'll need to use that to build the C++ code.
pub fn builder(
    rs_file: impl AsRef<Path>,
    autocxx_incs: impl IntoIterator<Item = impl AsRef<OsStr>>,
) -> Builder {
    setup_logging();
    let b = Builder::new_internal(rs_file, autocxx_incs);
    b.dependency_recorder(Box::new(CargoRebuildDependencyRecorder::new()))
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
