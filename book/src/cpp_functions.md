# C++ functions

Calling C++ functions is largly as you might expect.

## Value and rvalue parameters

Functions taking [non-POD](cpp_types.md) value parameters can take a `cxx::UniquePtr<T>`
or a `&T`. This gives you the choice of Rust semantics - where a parameter
is absorbed and destroyed - or C++ semantics where the parameter is copied.

```rust,ignore,autocxx,hidecpp
autocxx_integration_tests::doctest(
"
Goat::Goat() : horn_count(0) {}
void feed_goat(Goat) {}
",
"#include <cstdint>

struct Goat {
    Goat();
    uint32_t horn_count;
};

void feed_goat(Goat g); // takes goat by value
",
{
use autocxx::prelude::*;

include_cpp! {
    #include "input.h"
    safety!(unsafe_ffi)
    generate!("Goat")
    generate!("feed_goat")
}

fn main() {
    let goat = ffi::Goat::new().within_unique_ptr(); // returns a cxx::UniquePtr, i.e. a std::unique_ptr
    // C++-like semantics...
    ffi::feed_goat(&goat);
    // ... you've still got the goat!
    ffi::feed_goat(&goat);
    // Or, Rust-like semantics, where the goat is consumed.
    ffi::feed_goat(goat);
    // No goat any more...
    // ffi::feed_goat(&goat); // doesn't compile
}
}
)
```

Specifically, you can pass anything which implements [`ValueParam<T>`](https://docs.rs/autocxx/latest/autocxx/trait.ValueParam.html).

If you're keeping non-POD values on the Rust stack, you need to explicitly use [`as_mov`](https://docs.rs/autocxx/latest/autocxx/prelude/fn.as_mov.html) to indicate that you want to consume the object using move semantics:

```rust,ignore,autocxx,hidecpp
autocxx_integration_tests::doctest(
"
Blimp::Blimp() : tons_of_helium(3) {}
void burst(Blimp) {}
",
"#include <cstdint>

struct Blimp {
    Blimp();
    uint32_t tons_of_helium;
};

void burst(Blimp b); // consumes blimp,
    // but because C++ is amazing, may copy the blimp first.
",
{
use autocxx::prelude::*;

include_cpp! {
    #include "input.h"
    safety!(unsafe_ffi)
    generate!("Blimp")
    generate!("burst")
}

fn main() {
    moveit! {
        let mut blimp = ffi::Blimp::new();
    }
    ffi::burst(&*blimp); // pass by copy
    ffi::burst(as_copy(blimp.as_ref())); // explicitly say you want to pass by copy
    ffi::burst(as_mov(blimp)); // consume, using move constructor
}
}
)
```

RValue parameters are a little simpler, because (as you'd hope) they consume
the object you're passing in.

```rust,ignore,autocxx,hidecpp
autocxx_integration_tests::doctest(
"
Cake::Cake() {}
void eat(Cake&&) {}
",
"#include <cstdint>

struct Cake {
    Cake();
    uint32_t tons_of_sugar;
};

void eat(Cake&& c); // consumes the cake. You can't have your cake and eat it.
",
{
use autocxx::prelude::*;

include_cpp! {
    #include "input.h"
    safety!(unsafe_ffi)
    generate!("Cake")
    generate!("eat")
}

fn main() {
    moveit! {
        let mut stack_cake = ffi::Cake::new();
    }
    ffi::eat(stack_cake);
    // No more cake.

    // Still peckish.
    let heap_cake = ffi::Cake::new().within_unique_ptr();
    ffi::eat(heap_cake);
    // Really no more cake now.
}
}
)
```

## Default parameters

Are not yet supported[^default].

[^default]: the work is [planned here](https://github.com/google/autocxx/issues/563).

## Return values

Any C++ function which returns a [non-POD](cpp_types.md) type to Rust in fact gives you an opaque
object implementing [`moveit::New`](https://docs.rs/moveit/latest/moveit/new/trait.New.html).
This enables you to "emplace" the resulting object either on the stack or heap,
in exactly the same way as if you're constructying an object. See [the section on construction](cpp_types.md#construction)
for how to turn this opaque object into something useful (spoiler: just append `.within_unique_ptr()`).

## Overloads - and identifiers ending in digits

C++ allows function overloads; Rust doesn't. `autocxx` follows the lead
of `bindgen` here and generating overloads as `func`, `func1`, `func2` etc.
This is essentially awful without `rust-analyzer` IDE support - see the
[workflows chapter](workflow.md) for why you should be using an IDE.

```rust,ignore,autocxx,hidecpp
autocxx_integration_tests::doctest(
"
void saw(const View&) {}
void saw(const Tree&) {}",
"
#include <string>
struct View {
    std::string of_what; // go and watch In Bruges, it's great
};

struct Tree {
    int dendrochronologically_determined_age;
};

void saw(const View&);
void saw(const Tree&);
",
{
use autocxx::prelude::*;

include_cpp! {
    #include "input.h"
    safety!(unsafe_ffi)
    generate!("Tree")
    generate!("View")
    generate!("saw")
    generate!("saw1")
}

fn main() {
    let view = ffi::View::new().within_unique_ptr();
    ffi::saw(&view);
    let tree = ffi::Tree::new().within_unique_ptr();
    ffi::saw1(&tree); // yuck, overload
}
}
)
```

`autocxx` doesn't yet support default parameters.

It's fairly likely we'll change the model here in the future, such that
we can pass tuples of different parameter types into a single function
implementation.

## Methods

Calling a *const* method is simple:

```rust,ignore,autocxx,hidecpp
autocxx_integration_tests::doctest(
"",
"
class Sloth {
public:
    void sleep() const {} // sloths unchanged by sleep
};
",
{
use autocxx::prelude::*;

include_cpp! {
    #include "input.h"
    safety!(unsafe_ffi)
    generate!("Sloth")
}

fn main() {
    let sloth = ffi::Sloth::new().within_unique_ptr();
    sloth.sleep();
    sloth.sleep();
}
}
)
```

Calling a non-const method is a bit more of a pain. Per `cxx` norms, all mutable
references to C++ objects must be [pinned](https://doc.rust-lang.org/std/pin/).
In practice, this means you must call [`.pin_mut()`](https://docs.rs/cxx/latest/cxx/struct.UniquePtr.html#method.pin_mut)
every time you call a method:

```rust,ignore,autocxx,hidecpp
autocxx_integration_tests::doctest(
"",
"
class Sloth {
public:
    void unpeel_from_tree() {} // sloths get agitated when removed from
        // trees, probably shouldn't be const
};
",
{
use autocxx::prelude::*;

include_cpp! {
    #include "input.h"
    safety!(unsafe_ffi)
    generate!("Sloth")
}

fn main() {
    let mut sloth = ffi::Sloth::new().within_unique_ptr();
    sloth.pin_mut().unpeel_from_tree();
    sloth.pin_mut().unpeel_from_tree();
}
}
)
```