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
    generate!("MessageBuffer")
}

use ffi::{Point, Rect};

// A simple example dealing with plain-old-data types.

fn main() {
    moveit! {
        let mut msg = ffi::MsgBuffer::new();
    }
    msg.as_mut().add_blurb("Hello");
    msg.as_mut().add_blurb(" world!");

    assert_eq!(msg.get().unwrap().to_str_lossy(), "Hello world!");
}
