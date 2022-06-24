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
    let field = ffi::Field::new().within_unique_ptr();
    let field = ffi::CppUniquePtrPin::new(field);
    let another_goat = field.as_cpp_ref().get_goat();
    assert_eq!(
        another_goat
            .describe()
            .as_ref()
            .unwrap()
            .to_string_lossy(),
        "This goat has 0 horns."
    );
}
