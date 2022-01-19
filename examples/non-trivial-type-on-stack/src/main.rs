// Copyright 2022 Google LLC
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
