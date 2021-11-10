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

#pragma once
#include <iostream>

struct Point {
  int x;
  int y;
};

class Rect {
public:
  Point top_left;
  Point bottom_right;
  int width() const { return bottom_right.x - top_left.x; }
  int height() const { return bottom_right.y - top_left.y; }
};

inline void print_point(Point p) {
  std::cout << "(" << p.x << ", " << p.y << ")\n";
}
