// Copyright 2021 Google LLC
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
    // Safety policy. We are marking that this whole C++ inclusion is unsafe
    // which means the functions themselves do not need to be marked
    // as unsafe. Other policies are possible.
    safety!(unsafe)
    // What types and functions we want to generate
    generate_pod!("Rect")
    generate!("print_point")
}

use ffi::{Point, Rect};

// A simple example dealing with plain-old-data types.

fn main() {
    let r = Rect {
        top_left: Point { x: 3, y: 3 },
        bottom_right: Point { x: 12, y: 15 },
    };
    // r.width() and r.height() return an autocxx::c_int
    // which we need to unpackage. It is hoped that one day cxx will
    // natively support 'int' and friends, and that won't be necessary.
    let center = Point {
        x: r.top_left.x + r.width().0 / 2,
        y: r.top_left.y + r.height().0 / 2,
    };
    ffi::print_point(center);
}
