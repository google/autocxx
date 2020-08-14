# Autocxx

This project is a tool for calling C++ from Rust in a heavily automated, but safe, fashion.

The intention is that it has all the fluent safety from [cxx](https://github.com/dtolnay/cxx) whilst generating interfaces automatically from existing C++ headers using a variant of [bindgen](https://docs.rs/bindgen/0.54.1/bindgen/). Think of autocxx as glue which plugs bindgen into cxx.

# Intended eventual interface

It's intended that eventually this exposes a single procedural macro, something like this:

```cpp
class Bob {
public:
    Bob(std::string name);
    ...
    void do_a_thing();
}
```

```rust
use autocxx::include_cpp;

include_cpp!(
    Header("base/bob.h"),
    Allow("Bob"),
)

let a = ffi::base::bob::make_unique("hello".into());
a.do_a_thing();
```

The existing cxx facilities are used to allow safe ownership of C++ types from Rust; specifically things like `std::unique_ptr` and `std::string`.

# Current state of affairs

At present, this macro does not work, for two reasons:
*  because there is no equivalent of the `cxxbridge` tool that knows how to expand the `include_cpp!` macro into a `cxx::bridge` mod (which in turn is then used to generate `.cc` and `.h` files.)
* lots of nasty hard-coded nonsense around include paths etc.

However, this project does contain test code which does this end-to-end. At present, three of the many tests actually pass, proving that for the simplest imaginable case it is possible to tie `bindgen` into `cxx`.

# Build environment

This crate is not yet on crates.io and currently depends on a hacked-up version of bindgen
and a hacked-up version of cxx.

To try it out,

* Fetch the code using git.
* cargo test

This will fetch a specific fork of bindgen (see the Cargo.toml for the repo and branch) and use that as the dependency. The same applies to cxx.

At present, many of the tests fail, and thus the test run fails overall. Individual tests can be run, and some pass.

For some reason, more tests fail if they run in parallel, so use `cargo test -- --test-threads=1`.

# Caveats

This is not an officially supported Google product.

#### License

<sup>
Licensed under either of <a href="LICENSE-APACHE">Apache License, Version
2.0</a> or <a href="LICENSE-MIT">MIT license</a> at your option.
</sup>

