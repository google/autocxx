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

use autocxx_engine::{
    build as engine_build, expect_build as engine_expect_build, BuilderBuild, BuilderError,
};
use std::io::Write;
use std::{ffi::OsStr, path::Path};

/// Build autocxx C++ files and return a cc::Build you can use to build
/// more from a build.rs file.
/// You need to provide the Rust file path and the iterator of paths
/// which should be used as include directories.
pub fn build<P1, I, T>(rs_file: P1, autocxx_incs: I) -> Result<BuilderBuild, BuilderError>
where
    P1: AsRef<Path>,
    I: IntoIterator<Item = T>,
    T: AsRef<OsStr>,
{
    setup_logging();
    engine_build(rs_file, autocxx_incs).map(|r| r.0)
}

/// Builds successfully, or exits the process displaying a suitable
/// message.
pub fn expect_build<P1, I, T>(rs_file: P1, autocxx_incs: I) -> BuilderBuild
where
    P1: AsRef<Path>,
    I: IntoIterator<Item = T>,
    T: AsRef<OsStr>,
{
    setup_logging();
    engine_expect_build(rs_file, autocxx_incs).0
}

fn setup_logging() {
    env_logger::builder()
        .format(|buf, record| writeln!(buf, "cargo:warning=MESSAGE:{}", record.args()))
        .init();
}
