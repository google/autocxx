# Tutorial

Example:

```rust,autocxx
autocxx_integration_tests::doctest(
"",
"#include <stdint.h>
inline uint32_t do_math() { return 3; }",
{
use autocxx::include_cpp;

include_cpp! {
    #include "input.h"
    safety!(unsafe_ffi)
    generate!("do_math")
}

fn main() {
    ffi::do_math();
}
}
)
```


Another example:


```rust,autocxx
autocxx_integration_tests::doctest(
"",
"#include <stdint.h>
inline uint32_t do_math() { return 55; }",
{
use autocxx::include_cpp;

include_cpp! {
    #include "input.h"
    safety!(unsafe_ffi)
    generate!("do_math")
}

fn main() {
    ffi::do_math();
}
}
)
```
