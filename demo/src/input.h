// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#pragma once

#include <cstdint>
#include <sstream>
#include <stdint.h>
#include <string>

/// A C++ goat.
class Goat {
public:
    /// Goat constructor.
    Goat() : horns(0) {}
    /// Add a horn to the goat.
    void add_a_horn();
    /// Describe the goat.
    std::string describe() const;
private:
    uint32_t horns;
};

/// Do some maths!
inline uint32_t DoMath(uint32_t a) {
    return a * 3;
}

inline void Goat::add_a_horn() { horns++; }
inline std::string Goat::describe() const {
    std::ostringstream oss;
    std::string plural = horns == 1 ? "" : "s";
    oss << "This goat has " << horns << " horn" << plural << ".";
    return oss.str();
}
