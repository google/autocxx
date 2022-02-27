# Built-in types

`autocxx` relies heavily on the [standard cxx types](https://cxx.rs/bindings.html).

There are a few additional integer types, such as [`c_int`](https://docs.rs/autocxx/latest/autocxx/struct.c_int.html),
which are not yet upstreamed to `cxx`. These are to support those pesky C/C++ integer types
which do not have a predictable number of bits on different machines.

```rust,ignore,autocxx
autocxx_integration_tests::doctest(
"",
"inline int do_math(int a, int b) { return a+b; }",
{
use autocxx::prelude::*;

include_cpp! {
    #include "input.h"
    safety!(unsafe_ffi)
    generate!("do_math")
}

fn main() {
    assert_eq!(ffi::do_math(c_int(12), c_int(13)), c_int(25));
}
}
)
```