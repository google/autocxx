fn main() {
    // It's necessary to use an absolute path here because the
    // C++ codegen and the macro codegen appears to be run from different
    // working directories.
    let path = std::path::PathBuf::from("src").canonicalize().unwrap();
    std::env::set_var("AUTOCXX_INC", &path);
    let mut b = autocxx_build::Builder::new("src/main.rs").unwrap();
    b.builder()
        .flag_if_supported("-std=c++14")
        .compile("autocxx-demo");

    println!("cargo:rerun-if-changed=src/main.rs");
    println!("cargo:rerun-if-changed=src/input.h");
    println!("cargo:rustc-env=AUTOCXX_INC={}", path.to_str().unwrap());
}
