// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use autocxx::prelude::*;

include_cpp! {
    #include "input.h"
    safety!(unsafe_ffi)
    generate!("DoMath")
    generate!("Goat")
}

fn main() {
    println!("Hello, world! - C++ math should say 12={}", ffi::DoMath(4));
    let mut goat = ffi::Goat::make_unique();
    goat.as_mut().unwrap().add_a_horn();
    goat.as_mut().unwrap().add_a_horn();
    assert_eq!(
        goat.describe().as_ref().unwrap().to_string_lossy(),
        "This goat has 2 horns."
    );
}
