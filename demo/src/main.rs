use autocxx_macro::include_cxx;

// The following syntax is very interim as I haven't figured out
// how to parse a string inside a procedural macro yet(!)
// Also, 'input' is supposed to refer to input.h, but currently
// that name is totally hardcoded in our hacky fork of bindgen, so don't
// attempt to change it!
include_cxx!(
    <input>,
    <DoMath>
);

fn main() {
    println!("Hello, world! - C++ math should say 12={}", ffi::DoMath(4));
}
