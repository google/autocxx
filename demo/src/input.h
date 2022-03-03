// Copyright 2020 Google LLC
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

#pragma once

#include <cstdint>
#include <strstream>
#include <stdint.h>
#include <string>

class Goat {
public:
    Goat();
    ~Goat();
    void add_a_horn();
    std::string describe() const;
private:
    uint32_t horns;
};

inline uint32_t DoMath(uint32_t a) {
    return a * 3;
}

Goat::Goat() : horns(0) {}
Goat::~Goat() {}
void Goat::add_a_horn() { horns++; }
std::string Goat::describe() const {
    std::ostrstream oss;
    std::string plural = horns == 1 ? "" : "s";
    oss << "This goat has " << horns << " horn" << plural << ".";
    return oss.str();
}