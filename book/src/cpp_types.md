# C++ structs, enums and classes

If you add a C++ struct, class or enum to the allowlist, Rust bindings will be generated to that type and to any methods it has.
Even if you don't add it to the allowlist, the type may be generated if it's required by some other function - but in this case
all its methods won't be generated.

Rust and C++ differ in an important way:

* In Rust, the compiler is free to pick up some data and move it to somewhere else (in a `memcpy` sense). The object is none the wiser.
* In C++, once created, an object stays where it is, until or unless it has its "move constructor" invoked.

This makes a big difference: C++ objects can have self-referential pointers, and any such pointer would be invalidated by Rust doing
a memcpy. Such self-referential pointers are common - even some implementations of `std::string` do it.

## POD and non-POD

When asking `autocxx` to generate bindings for a type, then, you have to make a choice.

* *This C++ type is trivial*. It has no destructor or move constructor (or they're trivial), and thus Rust is free to move it around the stack as it wishes. `autocxx` calls these types POD ("plain old data"). Alternatively,
* *This C++ type has a non-trivial destructor or move constructor, so we can't allow Rust to move this around*. `autocxx` calls these types non-POD.

POD types are nicer:

* You can just use them as regular Rust types.
* You get direct field access.
* No funny business.

Non-POD types are awkward:

* You can't just _have_ one as a Rust variable. Normally you hold them in a [`cxx::UniquePtr`](https://docs.rs/cxx/latest/cxx/struct.UniquePtr.html), though there are other options.
* There is no access to fields (yet).
* You can't even have a `&mut` reference to one, because then you might be able to use [`std::mem::swap`](https://doc.rust-lang.org/stable/std/mem/fn.swap.html) or similar. You can have a `Pin<&mut>` reference, which is more fiddly.

By default, `autocxx` generates non-POD types. You can request a POD type using [`generate_pod!`](https://docs.rs/autocxx/latest/autocxx/macro.generate_pod.html). Don't worry: you can't mess this up. If the C++ type doesn't in fact comply with the requirements for a POD type, your build will fail thanks to some static assertions generated in the C++.

See [the chapter on storage](storage.md) for lots more detail on how you can hold onto non-POD types.

## Construction

Each constructor results in _two_ Rust functions.

* A `new` function exists, which
  offers a constructor per the standards of the `moveit` crate, and thus can be used
  to place the object on the Rust stack (as well as in various containers such as `Box`
  and `UniquePtr`)
* A `make_unique` function is also created, which constructs the item directly into
  a `cxx::UniquePtr`. This is more commonly what you want.

```rust,ignore,autocxx,hidecpp
autocxx_integration_tests::doctest(
"void A::set(uint32_t val) { a = val; }
uint32_t A::get() const { return a; }",
"#include <stdint.h>
#include <string>
struct A {
    A() {}
    void set(uint32_t val);
    uint32_t get() const;
    uint32_t a;
};
",
{
use autocxx::prelude::*;

include_cpp! {
    #include "input.h"
    safety!(unsafe_ffi)
    generate!("A")
}

fn main() {
    moveit! {
        let mut stack_obj = ffi::A::new();
    }
    stack_obj.as_mut().set(42);
    assert_eq!(stack_obj.get(), 42);

    let mut heap_obj = ffi::A::make_unique();
    heap_obj.pin_mut().set(42);
    assert_eq!(heap_obj.get(), 42);
}
}
)
```

## Forward declarations

A type which is incomplete in the C++ headers (i.e. represented only by a forward
declaration) can't be held in a `UniquePtr` within Rust (because Rust can't know
if it has a destructor that will need to be called if the object is `Drop`ped.)
Naturally, such an object can't be passed by value either; it can still be
referenced in Rust references.

## Generic types

If you're using one of the generic types which is supported natively by cxx,
e.g. `std::unique_ptr`, it should work as you expect. For other generic types,
we synthesize a concrete Rust type, corresponding to a C++ typedef, for each
concrete instantiation of the type. Such generated types are always opaque,
and never have methods attached. That's therefore enough to pass them
between return types and parameters of other functions within [`cxx::UniquePtr`](https://docs.rs/cxx/latest/cxx/struct.UniquePtr.html)s
but not really enough to do anything else with these types yet.

To make them more useful, you might have to add extra C++ functions to extract
data or otherwise deal with them.