// Copyright 2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// This example serves to demonstrate the experimental C++
// reference wrappers. They exist because C++ references are not
// the same as Rust references: C++ references may alias, whereas
// Rust references may not.
//
// Standard autocxx behavior therefore introduces unsoundness when
// C++ references are encountered and treated like Rust references.
// (cxx has this soundness problem for Trivial types; autocxx
// makes it worse in that the same problem applies even for
// opaque types, because we make them sized such that we can allocate
// them on the stack).
//
// Reference wrappers solve that problem because internally, they're
// just pointers. On the other hand, they're awkward to use,
// especially in the absence of the Rust "arbitrary self types"
// feature.

// Necessary to be able to call methods on reference wrappers.
// For that reason, this example only builds on nightly Rust.
#![feature(arbitrary_self_types_pointers)]

use autocxx::prelude::*;
use std::pin::Pin;

include_cpp! {
    #include "input.h"
    // This next line enables C++ reference wrappers
    // This is what requires the 'arbitrary_self_types' feature.
    safety!(unsafe_references_wrapped)
    generate!("Goat")
    generate!("Field")
}

impl ffi::Goat {
    // Methods can be called on a CppRef<T> or &CppRef<T>
    fn bleat(self: CppRef<Self>) {
        println!("Bleat");
    }
}

trait FarmProduce {
    // Traits can be defined on a CppRef<T> so long as Self: Sized
    fn sell(self: &CppRef<Self>)
    where
        Self: Sized;
}

impl FarmProduce for ffi::Goat {
    fn sell(self: &CppRef<Self>) {
        println!("Selling goat");
    }
}

trait FarmArea {
    fn maintain(self: CppRef<Self>);
}

impl FarmArea for ffi::Field {
    fn maintain(self: CppRef<Self>) {
        println!("Maintaining");
    }
}

fn main() {
    // Create a cxx::UniquePtr as normal for a Field object.
    let field = ffi::Field::new().within_unique_ptr();
    // We assume at this point that C++ has had no opportunity
    // to retain any reference to the Field. That's not strictly
    // true, due to RVO, but under all reasonable circumstances
    // Rust currently has exclusive ownership of the Field we've
    // been given.
    // Therefore, at this point in the program, it's still
    // OK to take Rust references to this Field.
    let _field_rust_ref = field.as_ref();
    // However, as soon as we want to pass a reference to the field
    // back to C++, we have to ensure we have no Rust references
    // in existence. So: we imprison the object in a "CppPin":
    let field = CppUniquePtrPin::new(field);
    // We can no longer take Rust references to the field...
    //   let _field_rust_ref = field.as_ref();
    // However, we can take C++ references. And use such references
    // to call methods in C++. Quite often those methods will
    // return other references, like this.
    let another_goat = field.as_cpp_ref().get_goat();
    // The 'get_goat' method in C++ returns a reference, so this is
    // another CppRef, not a Rust reference.

    // We can still use these C++ references to call Rust methods,
    // so long as those methods have a "self" type of a
    // C++ reference not a Rust reference.
    another_goat.clone().bleat();
    another_goat.sell();

    // But most commonly, C++ references are simply used as the 'this'
    // type when calling other C++ methods, like this.
    assert_eq!(
        another_goat
            .describe() // returns a UniquePtr<CxxString>, there
            // are no Rust or C++ references involved at this point.
            .as_ref()
            .unwrap()
            .to_string_lossy(),
        "This goat has 0 horns."
    );

    // We can even use trait objects, though it's a bit of a fiddle.
    let farm_area: Pin<Box<dyn FarmArea>> = ffi::Field::new().within_box();
    let farm_area = CppPin::from_pinned_box(farm_area);
    farm_area.as_cpp_ref().maintain();
}
