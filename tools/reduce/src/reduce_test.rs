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

use assert_cmd::Command;
use std::{fs::File, io::Write, path::Path};
use tempdir::TempDir;

static INPUT_H: &str = indoc::indoc! {"
    inline int DoMath(int a) {
        return a * 3;
    }

    namespace {
        inline void foo() {}
    }; // anonymous namespace, not currently supported
"};

#[test]
fn test_reduce() -> Result<(), Box<dyn std::error::Error>> {
    let tmp_dir = TempDir::new("example")?;
    let demo_code_dir = tmp_dir.path().join("demo");
    std::fs::create_dir(&demo_code_dir).unwrap();
    write_to_file(&demo_code_dir, "input.h", INPUT_H.as_bytes());
    let mut cmd = Command::cargo_bin("autocxx-reduce")?;
    let output_path = tmp_dir.path().join("min.h");
    cmd.arg("--inc")
        .arg(demo_code_dir.to_str().unwrap())
        .arg("-h")
        .arg("input.h")
        .arg("-o")
        .arg(output_path.to_str().unwrap())
        .arg("-d")
        .arg("generate_all!()")
        .arg("-p")
        .arg("anonymous")
        .assert()
        .success();
    let minimized = std::fs::read_to_string(tmp_dir.path().join("min.h"))?;
    assert!(minimized.contains("namespace"));
    assert!(!minimized.contains("DoMath"));
    Ok(())
}

fn write_to_file(dir: &Path, filename: &str, content: &[u8]) {
    let path = dir.join(filename);
    let mut f = File::create(&path).expect("Unable to create file");
    f.write_all(content).expect("Unable to write file");
}
