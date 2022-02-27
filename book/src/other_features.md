# Other C++ features

You can make Rust subclasses of C++ classes - as these are mostly used to
implement the Observer pattern, they're documented under [calls from C++ to Rust](rust_calls.md).

## Exceptions

Exceptions are not supported. If your C++ code is compiled with exceptions,
you can expect serious runtime explosions. The underlying [cxx] crate has
exception support, so it would be possible to add them.
