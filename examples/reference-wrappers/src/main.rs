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
    generate!("Goat")
    generate!("Field")
}

fn main() {
    let mut goat = ffi::Goat::new().within_box();
    let mut goat = ffi::CppMutRef::from_box(&mut goat);
    goat.add_a_horn();
    goat.add_a_horn();
    assert_eq!(
        goat.as_cpp_ref()
            .describe()
            .as_ref()
            .unwrap()
            .to_string_lossy(),
        "This goat has 2 horns."
    );
    let mut field = ffi::Field::new().within_box();
    let mut field = ffi::CppMutRef::from_box(&mut field);
    let another_goat = field.as_cpp_ref().get_goat();
    assert_eq!(
        another_goat.as_cpp_ref()
            .describe()
            .as_ref()
            .unwrap()
            .to_string_lossy(),
        "This goat has 0 horns."
    );
}
