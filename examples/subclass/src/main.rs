// Copyright 2021 Google LLC
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

// This example shows some Rust subclasses of C++ classes.

use autocxx::include_cpp;
use autocxx::subclass::{is_subclass, CppSubclass};
use cxx::CxxString;
use std::cell::RefCell;
use std::rc::Rc;

include_cpp! {
    #include "messages.h"
    safety!(unsafe)
    // What types and functions we want to generate
    generate!("run_demo")
    generate!("register_cpp_thingies")
    generate!("register_producer")
    generate!("register_displayer")
    // Declare some C++ classes have Rust subclasses.
    subclass!("MessageDisplayer",UwuDisplayer)
    subclass!("MessageDisplayer",BoxDisplayer)
    subclass!("MessageProducer",QuoteProducer)
}

static SHAKESPEARE_QUOTES: [&str; 10] = [
    "All that glitters is not gold",
    "Hell is empty and all the devils are here.",
    "Good night, good night! parting is such sweet sorrow, That I shall say good night till it be morrow.",
    "These violent delights have violent ends...",
    "Something is rotten in the state of Denmark.",
    "Love all, trust a few, do wrong to none.",
    "The lady doth protest too much, methinks.",
    "Brevity is the soul of wit.",
    "Uneasy lies the head that wears a crown.",
    "Now is the winter of our discontent.",
];

// Here's the definition of MessageDisplayer from src/messages.h:
// ```cpp
// class MessageDisplayer {
// public:
//     virtual void display_message(const std::string& message) const = 0;
//     virtual ~MessageDisplayer() {};
// };
// ```
// The following lines define a subclass of MessageDisplayer,
// together with the "subclass!" directive above in the include_cpp!
// macro. See the main function at the bottom for how this subclass
// is instantiated.

#[is_subclass]
#[derive(Default)]
pub struct UwuDisplayer {}

impl UwuDisplayer {
    fn new() -> Rc<RefCell<Self>> {
        // We create and pass an instance of
        // the subclass on the Rust side - that's the first parameter -
        // and also a closure which calls an appropriate C++ constructor for
        // the C++ side peer object. Constructors exist for each superclass
        // constructor, so effectively this is a call to the C++ superclass
        // constructor.
        // The `new_rust_owned` function in the `CppSubclass` trait
        // (which we implement) takes care of gluing the C++ and Rust side
        // objects together.
        // We could have said `new_cpp_owned` instead, which would
        // have given us a cxx::UniquePtr that we could have passed to C++.
        Self::new_rust_owned(Self::default(), ffi::UwuDisplayerCpp::make_unique)
    }
}

impl ffi::MessageDisplayer_methods for UwuDisplayer {
    fn display_message(&self, _msg: &CxxString) {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        let uwu = uwuifier::uwuify_str_sse(_msg.to_str().unwrap());
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
        let uwu = "uwuification is unavailable for this pwatform :(";
        println!("{}", uwu);
    }
}

// And here's a different pure virtual class.
// Here's its definition from src/messages.h:
// ```cpp
// class MessageProducer {
// public:
//     virtual std::string get_message() const = 0;
//     virtual ~MessageProducer() {};
// };
// ```
// This one is notable only in that the interface of the C++ class
// involves std::string, yet in Rust the subclass uses
// std::unique_ptr<std::string> (for all the normal reasons in autocxx -
// for now, at least, we can't hold non-trivial C++ objects on the Rust stack.)
// All the boxing and unboxing is done automatically by autocxx layers.

#[is_subclass]
#[derive(Default)]
pub struct QuoteProducer {}

impl QuoteProducer {
    fn new() -> Rc<RefCell<Self>> {
        Self::new_rust_owned(Self::default(), ffi::QuoteProducerCpp::make_unique)
    }
}

impl ffi::MessageProducer_methods for QuoteProducer {
    fn get_message(&self) -> cxx::UniquePtr<CxxString> {
        use ffi::ToCppString;
        SHAKESPEARE_QUOTES[fastrand::usize(0..SHAKESPEARE_QUOTES.len())].into_cpp()
    }
}

// Here's another subclass of the same 'displayer' class.
// This one is more complex in two ways.
//
// First, we actually want to store some data here in our subclass.
// That means we can't just allocate ourselves with Default::default().
// And that means we need to be aware of the cpp_peer field which is
// added by the #[subclass] macro.
//
// Second, we're going to simulate the observer/listener type pattern
// in C++ where a const* is used to send messages around a codebase yet
// recipients need to react by mutating themselves or otherwise actively
// doing stuff. In C++ you'd probably need a const_cast. Here we use
// interior mutability.

#[is_subclass]
pub struct BoxDisplayer {
    message_count: RefCell<usize>,
}

impl BoxDisplayer {
    fn new() -> Rc<RefCell<Self>> {
        Self::new_rust_owned(
            Self {
                // As we're allocating this class ourselves instead of using [`Default`]
                // we need to initialize the `cpp_peer` member ourselves. This member is
                // inserted by the `#[is_subclass]` annotation. autocxx will
                // later use this to store a pointer back to the C++ peer.
                cpp_peer: Default::default(),
                message_count: RefCell::new(1usize),
            },
            ffi::BoxDisplayerCpp::make_unique,
        )
    }
}

impl ffi::MessageDisplayer_methods for BoxDisplayer {
    fn display_message(&self, msg: &CxxString) {
        let msg = textwrap::fill(msg.to_str().unwrap(), 70);
        let horz_line = std::iter::repeat("#").take(74).collect::<String>();
        println!("{}", horz_line);
        let msgmsg = format!("Message {}", self.message_count.borrow());
        self.message_count.replace_with(|old| *old + 1usize);
        println!("# {:^70} #", msgmsg);
        println!("{}", horz_line);
        for l in msg.lines() {
            println!("# {:^70} #", l);
        }
        println!("{}", horz_line);
    }
}

fn main() {
    ffi::register_cpp_thingies();
    let uwu = UwuDisplayer::new();
    // The next line casts the &UwuDisplayerCpp to a &MessageDisplayer.
    ffi::register_displayer(uwu.as_ref().borrow().as_ref());
    let boxd = BoxDisplayer::new();
    ffi::register_displayer(boxd.as_ref().borrow().as_ref());
    let shakespeare = QuoteProducer::new();
    ffi::register_producer(shakespeare.as_ref().borrow().as_ref());
    ffi::run_demo();
    ffi::run_demo();
}
