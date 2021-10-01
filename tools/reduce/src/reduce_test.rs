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
use proc_macro2::Span;
use quote::quote;
use std::{fs::File, io::Write, path::Path};
use syn::Token;
use tempdir::TempDir;

static INPUT_H: &str = indoc::indoc! {"
    inline int DoMath(int a) {
        return a * 3;
    }

    struct First {
        First() {}
        int foo;
    };

    struct Second {
        Second(const First& a) {}
        int bar;
    };
"};

#[test]
fn test_reduce_direct() -> Result<(), Box<dyn std::error::Error>> {
    do_reduce(|header, _| Ok(header.into()))
}

#[test]
#[ignore] // takes absolutely ages but you can run using cargo test -- --ignored
fn test_reduce_preprocessed() -> Result<(), Box<dyn std::error::Error>> {
    do_reduce(|header, demo_code_dir| {
        let hexathorpe = Token![#](Span::call_site());
        write_to_file(
            &demo_code_dir,
            "main.rs",
            quote! {
                autocxx::include_cpp! {
                    #hexathorpe include #header
                    generate_all!()
                    safety(unsafe_ffi)
                }
            }
            .to_string()
            .as_bytes(),
        );
        let prepro = demo_code_dir.join("autocxx-preprocessed.h");
        let mut cmd = Command::cargo_bin("autocxx-gen")?;
        cmd.arg("--inc")
            .arg(demo_code_dir.to_str().unwrap())
            .arg(demo_code_dir.join("main.rs"))
            .env("AUTOCXX_PREPROCESS", prepro.to_str().unwrap())
            .arg("--outdir")
            .arg(demo_code_dir.to_str().unwrap())
            .arg("--gen-cpp")
            .arg("--suppress-system-headers")
            .assert()
            .success();
        Ok("autocxx-preprocessed.h".into())
    })
}

fn do_reduce<F>(get_header_name: F) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnOnce(&str, &Path) -> Result<String, Box<dyn std::error::Error>>,
{
    if creduce_is_broken() {
        return Ok(());
    }
    let tmp_dir = TempDir::new("example")?;
    let demo_code_dir = tmp_dir.path().join("demo");
    std::fs::create_dir(&demo_code_dir).unwrap();
    write_to_file(&demo_code_dir, "input.h", INPUT_H.as_bytes());
    let output_path = tmp_dir.path().join("min.h");
    let header_name = get_header_name("input.h", &demo_code_dir)?;
    let mut cmd = Command::cargo_bin("autocxx-reduce")?;
    let o = cmd
        .arg("--inc")
        .arg(demo_code_dir.to_str().unwrap())
        .arg("-h")
        .arg(header_name)
        .arg("-o")
        .arg(output_path.to_str().unwrap())
        .arg("-d")
        .arg("generate_all!()")
        .arg("-d")
        .arg("block!(\"First\")")
        .arg("-p")
        .arg("type marked as blocked")
        .arg("-k")
        .output()?;
    println!("Reduce output: {}", std::str::from_utf8(&o.stdout).unwrap());
    println!("Reduce error: {}", std::str::from_utf8(&o.stderr).unwrap());
    if !o.status.success() {
        panic!("autocxx-reduce returned non-zero result code");
    }
    let minimized = std::fs::read_to_string(output_path)?;
    assert!(minimized.contains("First"));
    assert!(!minimized.contains("DoMath"));
    Ok(())
}

fn write_to_file(dir: &Path, filename: &str, content: &[u8]) {
    let path = dir.join(filename);
    let mut f = File::create(&path).expect("Unable to create file");
    f.write_all(content).expect("Unable to write file");
}

fn creduce_is_broken() -> bool {
    // On some machines, creduce immediately segfaults
    Command::new("creduce").arg("--version").ok().is_err()
}
