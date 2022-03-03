// Copyright 2021 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// This example shows Rust subclasses of C++ classes.
// Here are the C++ classes which we're subclassing.

#pragma once

#include <string>
#include <memory>

class MessageProducer {
public:
    virtual std::string get_message() const = 0;
    virtual ~MessageProducer() {};
};

class MessageDisplayer {
public:
    virtual void display_message(const std::string& message) const = 0;
    virtual ~MessageDisplayer() {};
};

void register_cpp_thingies();
void register_producer(const MessageProducer& producer);
void register_displayer(const MessageDisplayer& displayer);

void run_demo();
