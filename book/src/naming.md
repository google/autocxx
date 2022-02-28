# Naming

## Namespaces

The C++ namespace structure is reflected in mods within the generated
ffi mod. However, at present there is an internal limitation that
autocxx can't handle multiple types with the same identifier, even
if they're in different namespaces. This will be fixed in future.

## Nested classes

There is support for generating bindings of nested types, with some
restrictions. Currently the C++ type `A::B` will be given the Rust name
`A_B` in the same module as its enclosing namespace.

## Overloads

See [the chapter on C++ functions](cpp_functions.md).
