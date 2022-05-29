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
    safety!(unsafe_references_wrapped)
    generate!("DoMath")
    generate!("Goat")
}

fn main() {
    println!("Hello, world! - C++ math should say 12={}", ffi::DoMath(4));
    let mut goat = ffi::Goat::new().within_box();
    // The next line is, of course, horrible and needs to go.
    let mut goat = ffi::CppRef(unsafe { std::pin::Pin::into_inner_unchecked(goat.as_ref())  as *const ffi::Goat }, std::marker::PhantomData);
    goat.add_a_horn();
    goat.add_a_horn();
    assert_eq!(
        goat.describe().as_ref().unwrap().to_string_lossy(),
        "This goat has 2 horns."
    );
}
