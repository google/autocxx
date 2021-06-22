#pragma once

#include <cstdint>
#include <vector>

struct A {
	uint32_t a;
};

// Pre-existing function.

inline void take_vec(std::vector<A>& some_vec) {
	A a;
	a.a = 3;
	some_vec.push_back(a);
}

// How do we create a std::vector<A> from Rust?
// Add this function (for now)

inline std::vector<A> make_vec_a() {
	return std::vector<A>();
}
