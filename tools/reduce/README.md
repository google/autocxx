Reduction tool for https://docs.rs/autocxx/latest/autocxx/.

Typical command-line with a repro.json and a compile error "

```
  cargo run --release -- --problem $EXPECTED_COMPILE_ERROR -k --creduce-arg=--n --creduce-arg=192 repro -r repro.json
```

From Chromium,

```
  CLANG_PATH=~/chromium/src/third_party/llvm-build/Release+Asserts/bin/clang++ AUTOCXX_REPRO_CASE=repro.json autoninja -C out/Release chrome
  CLANG_PATH=~/chromium/src/third_party/llvm-build/Release+Asserts/bin/clang++ cargo run --release -- --problem $EXPECTED_COMPILE_ERROR -k --clang-arg=-std=c++17 --creduce-arg=--n --creduce-arg=192 repro -r ~/dev/chromium/src/out/Release/repro.json
```
