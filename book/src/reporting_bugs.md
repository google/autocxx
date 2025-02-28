
## Reporting bugs

If you've found a problem, and you're reading this, *thank you*! Your diligence
in reporting the bug is much appreciated and will make `autocxx` better.

However, we get a lot of incoming bugs, and we are limited by our ability
to triage them. Please help us by making a good bug report.

Much of this is about minimizing the size of your test case so it does not
depend on your system's C++ headers or any copyrighted code. To minimize the
test case, you can use `tools/reduce`, something like this:

`cd tools/reduce; cargo run -- file -d "safety!(unsafe_ffi)" -d
'generate_pod!("A")' -I ~/my-include-dir -h my-header.h -p
problem-error-message -- --remove-pass pass_line_markers`
This is a wrapper for the amazing `creduce` which will take thousands of lines
of C++, preprocess it, and then identify the minimum required lines to
reproduce the same problem. This can sometimes take _days_ to run - that's
normal - if this is a problem, try to minimize the test case manually, or
do a combination.

Once you have a minimal test case:

* First see if it triggers a problem when you're using pure `bindgen`.
  autocxx is very dependent on `bindgen` generating correct code.
  To do this, fetch the [`bindgen` code](https://github.com/rust-lang/rust-bindgen)
  then run
  `cargo run -- XYZ.hpp --no-layout-tests --enable-cxx-namespaces --allowlist-type ABC-- -std=c++14`
  `XYZ.hpp` is your header (the extension is important) and `ABC` is the type
  you wish to generate. Assuming you minimized your test case in advance,
  this should generate just a few lines of Rust code and you can inspect it
  (or even build it) to see if it seems to be problematic.
* If the problem is in `bindgen`, report it to `bindgen`.
* If the problem is in `autocxx`, please report a bug to us. In order of preference:
  * Raise a pull request adding a new minimized failing integration test to
    [`integration_test.rs`](https://github.com/google/autocxx/blob/main/integration-tests/tests/integration_test.rs)
  * Simply raise an issue with the minimized code.

If you can't get minimization to work:
* Use the C++ preprocessor to give a single complete C++ file which demonstrates
  the problem, along with the `include_cpp!` directive you use.
  Alternatively, run your build using `AUTOCXX_REPRO_CASE=repro.json` which should
  put everything we need into `output.h`. If necessary, you can use the `CLANG_PATH`
  or `CXX` environment variables to specify the path to the Clang compiler to use.
* Failing all else, build using
  `cargo clean -p <your package name> && RUST_LOG=autocxx_engine=info cargo build -vvv`
  and send the _entire_ log to us. This will include two key bits of logging:
  the C++ bindings as distilled by `bindgen`, and then the version which
  we've converted and moulded to be suitable for use by `cxx`.

## Bugs related to linking problems

Unfortunately, _linking_ C++ binaries is a complex area subject in itself, and
we won't be able to debug your linking issues by means of an autocxx bug report.
Assuming you're using autocxx's build.rs support, the actual C++ build and
managed by the [`cc`](https://crates.io/crates/cc) crate. You can find
many of its options on its [`Build` type](https://docs.rs/cc/latest/cc/struct.Build.html).
If you need to bring in an external library, you may also need to emit certain
[print statements from your `build.rs`](https://doc.rust-lang.org/cargo/reference/build-scripts.html#rustc-link-lib)
to instruct cargo to link against that library.