# Structuring a large codebase

If you have multiple modules, it may be inconvenient to generate all bindings in one `include_cpp!` invocation.
There is _limited_ support to refer from one set of bindings to another, using the `extern_cpp_type!` directive.

```rust,ignore,autocxx,hidecpp
autocxx_integration_tests::doctest(
"void handle_a(const A&) {
}
A create_a(B) {
    A a;
    return a;
}",
"#include <cstdint>
struct A {
    A() : a(0) {}
    int a;
};
enum B {
    VARIANT,
};
void handle_a(const A& a);
A create_a(B);
",
{
use autocxx::prelude::*;

pub mod base {
    autocxx::include_cpp! {
        #include "input.h"
        name!(ffi2)
        safety!(unsafe_ffi)
        generate!("A")
        generate!("B")
    }
    pub use ffi2::*;
}
pub mod dependent {
    autocxx::include_cpp! {
        #include "input.h"
        safety!(unsafe_ffi)
        generate!("handle_a")
        generate!("create_a")
        extern_cpp_type!("A", crate::base::A)
        extern_cpp_type!("B", super::super::base::B)
        pod!("B")
    }
    pub use ffi::*;
}
fn main() {
    let a = dependent::create_a(base::B::VARIANT).within_unique_ptr();
    dependent::handle_a(&a);
}
}
)
```
