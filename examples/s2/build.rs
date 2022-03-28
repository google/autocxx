// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

fn main() {
    // It's necessary to use an absolute path here because the
    // C++ codegen and the macro codegen appears to be run from different
    // working directories.
    let path = std::path::PathBuf::from("s2geometry/src");
    let path2 = std::path::PathBuf::from("src");
    let mut b = autocxx_build::Builder::new("src/main.rs", &[&path, &path2]).expect_build();
    b.flag_if_supported("-std=c++14")
        .compile("autocxx-s2-example");
    println!("cargo:rerun-if-changed=src/main.rs");
}
