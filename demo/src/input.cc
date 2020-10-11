#include "input.h"
#include <iostream>

S::S(uint32_t i) : i(i) {
  std::cout << "C++:" << (size_t(&this->i) - size_t(this)) << std::endl;
}
