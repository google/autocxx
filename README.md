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
use autocxx::include_cxx;

include_cxx!(
    Header("base/bob.h"),
    Allow("Bob"),
)

let a = ffi::base::bob::make_unique("hello".into());
a.do_a_thing();
```

The existing cxx facilities are used to allow safe ownership of C++ types from Rust; specifically things like `std::unique_ptr` and `std::string` - so the Rust code should
not typically require use of unsafe code, unlike with normal `bindgen` bindings.

# How it works

It is effectively a two-stage procedural macro, which:

* First, runs `bindgen` to generate some bindings suitable for `cxx::bridge`
* Second, runs `cxx::bridge` to convert them to Rust code.

The same code can be passed through tools that generate .cc and .h bindings too:

* First, runs `bindgen` to generate some bindings suitable for `cxx::bridge` (in exactly the same way as above)
* Second, runs the codegen code from `cxx` to generate .cc and .h files

# Current state of affairs

There is an example of this macro working within the `demo` directory. At the
moment, it will work with only the very simplest functions.

The project also contains test code which does this end-to-end, for all sorts of C++ types and constructs which we eventually would like to support. They nearly all fail :)

# Build environment

This crate is not yet on crates.io and currently depends on a hacked-up version of bindgen
and a hacked-up version of cxx.

To try it out,

* Fetch the code using git.
* `cargo test -- --test-threads=1`.

This will fetch a specific fork of bindgen (see the Cargo.toml for the repo and branch) and use that as the dependency. The same applies to cxx.

At present, many of the tests fail, and thus the test run fails overall. Individual tests can be run, and some pass.

For some reason, more tests fail if they run in parallel, hence why you should use `cargo test -- --test-threads=1`.

# Directory structure

* `demo` - a demo example
* `engine` - all the core code. Currently a Rust library, but we wouldn't want to support
  these APIs for external users, so maybe it needs to be a directory of code symlinked
  into all the other sub-crates. All the following three sub-crates are thin wrappers
  for part of this engine.
* `macro` - the procedural macro `include_cxx` as described above.
* `gen/build` - an object to be used from `build.rs` scripts to generate .cc and .h
  files from an `include_cxx` section.
* `gen/cmd` - a command-line tool which does the same. Except this isn't written yet.
* `src` - currently, test code.

# Next steps

Plans:

* Rationalize the test code so it uses more of the facilities from `engine` (which didn't
  mostly exist when I wrote the test code)
* Upstream the `cxx` change if possible.
* Fix a few of the annoying TODOs (the oddest one being in `demo/build.rs`)
* Then, start working on the `bindgen` fork to add support for more C++ types and see
  how far we can get through the test suite.

#### License and usage notes

This is not an officially supported Google product.

<sup>
Licensed under either of <a href="LICENSE-APACHE">Apache License, Version
2.0</a> or <a href="LICENSE-MIT">MIT license</a> at your option.
</sup>

