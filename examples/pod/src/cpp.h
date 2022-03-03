// Copyright 2021 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

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
