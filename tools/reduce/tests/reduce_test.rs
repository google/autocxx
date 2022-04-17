// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use assert_cmd::Command;
use proc_macro2::Span;
use quote::quote;
use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};
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

enum Input {
    Header(String),
    ReproCase(PathBuf),
}

#[test]
fn test_reduce_direct_header() -> Result<(), Box<dyn std::error::Error>> {
    do_reduce(|header, _| Ok(Input::Header(header.into())))
}

#[test]
fn test_reduce_direct_repro_case() -> Result<(), Box<dyn std::error::Error>> {
    do_reduce(|header, demo_code_dir| {
        let config = format!(
            "#include \"{}\" generate_all!() block!(\"First\")
            safety!(unsafe_ffi)",
            header
        );
        let json = serde_json::json!({
            "header": INPUT_H,
            "config": config
        });
        let repropath = demo_code_dir.join("repro.json");
        let f = File::create(&repropath)?;
        serde_json::to_writer(f, &json)?;
        Ok(Input::ReproCase(repropath))
    })
}

#[test]
#[ignore] // takes absolutely ages but you can run using cargo test -- --ignored
fn test_reduce_preprocessed_repro_case() -> Result<(), Box<dyn std::error::Error>> {
    do_reduce(|header, demo_code_dir| {
        write_minimal_rs_code(header, demo_code_dir);
        let repro = demo_code_dir.join("autocxx-repro.json");
        let mut cmd = Command::cargo_bin("autocxx-gen")?;
        cmd.arg("--inc")
            .arg(demo_code_dir.to_str().unwrap())
            .arg(demo_code_dir.join("main.rs"))
            .env("AUTOCXX_REPRO_CASE", repro.to_str().unwrap())
            .arg("--outdir")
            .arg(demo_code_dir.to_str().unwrap())
            .arg("--gen-cpp")
            .arg("--suppress-system-headers")
            .assert()
            .success();
        Ok(Input::ReproCase(repro))
    })
}

#[test]
#[ignore] // takes absolutely ages but you can run using cargo test -- --ignored
fn test_reduce_preprocessed() -> Result<(), Box<dyn std::error::Error>> {
    do_reduce(|header, demo_code_dir| {
        write_minimal_rs_code(header, demo_code_dir);
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
        Ok(Input::Header("autocxx-preprocessed.h".into()))
    })
}

fn write_minimal_rs_code(header: &str, demo_code_dir: &Path) {
    let hexathorpe = Token![#](Span::call_site());
    write_to_file(
        demo_code_dir,
        "main.rs",
        quote! {
            autocxx::include_cpp! {
                #hexathorpe include #header
                generate_all!()
                block!("First")
                safety!(unsafe_ffi)
            }
        }
        .to_string()
        .as_bytes(),
    );
}

fn do_reduce<F>(get_repro_case: F) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnOnce(&str, &Path) -> Result<Input, Box<dyn std::error::Error>>,
{
    if creduce_is_broken() {
        return Ok(());
    }
    let tmp_dir = TempDir::new("example")?;
    let demo_code_dir = tmp_dir.path().join("demo");
    std::fs::create_dir(&demo_code_dir).unwrap();
    write_to_file(&demo_code_dir, "input.h", INPUT_H.as_bytes());
    let output_path = tmp_dir.path().join("min.h");
    let repro_case = get_repro_case("input.h", &demo_code_dir)?;
    let mut cmd = Command::cargo_bin("autocxx-reduce")?;
    let mut cmd = cmd
        .arg("-o")
        .arg(output_path.to_str().unwrap())
        .arg("-p")
        .arg("type marked as blocked")
        .arg("-k");
    match repro_case {
        Input::Header(header_name) => {
            cmd = cmd
                .arg("file")
                .arg("--inc")
                .arg(demo_code_dir.to_str().unwrap())
                .arg("-h")
                .arg(header_name)
                .arg("-d")
                .arg("generate_all!()")
                .arg("-d")
                .arg("block!(\"First\")");
        }
        Input::ReproCase(repro_case) => {
            cmd = cmd.arg("repro").arg("-r").arg(repro_case);
        }
    }
    eprintln!("Running {:?}", cmd);
    let o = cmd.output()?;
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
