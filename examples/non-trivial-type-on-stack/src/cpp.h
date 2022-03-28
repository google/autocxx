// Copyright 2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#pragma once
#include <iostream>

class MessageBuffer {
public:
  // std::string is not a trivial type because in some STL implementations
  // it may contain a self-referential pointer.
  void add_blurb(std::string blurb) {
    message += blurb;
  }
  std::string get() const {
    return message;
  }
private:
  std::string message;
};
