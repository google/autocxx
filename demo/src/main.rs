// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

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
