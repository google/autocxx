# Building

Building in a `cargo` environment is explained in [the tutorial](tutorial.md).

# Building without cargo

See instructions in the documentation for [`include_cpp`](https://docs.rs/autocxx/latest/autocxx/macro.include_cpp.html). This interop inevitably involves lots of fiddly small functions. It's likely to perform far better if you can achieve cross-language LTO. [This issue](https://github.com/dtolnay/cxx/issues/371) may give some useful hints - see also all the build-related help in [the cxx manual](https://cxx.rs/) which all applies here too.
