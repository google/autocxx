autocxx::include_cpp! {
	#include "input.h"
	safety!(unsafe_ffi)
	generate!("take_vec")
	generate!("make_vec_a")
}

fn main() {
	let mut a = ffi::make_vec_a();
	println!("Items in vec: {}", a.len());
	ffi::take_vec(a.pin_mut());
	println!("Items in vec: {}", a.len());
}
