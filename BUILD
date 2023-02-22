load("@rules_rust//rust:defs.bzl", "rust_library", "rust_proc_macro", "rust_binary")
load("//third-party/bazel:defs.bzl", "all_crate_deps")

# export files to be referred by third-party/BUILD.
exports_files([
    "Cargo.toml",
    "Cargo.lock",
    "engine/Cargo.toml",
    "gen/build/Cargo.toml",
    "gen/cmd/Cargo.toml",
    "integration-tests/Cargo.toml",
    "macro/Cargo.toml",
    "parser/Cargo.toml",
    "tools/mdbook-preprocessor/Cargo.toml",
    "tools/reduce/Cargo.toml",
])

rust_library(
    name = "autocxx",
    edition = "2021",
    srcs = glob(["src/**/*.rs"]),
    compile_data = glob(["README.md"]),
    deps = all_crate_deps(normal = True),
    visibility = ["//visibility:public"],
    proc_macro_deps = all_crate_deps(proc_macro = True) + [":autocxx-macro"],
)

rust_proc_macro(
    name = "autocxx-macro",
    srcs = glob(["macro/src/**/*.rs"]),
    edition = "2021",
    visibility = ["//visibility:public"],
    deps = all_crate_deps(normal = True, package_name = "macro") + [":autocxx-parser"],
)

rust_library(
    name = "autocxx-parser",
    srcs = glob(["parser/src/**/*.rs"]),
    edition = "2021",
    deps = all_crate_deps(normal = True, package_name = "parser"),
)

rust_library(
    name = "autocxx-engine",
    srcs = glob(["engine/src/**/*.rs"]),
    edition = "2021",
    visibility = ["//visibility:public"],
    crate_features = ["build"],
    deps = all_crate_deps(normal = True, package_name = "engine") + [":autocxx-parser"],
    proc_macro_deps = all_crate_deps(proc_macro = True, package_name = "engine"),
)

rust_library(
    name = "autocxx-build",
    srcs = glob(["gen/build/src/**/*.rs"]),
    edition = "2021",
    visibility = ["//visibility:public"],
    deps = all_crate_deps(normal = True, package_name = "gen/build"),
)

rust_binary(
    name = "autocxx-gen",
    srcs = glob(["gen/cmd/src/**/*.rs"]),
    edition = "2021",
    visibility = ["//visibility:public"],
    deps = all_crate_deps(normal = True, package_name = "gen/cmd") + [":autocxx-engine"],
)
