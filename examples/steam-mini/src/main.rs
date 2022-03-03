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
    #include "steam.h"
    // Safety policy. We are marking that this whole C++ inclusion is unsafe
    // which means the functions themselves do not need to be marked
    // as unsafe. Other policies are possible.
    safety!(unsafe)
    // What types and functions we want to generate
    generate!("GetSteamEngine")
    generate!("IEngine")
}

fn main() {
    // The "Steam" API gives us a void* on which we call virtual functions.
    // This is a void*.
    let steam_engine = ffi::GetSteamEngine();
    // We need to know three things about this void*:
    // 1. What is it? We know from the (fake) Steam documentation that it's
    //    an IEngine*
    // 2. Do we gain ownership of it? i.e. is it our responsibility to
    //    destroy it?
    // 3. If not, C++ presumably continues to own it. Does C++ ever destroy
    //    it?
    // None of these things are really encoded in the nature of a void*
    // so you have to figure them out from the documentation.
    //
    // In this case, the first is easy:
    let steam_engine = steam_engine as *mut ffi::IEngine;
    //
    // You then need to figure out how to expose it in Rust. Ideally, any
    // such lifetime invariants would be handled by the compiler.
    // If C++ is passing ownership of this object to us, and we have the
    // prerogative to destroy it whenever we wish, then
    // simply use [`cxx::UniquePtr::from_raw`]. If it goes out of scope in
    // Rust the underlying C++ object will be deleted.
    //
    // Let's assume life is more complicated, and we must never destroy this
    // object (because it's owned by C++). In that case, we ideally want
    // to convert the pointer into a Rust reference with the lifetime of
    // the program.
    //
    // We also have to promise to Rust that it'll never move in memory.
    // C++ doesn't do that, so that's OK.
    let mut steam_engine = unsafe { std::pin::Pin::new_unchecked(&mut *steam_engine) };
    // Now we have steam_engine which is a Pin<&mut SteamEngine>
    // Each time we call a method we need to add `as_mut()`
    // as per the pattern explained in
    // https://doc.rust-lang.org/std/pin/struct.Pin.html#method.as_mut
    steam_engine
        .as_mut()
        .ConnectToGlobalUser(autocxx::c_int(12));
    steam_engine
        .as_mut()
        .DisconnectGlobalUser(autocxx::c_int(12));
}
