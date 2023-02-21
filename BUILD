load("@rules_rust//rust:defs.bzl", "rust_library", "rust_proc_macro", "rust_binary")

rust_library(
    name = "autocxx",
    edition = "2021",
    srcs = glob(["src/**/*.rs"]),
    data = glob(["README.md"]),
    deps = [
        "//third-party:cxx",
        "//third-party:moveit",
    ],
    visibility = ["//visibility:public"],
    proc_macro_deps = [
	":autocxx-macro",
        "//third-party:aquamarine",
    ],
)

rust_proc_macro(
    name = "autocxx-macro",
    srcs = glob(["macro/src/**/*.rs"]),
    edition = "2021",
    visibility = ["//visibility:public"],
    deps = [
	":autocxx-parser",
        "//third-party:proc-macro-error",
        "//third-party:proc-macro2",
        "//third-party:quote",
        "//third-party:syn",
    ],
)

rust_library(
    name = "autocxx-parser",
    srcs = glob(["parser/src/**/*.rs"]),
    edition = "2021",
    deps = [
        "//third-party:log",
        "//third-party:proc-macro2",
        "//third-party:quote",
        "//third-party:serde",
        "//third-party:thiserror",
        "//third-party:once_cell",
        "//third-party:itertools",
        "//third-party:indexmap",
        "//third-party:serde_json",
        "//third-party:syn",
    ],
)

rust_library(
    name = "autocxx-engine",
    srcs = glob(["engine/src/**/*.rs"]),
    compile_data = glob(["**/*.md", "include/cxx.h"]),
    edition = "2021",
    visibility = ["//visibility:public"],
    crate_features = ["build"],
    deps = [
        "//third-party:log",
        "//third-party:proc-macro2",
        "//third-party:quote",
	"//third-party:autocxx-bindgen",
        "//third-party:itertools",
        "//third-party:cc",
        "//third-party:cxx-gen",
	":autocxx-parser",
        "//third-party:version_check",
        "//third-party:tempfile",
        "//third-party:once_cell",
        "//third-party:serde_json",
        "//third-party:miette",
        "//third-party:thiserror",
        "//third-party:regex",
        "//third-party:indexmap",
        "//third-party:prettyplease",
        "//third-party:syn",
    ],
    proc_macro_deps = [
        "//third-party:indoc",
        "//third-party:aquamarine",
        "//third-party:strum_macros",
        "//third-party:rustversion",
    ],
)

rust_library(
    name = "autocxx-build",
    srcs = glob(["gen/build/src/**/*.rs"]),
    edition = "2021",
    visibility = ["//visibility:public"],
    deps = [
        ":autocxx-engine",
        "//third-party:env_logger",
        "//third-party:indexmap",
    ],
)

rust_binary(
    name = "autocxx-gen",
    srcs = glob(["gen/cmd/src/**/*.rs"]),
    edition = "2021",
    visibility = ["//visibility:public"],
    deps = [
        ":autocxx-engine",
        "//third-party:clap",
        "//third-party:proc-macro2",
        "//third-party:env_logger",
        "//third-party:pathdiff",
        "//third-party:indexmap",
        "//third-party:miette",
    ],
)
