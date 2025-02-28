// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(skip_windows_gnu_failing_tests)");
    println!("cargo::rustc-check-cfg=cfg(skip_windows_msvc_failing_tests)");
}
