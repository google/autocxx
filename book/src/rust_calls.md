# Callbacks into Rust

`autocxx` is primarily to allow calls from Rust to C++, but like `cxx` it also allows you to expose Rust APIs to C++.

You can:
* Declare that Rust types should be available to C++ using [`extern_rust_type`](https://docs.rs/autocxx/latest/autocxx/extern_rust/attr.extern_rust_type.html)
* Make Rust functions available to C++ using [`extern_rust_function`](https://docs.rs/autocxx/latest/autocxx/extern_rust/attr.extern_rust_function.html).
* Allow Rust subclasses of C++ classes.

This latter option is most commonly used for implementing "listeners" or ["observers"](https://en.wikipedia.org/wiki/Observer_pattern), so is often in practice how C++ will call into Rust. More details below.

# Subclasses

There is limited and experimental support for creating Rust subclasses of
C++ classes. (Yes, even more experimental than all the rest of this!)
See [`subclass::CppSubclass`](https://docs.rs/autocxx/latest/autocxx/subclass/trait.CppSubclass.html) for information about how you do this.
This is useful primarily if you want to listen out for messages broadcast
using the C++ observer/listener pattern.
