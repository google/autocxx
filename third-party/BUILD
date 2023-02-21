load("@rules_rust//crate_universe:defs.bzl", "crates_vendor")

crates_vendor(
    name = "vendor",
    cargo_lockfile = "//third-party:Cargo.lock",
    generate_build_scripts = True,
    manifests = ["//third-party:Cargo.toml"],
    mode = "remote",
    tags = ["manual"],
    vendor_path = "bazel",
)

[
    alias(
        name = name,
        actual = "//third-party/bazel:{}".format(name),
        visibility = ["//visibility:public"],
    )
    for name in [
        "aquamarine",
        "autocxx-bindgen",
        "cc",
        "clap",
	"cxx",
        "cxx-gen",
        "env_logger",
        "indexmap",
        "indoc",
        "itertools",
        "log",
        "miette",
	"moveit",
        "once_cell",
        "pathdiff",
        "prettyplease",
        "proc-macro-error",
        "proc-macro2",
        "quote",
        "regex",
        "rustversion",
        "serde_json",
        "serde",
        "strum_macros",
        "syn",
        "tempfile",
        "thiserror",
        "version_check",
    ]
]
