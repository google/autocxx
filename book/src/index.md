[![GitHub](https://img.shields.io/crates/l/autocxx)](https://github.com/google/autocxx)
[![crates.io](https://img.shields.io/crates/d/autocxx)](https://crates.io/crates/autocxx)
[![docs.rs](https://docs.rs/autocxx/badge.svg)](https://docs.rs/autocxx)

# autocxx — automatic safe interop between Rust and C++

`autocxx` is like `bindgen`, in that it enables you to use C++ functions and types from within Rust. It automates a lot of the fiddly things you need to do with `bindgen` bindings: calling destructors, converting strings, unsafely handling raw pointers. C++ functions and types within `autocxx` bindings should behave naturally and ergonomically, almost as if they were safe Rust functions and types themselves. These ergonomics and safety improvements come from the [cxx](https://cxx.rs) project - hence the name of this tool, `autocxx`.

`autocxx` combines the safety and ergonomics of `cxx` with the automatic bindings generation of `bindgen`.

