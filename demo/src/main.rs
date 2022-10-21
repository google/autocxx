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
    generate!("A")
    generate!("B")
    generate!("get_b")
}

fn main() {
    println!("Hello, world! - C++ math should say 12={}", ffi::DoMath(4));
    let mut goat = ffi::Goat::new().within_box();
    goat.as_mut().add_a_horn();
    goat.as_mut().add_a_horn();
    assert_eq!(
        goat.describe().as_ref().unwrap().to_string_lossy(),
        "This goat has 2 horns."
    );

    let mut b = ffi::get_b();
    b.as_ref().unwrap().as_ref().foo();
    b.as_mut().unwrap().pin_mut().foo_mut();
}
