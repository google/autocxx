# C++ Exceptions

C++ exceptions are supported via the `throws!` directive. When a function is marked
with `throws!`, its Rust binding returns `Result<T, cxx::Exception>` instead of `T`,
allowing you to handle C++ exceptions that propagate across the FFI boundary.

## Basic usage

```rust,ignore,autocxx,hidecpp
autocxx_integration_tests::doctest(
"
#include <stdexcept>
void do_risky_thing() {
    throw std::runtime_error(\"something went wrong\");
}
",
"
#include <stdexcept>
void do_risky_thing();
",
{
use autocxx::prelude::*;

include_cpp! {
    #include "input.h"
    safety!(unsafe_ffi)
    generate!("do_risky_thing")
    throws!("do_risky_thing")
}

fn main() {
    let result = ffi::do_risky_thing();
    assert!(result.is_err());
    // You can access the exception message:
    // println!("Error: {}", result.unwrap_err());
}
}
)
```

## Functions with return values

Functions that return values work the same way - the return type becomes
`Result<T, cxx::Exception>`:

```rust,ignore,autocxx,hidecpp
autocxx_integration_tests::doctest(
"
#include <stdexcept>
#include <cstdint>
uint32_t parse_number(const char* s) {
    if (!s || !*s) throw std::runtime_error(\"empty string\");
    return atoi(s);
}
",
"
#include <stdexcept>
#include <cstdint>
uint32_t parse_number(const char* s);
",
{
use autocxx::prelude::*;

include_cpp! {
    #include "input.h"
    safety!(unsafe_ffi)
    generate!("parse_number")
    throws!("parse_number")
}

fn main() {
    // Successful call - unwrap the Result
    let value = ffi::parse_number(c"42".as_ptr()).unwrap();
    assert_eq!(value, 42);

    // Exception is caught and converted to Err
    let result = ffi::parse_number(std::ptr::null());
    assert!(result.is_err());
}
}
)
```

## Qualified names

The `throws!` directive supports qualified names for precise control over which
functions are marked as throwing:

| Pattern | Matches |
|---------|---------|
| `throws!("do_something")` | Any function named `do_something` |
| `throws!("MyClass::method")` | Method `method` on class `MyClass` |
| `throws!("ns::do_something")` | Function `do_something` in namespace `ns` |
| `throws!("ns::MyClass::method")` | Method on a namespaced class |

### Partial matching

Partial matching is supported: a shorter pattern will match functions in any
namespace. For example, `throws!("do_something")` will match both a top-level
`do_something` and `my_namespace::do_something`.

```rust,ignore,autocxx,hidecpp
autocxx_integration_tests::doctest(
"
#include <stdexcept>
namespace utils {
    void validate() {
        throw std::runtime_error(\"validation failed\");
    }
}
",
"
#include <stdexcept>
namespace utils {
    void validate();
}
",
{
use autocxx::prelude::*;

include_cpp! {
    #include "input.h"
    safety!(unsafe_ffi)
    generate!("utils::validate")
    throws!("validate")  // matches utils::validate
}

fn main() {
    let result = ffi::utils::validate();
    assert!(result.is_err());
}
}
)
```

## How it works

Under the hood, `autocxx` leverages [cxx's native exception handling](https://cxx.rs/binding/result.html).
When a function is marked with `throws!`, the generated cxx bridge declaration
uses `Result<T>` as the return type. This causes cxx to automatically wrap
the C++ call in a try-catch block and convert any caught `std::exception`
(or derived types) to `cxx::Exception`.

The `cxx::Exception` type provides:
- `Display` implementation to get the exception message (`what()`)
- Conversion to `std::io::Error` via `From` trait

## Limitations

* **Constructors**: Throwing constructors are not currently supported due to
  the complexity of `moveit::new::New` return types. If you need to handle
  constructor failures, consider using a factory function instead.

* **Non-std::exception types**: Only exceptions derived from `std::exception`
  are caught and converted. Other thrown types (like integers or strings)
  will still cause undefined behavior.

* **Performance**: Exception handling adds minimal runtime overhead - the
  cost is only incurred when an exception actually occurs.
