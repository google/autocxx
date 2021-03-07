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

use std::{fs::File, io::Write, path::PathBuf};

use assert_cmd::Command;
use tempdir::TempDir;

static MAIN_RS: &str = include_str!("../../../demo/src/main.rs");
static INPUT_H: &str = include_str!("../../../demo/src/input.h");

#[test]
fn test_help() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("autocxx-gen")?;
    cmd.arg("-h").assert().success();
    Ok(())
}

#[test]
fn test_gen() -> Result<(), Box<dyn std::error::Error>> {
    let tmp_dir = TempDir::new("example")?;
    let demo_code_dir = tmp_dir.path().join("demo");
    std::fs::create_dir(&demo_code_dir).unwrap();
    write_to_file(&demo_code_dir, "input.h", INPUT_H.as_bytes());
    write_to_file(&demo_code_dir, "main.rs", MAIN_RS.as_bytes());
    let demo_rs = demo_code_dir.join("main.rs");
    let mut cmd = Command::cargo_bin("autocxx-gen")?;
    cmd.arg("--inc")
        .arg(demo_code_dir.to_str().unwrap())
        .arg(demo_rs)
        .arg("--outdir")
        .arg(tmp_dir.path().to_str().unwrap())
        .arg("gen-cpp")
        .assert()
        .success();
    Ok(())
}

fn write_to_file(dir: &PathBuf, filename: &str, content: &[u8]) {
    let path = dir.join(filename);
    let mut f = File::create(&path).expect("Unable to create file");
    f.write_all(content).expect("Unable to write file");
}
