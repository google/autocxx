// Copyright 2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use autocxx::prelude::*;

include_cpp! {
    // C++ headers we want to include.
    #include "cpp.h"
    safety!(unsafe)
    // A non-trivial C++ type
    generate!("MessageBuffer")
}

fn main() {
    // Put the non-trivial C++ type on the Rust stack.
    moveit! { let mut msg = ffi::MessageBuffer::new(); }
    // Call methods on it.
    msg.as_mut().add_blurb("Hello");
    msg.as_mut().add_blurb(" world!");

    assert_eq!(
        msg.get().as_ref().unwrap().to_string_lossy(),
        "Hello world!"
    );
}
