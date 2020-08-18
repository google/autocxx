autocxx_macro::include_cxx!(
    Header("test.h"),
    Allow("DoMath"),
);

fn main() {
    println!("Hello, world! - C++ math should say 12={}", ffi::DoMath(4));
}
