use autocxx_macro::include_cxx;

// input.h is currently hardcoded in our hacky fork of bindgen, so don't
// attempt to change it!
include_cxx!(
    Header("input.h"),
    Allow("DoMath"),
);

fn main() {
    println!("Hello, world! - C++ math should say 12={}", ffi::DoMath(4));
}
