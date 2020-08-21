fn main() {
    // TODO work out why AUTOCXX_INC needs to be different
    // for the C++ generation (this line...)
    std::env::set_var("AUTOCXX_INC", "src");
    let mut b = autocxx_build::Builder::new("src/main.rs").unwrap();
    b.builder()
        .flag_if_supported("-std=c++14")
        .compile("autocxx-demo");

    println!("cargo:rerun-if-changed=src/main.rs");
    println!("cargo:rerun-if-changed=src/input.h");
    // ... (TODO) versus the .rs generation by macro (this line).
    println!("cargo:rustc-env=AUTOCXX_INC=demo/src");
}
