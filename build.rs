// Copyright 2023 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// It would be nice to use the rustversion crate here instead,
// but that doesn't work with inner attributes.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(nightly)");
    if let Some(ver) = rustc_version() {
        if ver.contains("nightly") {
            println!("cargo:rustc-cfg=nightly")
        }
    }
}

fn rustc_version() -> Option<String> {
    let rustc = std::env::var_os("RUSTC")?;
    let output = std::process::Command::new(rustc)
        .arg("--version")
        .output()
        .ok()?;
    let version = String::from_utf8(output.stdout).ok()?;
    Some(version)
}
