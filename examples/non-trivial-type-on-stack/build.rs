// Copyright 2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

fn main() {
    let path = std::path::PathBuf::from("src");
    let mut b = autocxx_build::Builder::new("src/main.rs", &[&path]).expect_build();
    b.flag_if_supported("-std=c++14")
        .compile("autocxx-non-trivial-type-on-stack-example");
    println!("cargo:rerun-if-changed=src/main.rs");
    println!("cargo:rerun-if-changed=src/cpp.h");
}
