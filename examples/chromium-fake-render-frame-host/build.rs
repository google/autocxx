// Copyright 2021 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

fn main() -> miette::Result<()> {
    let path = std::path::PathBuf::from("src");
    let mut b = autocxx_build::Builder::new("src/main.rs", &[&path]).build()?;
    b.flag_if_supported("-std=c++17")       // clang
        .flag_if_supported("/std:c++17")    // msvc
        .file("src/fake-chromium-src.cc")
        .compile("autocxx-fake-render-frame-host-example");
    println!("cargo:rerun-if-changed=src/main.rs");
    println!("cargo:rerun-if-changed=src/fake-chromium-src.cc");
    println!("cargo:rerun-if-changed=src/fake-chromium-header.h");
    Ok(())
}
