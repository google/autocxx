// Copyright 2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// This example shows calls from C++ back into Rust. That's really not
// the main purpose of autocxx, and this support is immature. If you're
// primarily doing this sort of thing, look into other tools such as
// cbindgen or cxx.

use autocxx::prelude::*;
use std::pin::Pin;

include_cpp! {
    #include "input.h"
    safety!(unsafe_ffi)
    generate!("jurassic")
}

fn main() {
    ffi::jurassic();
}

#[autocxx::extern_rust::extern_rust_type]
pub struct Dinosaur {
    carnivore: bool,
}

#[autocxx::extern_rust::extern_rust_function]
pub fn new_dinosaur(carnivore: bool) -> Box<Dinosaur> {
    Box::new(Dinosaur { carnivore })
}

impl Dinosaur {
    #[autocxx::extern_rust::extern_rust_function]
    fn roar(&self) {
        println!("Roar");
    }

    #[autocxx::extern_rust::extern_rust_function]
    fn eat(self: Pin<&mut Dinosaur>, other_dinosaur: Box<Dinosaur>) {
        assert!(self.carnivore);
        other_dinosaur.get_eaten();
        println!("Nom nom");
    }

    fn get_eaten(&self) {
        println!("Uh-oh");
    }
}

#[autocxx::extern_rust::extern_rust_function]
pub fn go_extinct() {
    println!("Boom")
}
