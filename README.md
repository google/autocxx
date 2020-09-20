# Autocxx

[Docs.rs](https://docs.rs/autocxx) [Crates.io](https://crates.io/crates/autocxx)

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
| Typedefs | - |
| Structs containing UniquePtr | Works |
| Structs containing strings | Works (opaque only) |
| Passing opaque structs (owned by UniquePtr) into C++ APIs which take them by value | - |
| Constructors/make_unique | Sort of. A function called {type}_make_unique is generated which is not the eventual intended syntax. |
| Field access to opaque objects via UniquePtr | - |
| Reference counting | - |
| std::optional | - |
| Function pointers | - |
| Unique ptrs to primitives | - |
| Inheritance from pure virtual classes | - |
| Destructors | Works via cxx `UniquePtr` already |
| Namespaces | - |

The plan is (roughly) to work through the above list of features. Some are going to be _very_ hard, and it's not at all clear that a plan will present itself. In particular, some will require that C++ structs are owned by `UniquePtr` yet passed to C++ by value. It's not clear how ergonomic the results will be. Until we are much further, I don't advise using this for anything in production.

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

Because this uses `bindgen`, and `bindgen` may depend on the state of your system C++ headers, it is somewhat sensitive. The following known build problems exist:

* It requires [llvm to be installed due to bindgen](https://rust-lang.github.io/rust-bindgen/print.html#requirements)
* Two of the tests fail when built against some STLs due to a problem where bindgen generates `type-parameter-0-0` which is not a valid identifier.

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
* `gen/build` - a library to be used from `build.rs` scripts to generate .cc and .h
  files from an `include_cxx` section.
* `gen/cmd` - a command-line tool which does the same. Except this isn't written yet.
* `src` (outermost project)- the procedural macro `include_cxx` as described above.

# Credits

David Tolnay did much of the hard work here, by inventing the underlying cxx crate, and in fact nearly all of the parsing infrastructure on which this crate depends. `bindgen` is also awesome. This crate stands on the shoulders of giants!

#### License and usage notes

This is not an officially supported Google product.

<sup>
Licensed under either of <a href="LICENSE-APACHE">Apache License, Version
2.0</a> or <a href="LICENSE-MIT">MIT license</a> at your option.
</sup>

