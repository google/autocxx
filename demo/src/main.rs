use autocxx_macro::include_cxx;

// The following syntax is very interim as I haven't figured out
// how to parse a string inside a procedural macro yet(!)
include_cxx!(
    <test>,
    <DoMath>
);

fn main() {
    println!("Hello, world! - C++ math should say 12={}", ffi::DoMath(4));
}
