# S2 Geometry Example

This example demonstrates how to use autocxx with the S2 Geometry library.

## Issue and Solution

The s2geometry dependency was updated to a newer version that requires external abseil-cpp dependency, which caused build failures. The solution was to:

1. **Revert to a compatible s2geometry version**: The newer version (9a43f6a) requires external abseil-cpp and C++17, which causes compatibility issues with the current autocxx setup.

2. **Use the older version**: Version `0c4c460` includes abseil as a third-party library within s2geometry, which works correctly with the current autocxx configuration.

## Current Status

- ✅ **Working**: s2geometry version `0c4c460` with embedded abseil
- ❌ **Broken**: s2geometry version `9a43f6a` (latest) requires external abseil-cpp

## To Update s2geometry

If you want to use a newer version of s2geometry, you would need to:

1. Install abseil-cpp: `brew install abseil`
2. Modify `build.rs` to include abseil paths and libraries
3. Update to C++17 standard
4. Handle compatibility issues between abseil versions

## Running the Example

```bash
cd examples/s2
cargo run
```

Expected output:
```
Center of rectangle is 1.5, 5.5
```
