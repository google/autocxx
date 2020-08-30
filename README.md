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

let a = ffi::base::Bob::make_unique("hello".into());
a.do_a_thing();
```

The existing cxx facilities are used to allow safe ownership of C++ types from Rust; specifically things like `std::unique_ptr` and `std::string` - so the Rust code should not typically require use of unsafe code, unlike with normal `bindgen` bindings.

The macro and code generator will both need to know the include path to be passed to bindgen. At the moment, this is passed in via an
environment variable, `AUTOCXX_INC`. See the `demo/build.rs` file for details.

# How it works

It is effectively a two-stage procedural macro, which:

* First, runs `bindgen` to generate some bindings suitable for `cxx::bridge`
* Second, runs `cxx::bridge` to convert them to Rust code.

The same code can be passed through tools that generate .cc and .h bindings too:

* First, runs `bindgen` to generate some bindings suitable for `cxx::bridge` (in exactly the same way as above)
* Second, runs the codegen code from `cxx` to generate .cc and .h files

# Current state of affairs

There is an example of this macro working within the `demo` directory. At the
moment, it will work with only the very simplest functions (ints and voids only)!

The project also contains test code which does this end-to-end, for all sorts of C++ types and constructs which we eventually would like to support. They nearly all fail :)

| Type | Status |
| ---- | ------ |
| Primitives (u8, etc.) | Works |
| Plain-old-data structs | Works |
| std::unique_ptr of POD | Works |
| std::unique_ptr of std::string | Works |
| std::unique_ptr of opaque types | - |
| Methods | - |
| #defines | - |
| Constants | - |
| Enums | - |
| Structs containing UniquePtr | - |
| Structs containing strings | - |

# Build environment

This crate is not yet on crates.io and currently depends on a hacked-up version of bindgen
and a hacked-up version of cxx.

To try it out,

* Fetch the code using git.
* `cargo update`
* `cargo test --all`

This will fetch a specific fork of bindgen (see the Cargo.toml for the repo and branch) and use that as the dependency. The same applies to cxx.

At present, eight of the tests pass.

Because this uses `bindgen`, and `bindgen` may depend on the state of your system C++ headers, it is somewhat sensitive. The following known build problems exist:

* It requires Rust nightly due to `#[proc_macro_span]`
* It requires [llvm to be installed due to bindgen](https://rust-lang.github.io/rust-bindgen/print.html#requirements)
* On Linux including any system header: `bindgen` generates `pub type __uint32_t = ::std::os::raw::c_uint;` which `cxx` can't cope with. This is just a matter of munging `bindgen` more. This currently stops the demo building on my Linux box, and prevents all but one test passing.
* On Linux using `cargo` 1.47 nightly: `trybuild` is unable to pull in dependencies from git repositories because it's in offline mode. Running `cargo update` first seems to solve this.
* There's a big blocklist of STL types hard-coded. That may be quite OSX-specific or otherwise subject to different STL implementations.

# Directory structure

* `demo` - a demo example
* `engine` - all the core code. Currently a Rust library, but we wouldn't want to support
  these APIs for external users, so maybe it needs to be a directory of code symlinked
  into all the other sub-crates. All the following three sub-crates are thin wrappers
  for part of this engine. This also contains the test code.
* `macro` - the procedural macro `include_cxx` as described above.
* `gen/build` - a library to be used from `build.rs` scripts to generate .cc and .h
  files from an `include_cxx` section.
* `gen/cmd` - a command-line tool which does the same. Except this isn't written yet.
* `src` - currently, nothing.

# Next steps

Plans:

* Upstream the `cxx` change if possible.
* Then, start working on the `bindgen` fork to add support for more C++ types and see
  how far we can get through the test suite.

#### License and usage notes

This is not an officially supported Google product.

<sup>
Licensed under either of <a href="LICENSE-APACHE">Apache License, Version
2.0</a> or <a href="LICENSE-MIT">MIT license</a> at your option.
</sup>

