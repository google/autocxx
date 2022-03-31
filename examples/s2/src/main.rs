// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use autocxx::prelude::*;

include_cpp! {
    // C++ headers we want to include.
    #include "s2/r2rect.h"
    #include "extras.h"
    // Safety policy. We are marking that this whole C++ inclusion is unsafe
    // which means the functions themselves do not need to be marked
    // as unsafe. Other policies are possible.
    safety!(unsafe)
    // What types and functions we want to generate
    generate!("R1Interval")
    generate!("R2Rect")
    generate!("describe_point")
}

// Everything that we care about is inlined, so we don't have to do
// anything fancy to build or link any external code.
fn main() {
    // Create a couple of R1Intervals using their pre-existing C++
    // constructors. Actually these will be cxx::UniquePtr<R1Interval>s.
    let i1 = ffi::R1Interval::make_unique(1.0f64, 2.0f64);
    let i2 = ffi::R1Interval::make_unique(5.0f64, 6.0f64);
    // Create a rect, passing references to the intervals.
    // Note this is 'make_unique1' because R2Rect has multiple
    // overloaded constructors. 'cargo expand' is useful here,
    // and there's work afoot to make this work nicely with
    // rust-analyzer to give IDE autocompletion.
    let r = ffi::R2Rect::make_unique1(&i1, &i2);
    // Call a method on one of these objects. As it happens,
    // this returns a
    // UniquePtr< ... opaque object representing a point ...>.
    let center = r.GetCenter();
    // As the object is too complex for autocxx to understand,
    // we can't do much with it except to send it into other
    // C++ APIs. We'll make our own which describes the point.
    // This will return a std::string, which autocxx will
    // convert to a UniquePtr<CxxString>. We can convert that
    // back to a Rust string and print it, so long as we
    // take care to decide how to deal with non-UTF8
    // characters (hence the unwrap).
    println!(
        "Center of rectangle is {}",
        ffi::describe_point(center).to_str().unwrap()
    );
}
