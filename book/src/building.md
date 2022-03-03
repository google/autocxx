# Building

Building in a `cargo` environment is explained in [the tutorial](tutorial.md).

# Configuring the build - if you're not using cargo

See the `autocxx-gen` crate. You'll need to:

* Run the `codegen` phase. You'll need to use the [`autocxx-gen`](https://crates.io/crates/autocxx-gen)
  tool to process the .rs code into C++ header and
  implementation files. This will also generate `.rs` side bindings.
* Educate the procedural macro about where to find the generated `.rs` bindings. Set the
  `AUTOCXX_RS` environment variable to a list of directories to search.
  If you use `autocxx-build`, this happens automatically. (You can alternatively
  specify `AUTOCXX_RS_FILE` to give a precise filename as opposed to a directory to search,
  though this isn't recommended unless your build system specifically requires it
  because it allows only a single `include_cpp!` block per `.rs` file.)

```mermaid
flowchart TB
    s(Rust source with include_cpp!)
    c(Existing C++ headers)
    cg(autocxx-gen or autocxx-build)
    genrs(Generated .rs file)
    gencpp(Generated .cpp and .h files)
    rsb(Rust/Cargo build)
    cppb(C++ build)
    l(Linker)
    s --> cg
    c --> cg
    cg --> genrs
    cg --> gencpp
    m(autocxx-macro)
    s --> m
    genrs-. included .->m
    m --> rsb
    gencpp --> cppb
    cppb --> l
    rsb --> l
```

This interop inevitably involves lots of fiddly small functions. It's likely to perform far better if you can achieve cross-language LTO. [This issue](https://github.com/dtolnay/cxx/issues/371) may give some useful hints - see also all the build-related help in [the cxx manual](https://cxx.rs/) which all applies here too.
