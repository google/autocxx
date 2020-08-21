fn main() {
    let mut b = autocxx_build::Builder::new("main.rs").unwrap();
    b.builder().file("demo.cc")
        .flag_if_supported("-std=c++14")
        .compile("autocxx-demo");

    println!("cargo:rerun-if-changed=src/main.rs");
    println!("cargo:rerun-if-changed=src/test.h");
    println!("cargo:rerun-if-changed=src/test.cc");
}
