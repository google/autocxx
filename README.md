# Autocxx

[Docs.rs](https://docs.rs/autocxx) [Crates.io](https://crates.io/crates/autocxx)

This project is a tool for calling C++ from Rust in a heavily automated, but safe, fashion.

The intention is that it has all the fluent safety from [cxx](https://github.com/dtolnay/cxx) whilst generating interfaces automatically from existing C++ headers using a variant of [bindgen](https://docs.rs/bindgen/0.54.1/bindgen/). Think of autocxx as glue which plugs bindgen into cxx.

# Overview

```cpp
namespace base {
  class Bob {
  public:
      Bob(std::string name);
      ...
      void do_a_thing() const;
  };
}
```

```rust
use autocxx::include_cpp;

include_cpp!(
    #include "base/bob.h"
    generate!("Bob")
)

let a = ffi::base::Bob::make_unique("hello".into());
a.do_a_thing();
```

See [demo/src/main.rs](demo/src/main.rs) for a real example.

The existing cxx facilities are used to allow safe ownership of C++ types from Rust; specifically things like `std::unique_ptr` and `std::string` - so the Rust code should not typically require use of unsafe code, unlike with normal `bindgen` bindings.

The macro and code generator will both need to know the include path to be passed to bindgen. At the moment, this is passed in via an
environment variable, `AUTOCXX_INC`. See the [demo/build.rs](demo/build.rs) file for details.

# How it works

It is effectively a three-stage procedural macro, which:

* First, runs `bindgen` to generate some bindings (with all the usual `unsafe`, `#[repr(C)]` etc.)
* Second, interprets and converts them to bindings suitable for `cxx::bridge`.
* Thirdly, runs `cxx::bridge` to convert them to Rust code.

The same code can be passed through tools that generate .cc and .h bindings too:

* First, runs `bindgen` to generate some bindings (with all the usual `unsafe`, `#[repr(C)]` etc.) - in exactly the same way as above.
* Second, interprets and converts them to bindings suitable for `cxx::bridge` - in the same way as above.
* Thirdly, runs the codegen code from `cxx` to generate .cc and .h files

# Current state of affairs

There is an example of this macro working within the `demo` directory.

The project also contains test code which does this end-to-end, for all sorts of C++ types and constructs which we eventually would like to support.

| Type | Status |
| ---- | ------ |
| Primitives (u8, etc.) | Works |
| Plain-old-data structs | Works |
| std::unique_ptr of POD | Works |
| std::unique_ptr of std::string | Works |
| std::unique_ptr of opaque types | - |
| Reference to POD | Works |
| Reference to std::string | Works |
| Classes | Works, but with warnings |
| Methods | Works (classes give warnings) |
| Int #defines | Works |
| String #defines | Works |
| Primitive constants | Works |
| Enums | Works, though more thought needed |
| #ifdef, #if etc. | - |
| Typedefs | Infinite permutations, some of which work |
| Structs containing UniquePtr | Works |
| Structs containing strings | Works (opaque only) |
| Passing opaque structs (owned by UniquePtr) into C++ functions which take them by value | Works |
| Passing opaque structs (owned by UniquePtr) into C++ methods which take them by value | Works, but with non-ideal syntax |
| Constructors/make_unique | Works, though probably many problems |
| Destructors | Works via cxx `UniquePtr` already |
| Inline functions | Works |
| Construction of std::unique_ptr<std::string> in Rust | Works |
| Namespaces | Works, but a known limitation |
| Field access to opaque objects via UniquePtr | - |
| Plain-old-data structs containing opaque fields | Impossible by design, but may not be ergonomic so may need more thought |
| Reference counting, std::shared_ptr | - |
| std::optional | - |
| Function pointers | - |
| Unique ptrs to primitives | - |
| Inheritance from pure virtual classes | - |
| Generic (templated) types | - |

The plan is (roughly) to work through the above list of features and fix corner cases. This project is deliberately incremental. There are open questions about whether the end result is ergonomic and performant: specifically, whether it's acceptable to hold opaque C++ types always by `UniquePtr` in Rust. Until we know more, this project is considered experimental and we don't advise using it for anything in production.

# On safety

This crate mostly intends to follow the lead of the `cxx` crate in where and when `unsafe` is required (it doesn't currently do that, because `cxx` has recently changed policy). But, this crate is opinionated. It believes some unsafety requires more careful review than other bits, along the following spectrum:

* Rust unsafe code (requires most review)
* Rust code calling C++ with raw pointers
* Rust code calling C++ with shared pointers, or anything else where there can be concurrent mutation
* Rust code calling C++ with unique pointers, where the Rust single-owner model nearly always applies (but we can't _prove_ that the C++ developer isn't doing something weird)
* Rust safe code (requires least review)

If your project is 90% Rust code, with small bits of C++, don't use this crate. You need something where all C++ interaction is marked with big red "this is terrifying" flags. This crate is aimed at cases where there's 90% C++ and small bits of Rust, and so we want the Rust code to be pragmatically reviewable without the signal:noise ratio of `unsafe` in the Rust code becoming so bad that `unsafe` loses all value.

It's not clear yet how this opinion will play out in practice. Perhaps we will get something like `unsafe(ffi)` or `might_be_a_bit_unsafe` (joking!) All that is yet to be determined.

# Build environment

Because this uses `bindgen`, and `bindgen` may depend on the state of your system C++ headers, it is somewhat sensitive. It requires [llvm to be installed due to bindgen](https://rust-lang.github.io/rust-bindgen/requirements.html)

As with `cxx`, this generates both Rust and C++ side bindings code. The Rust code is simply
and transparently generated at build time by the `include_cpp!` procedural macro. But you'll
need to take steps to generate the C++ code: either by using the `build.rs` integration within
`autocxx_build`, or the command line utility within `autocxx_gen`.

# Configuring the build

This runs `bindgen` within a procedural macro. There are limited opportunities to pass information into procedural macros, yet bindgen needs to know a lot about the build environment.

The plan is:
* The Rust code itself will specify the include file(s) and allowlist by passing them into the macro. This is the sort of thing that developers within an existing C++ codebase would specify in C++ (give or take) so it makes sense for it to be specific in the Rust code.
* However, all build settings (e.g. bindgen compiler configuration, include path etc.) will be passed into the macro by means of environment variables. The build environment should set these before building the code. (An alternative means will be provided to pass these into the C++ code generator tools.)

# Directory structure

* `demo` - a demo example
* `engine` - all the core code. Currently a Rust library, but we wouldn't want to support
  these APIs for external users, so maybe it needs to be a directory of code symlinked
  into all the other sub-crates. All the following three sub-crates are thin wrappers
  for part of this engine. This also contains the test code.
* `macro` - the procedural macro which expands the Rust code.
* `gen/build` - a library to be used from `build.rs` scripts to generate .cc and .h
  files from an `include_cxx` section.
* `gen/cmd` - a command-line tool which does the same.
* `src` (outermost project) - a wrapper crate which imports the procedural macro and
  a few other things.

# Where to start reading

The main algorithm is in `engine/src/lib.rs`, in the function `generate()`. This asks
`bindgen` to generate a heap of Rust code and then passes it into
`engine/src/conversion` to convert it to be a format suitable for input
to `cxx`.

However, most of the actual code is in `engine/src/conversion/mod.rs`.

At the moment we're using a slightly branched version of `bindgen` called `autocxx-bindgen`.
It's hoped this is temporary; some of our changes are sufficiently weird that it would be presumptious to try to get them accepted upstream until we're sure `autocxx` has roughly the right approach.

# How to develop

If you're making a change, here's what you need to do to get useful diagnostics etc.
First of all, `cargo run` in the `demo` directory. If it breaks, you don't get much
in the way of useful diagnostics, because `stdout` is swallowed by cargo build scripts.
So, practically speaking, you would almost always move onto running one of the tests
in the test suite. With suitable options, you can get plenty of output. For instance:

```
RUST_BACKTRACE=1 RUST_LOG=autocxx_engine=info cargo test  integration_tests::test_cycle_string_full_pipeline -- --nocapture
```

This is especially valuable to see the `bindgen` output Rust code, and then the converted Rust code which we pass into cxx. Usually, most problems are due to some mis-conversion somewhere
in `engine/src/conversion`.

# Credits

David Tolnay did much of the hard work here, by inventing the underlying cxx crate, and in fact nearly all of the parsing infrastructure on which this crate depends. `bindgen` is also awesome. This crate stands on the shoulders of giants!

#### License and usage notes

This is not an officially supported Google product.

<sup>
Licensed under either of <a href="LICENSE-APACHE">Apache License, Version
2.0</a> or <a href="LICENSE-MIT">MIT license</a> at your option.
</sup>
