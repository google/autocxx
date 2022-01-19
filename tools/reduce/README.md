Reduction tool for https://docs.rs/autocxx/latest/autocxx/.

Typical command-line with a repro.json and a compile error "

  cargo run --release -- --problem $EXPECTED_COMPILE_ERROR -k --creduce-arg=--n --creduce-arg=192 repro -r repro.json
