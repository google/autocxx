# C++ functions




## Overloads - and identifiers ending in digits

C++ allows function overloads; Rust doesn't. `autocxx` follows the lead
of `bindgen` here and generating overloads as `func`, `func1`, `func2` etc.
This is essentially awful without `rust-analyzer` IDE support, which isn't
quite there yet.

`autocxx` doesn't yet support default paramters.

It's fairly likely we'll change the model here in the future, such that
we can pass tuples of different parameter types into a single function
implementation.