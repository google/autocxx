# C++ functions

Calling C++ functions is largly as you might expect.

## Value and rvalue parameters

Functions taking [non-POD](cpp_types.md) value parameters can take a `cxx::UniquePtr<T>`
or a `&T`. This gives you the choice of Rust semantics - where a parameter
is absorbed and destroyed - or C++ semantics where the parameter is copied.

## Return values

At present, return values for [non-POD](cpp_types.md) types are always
a `cxx::UniquePtr<T>`. This is likely to change in future.

## Overloads - and identifiers ending in digits

C++ allows function overloads; Rust doesn't. `autocxx` follows the lead
of `bindgen` here and generating overloads as `func`, `func1`, `func2` etc.
This is essentially awful without `rust-analyzer` IDE support, which isn't
quite there yet.

`autocxx` doesn't yet support default paramters.

It's fairly likely we'll change the model here in the future, such that
we can pass tuples of different parameter types into a single function
implementation.
