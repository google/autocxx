# Workflow

Here's how to approach autocxx:

```mermaid
flowchart TB
    %%{init:{'flowchart':{'nodeSpacing': 60, 'rankSpacing': 30}}}%%
    autocxx[Add a dependency on autocxx in your project]
    which-build([Do you use cargo?])
    autocxx--->which-build
    autocxx-build[Add a dev dependency on autocxx-build]
    build-rs[In your build.rs, tell autocxx-build about your header include path]
    autocxx-build--->build-rs
    which-build-- Yes -->autocxx-build
    macro[Add include_cpp! macro: list headers and allowlist]
    build-rs--->macro
    autocxx-gen[Use autocxx-gen command line tool]
    which-build-- No -->autocxx-gen
    autocxx-gen--->macro
    build[Build]
    macro--->build
    check[Confirm generation using cargo expand]
    build--->check
    manual[Add manual cxx::bridge for anything missing]
    check--->manual
    use[Use generated ffi mod APIs]
    manual--->use
```

# Did it work? How do I deal with failure?

Once you've achieved a successful build, you might wonder how to know what
bindings have been generated. `cargo expand` will show you. Alternatively,
you can get autocompletion within an IDE supported by Rust analyzer. You'll
need to enable _both_:
* Rust-analyzer: Proc Macro: Enable
* Rust-analyzer: Experimental: Proc Attr Macros

Either way, you'll find (for sure!) that `autocxx` hasn't been able to generate
bindings for all your C++ APIs. This may manifest as a hard failure or a soft
failure:
* If you specified such an item in a [`generate`] directive (or similar such
  as [`generate_pod`]) then your build will fail.
* If such APIs are methods belonging to a type, `autocxx` will generate other
  methods for the type but ignore those.

In this latter case, you should see helpful messages _in the generated bindings_
as rust documentation explaining what went wrong.

If this happens (and it will!) your options are:
* Add more, simpler C++ APIs which fulfil the same need but are compatible with
  `autocxx`.
* Write manual bindings. This is most useful if a type is supported by [cxx]
  but not `autocxx` (for example, at the time of writing `std::array`). See
  the later section on 'combinining automatic and manual bindings'.

# Mixing manual and automated bindings

`autocxx` uses [cxx] underneath, and its build process will happily spot and
process and manually-crafted [`cxx::bridge`] mods which you include in your
Rust source code. A common pattern good be to use `autocxx` to generate
all the bindings possible, then hand-craft a [`cxx::bridge`] mod for the
remainder where `autocxx` falls short.

To do this, you'll need to use the [ability of one cxx::bridge mod to refer to types from another](https://cxx.rs/extern-c++.html#reusing-existing-binding-types),
for example:

```rust,ignore
autocxx::include_cpp! {
    #include "foo.h"
    safety!(unsafe_ffi)
    generate!("take_A")
    generate!("A")
}
#[cxx::bridge]
mod ffi2 {
    unsafe extern "C++" {
        include!("foo.h");
        type A = crate::ffi::A;
        fn give_A() -> UniquePtr<A>; // in practice, autocxx could happily do this
    }
}
fn main() {
    let a = ffi2::give_A();
    assert_eq!(ffi::take_A(&a), autocxx::c_int(5));
}
```