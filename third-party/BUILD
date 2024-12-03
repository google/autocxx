load("@rules_rust//crate_universe:defs.bzl", "crates_vendor")

crates_vendor(
    name = "vendor",
    cargo_lockfile = "//:Cargo.lock",
    manifests = [
	    "//:Cargo.toml",
	    "//:engine/Cargo.toml",
	    "//:gen/build/Cargo.toml",
	    "//:gen/cmd/Cargo.toml",
	    "//:integration-tests/Cargo.toml",
	    "//:macro/Cargo.toml",
	    "//:parser/Cargo.toml",
	    "//:tools/mdbook-preprocessor/Cargo.toml",
	    "//:tools/reduce/Cargo.toml",
	    # demo is a subpackage hence the file is exported via its BUILD.
	    "//demo:Cargo.toml",
    ],
    mode = "remote",
    vendor_path = "bazel",
)
