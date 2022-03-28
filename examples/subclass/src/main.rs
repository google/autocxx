// Copyright 2021 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// This example shows some Rust subclasses of C++ classes.

mod billy;
mod uwu;

use autocxx::prelude::*;
use autocxx::subclass::prelude::*;
use cxx::CxxString;
use std::cell::RefCell;
use std::rc::Rc;

include_cpp! {
    #include "messages.h"
    safety!(unsafe) // unsafety policy; see docs
}

// Here's the definition of MessageDisplayer from src/messages.h:
// ```cpp
// class MessageDisplayer {
// public:
//     virtual void display_message(const std::string& message) const = 0;
//     virtual ~MessageDisplayer() {};
// };
// ```
// The following lines define a subclass of MessageDisplayer.
// See the main function at the bottom for how this subclass
// is instantiated.

#[is_subclass(superclass("MessageDisplayer"))]
#[derive(Default)]
pub struct UwuDisplayer {}

impl ffi::MessageDisplayer_methods for UwuDisplayer {
    fn display_message(&self, msg: &CxxString) {
        let uwu = uwu::uwu(msg.to_str().unwrap());
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

#[is_subclass(superclass("MessageProducer"))]
#[derive(Default)]
pub struct QuoteProducer;

// Here we've chosen to have an explicit constructor instead rather than deriving
// from CppSubclassDefault. It's functionally the same.
impl QuoteProducer {
    fn new() -> Rc<RefCell<Self>> {
        Self::new_rust_owned(Self::default())
    }
}

impl ffi::MessageProducer_methods for QuoteProducer {
    fn get_message(&self) -> cxx::UniquePtr<CxxString> {
        use ffi::ToCppString;
        billy::SHAKESPEARE_QUOTES[fastrand::usize(0..billy::SHAKESPEARE_QUOTES.len())].into_cpp()
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

#[is_subclass(superclass("MessageDisplayer"))]
pub struct BoxDisplayer {
    message_count: RefCell<usize>,
}

impl BoxDisplayer {
    fn new() -> Rc<RefCell<Self>> {
        Self::new_rust_owned(Self {
            // As we're allocating this class ourselves instead of using [`Default`]
            // we need to initialize the `cpp_peer` member ourselves. This member is
            // inserted by the `#[is_subclass]` annotation. autocxx will
            // later use this to store a pointer back to the C++ peer.
            cpp_peer: Default::default(),
            message_count: RefCell::new(1usize),
        })
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
    // Construct a Rust-owned UwuDisplayer. We can also construct
    // a C++-owned or self-owned subclass - see docs for `CppSubclass`.
    let uwu = UwuDisplayer::default_rust_owned();
    // The next line casts the &UwuDisplayerCpp to a &MessageDisplayer.
    ffi::register_displayer(uwu.as_ref().borrow().as_ref());
    // Constructs in just the same way as the first one, but using
    // our explicit constructor.
    let boxd = BoxDisplayer::new();
    ffi::register_displayer(boxd.as_ref().borrow().as_ref());
    let shakespeare = QuoteProducer::new();
    ffi::register_producer(shakespeare.as_ref().borrow().as_ref());
    ffi::run_demo();
    ffi::run_demo();
}
