// Copyright 2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#include "cxxgen.h"
#include "input.h"

void jurassic() {
    rust::Box<Dinosaur> prey = new_dinosaur(false);
    rust::Box<Dinosaur> predator = new_dinosaur(true);
    prey->roar();
    predator->roar();
    predator->eat(std::move(prey));
    go_extinct();
}