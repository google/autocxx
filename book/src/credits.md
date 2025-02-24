# Credits

David Tolnay did much of the hard work here, by inventing the underlying cxx crate, and in fact nearly all of the parsing infrastructure on which this crate depends. `bindgen` is also awesome. This crate stands on the shoulders of giants!

Miguel Young also did all the hard thinking about whether non-trivial C++ objects can safely exist on the Rust
stack. They can! He also draws nifty cartoons. 

Thanks to all the other contributors to cxx, bindgen and autocxx.

And thanks to [all in the Chromium community for inspiring this tool](https://www.chromium.org/Home/chromium-security/memory-safety/rust-and-c-interoperability/).