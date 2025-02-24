// Copyright 2022 Google LLC
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

class Goat {
public:
    Goat() : horns(0) {}
    void add_a_horn();
    std::string describe() const;
    uint32_t& get_horns() { return horns; }
private:
    uint32_t horns;
};


inline void Goat::add_a_horn() { horns++; }
inline std::string Goat::describe() const {
    std::ostringstream oss;
    std::string plural = horns == 1 ? "" : "s";
    oss << "This goat has " << horns << " horn" << plural << ".";
    return oss.str();
}

class Field {
public:
    const Goat& get_goat() const {
        return the_goat;
    }

private:
    Goat the_goat;
};
