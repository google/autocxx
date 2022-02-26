# C++ structs, enums and classes

If you add a C++ struct, class or enum to the allowlist, Rust bindings will be generated to that type and to any methods it has.
Even if you don't add it to the allowlist, the type may be generated if it's required by some other function - but in this case
all its methods won't be generated.

Rust and C++ differ in an important way:

* In Rust, the compiler is free to pick up some data and move it to somewhere else (in a `memcpy` sense). The object is none the wiser.
* In C++, once created, an object stays where it is, until or unless it has its "move constructor" invoked.

This makes a big difference: C++ objects can have self-referential pointers, and any such pointer would be invalidated by Rust doing
a memcpy. Such self-referential pointers are common - even some implementations of `std::string` do it.

# POD and non-POD

When asking `autocxx` to generate bindings for a type, then, you have to make a choice.

* This C++ type is trivial. It has no destructor or move constructor (or they're trivial), and thus Rust is free to move it around the stack as it wishes. `autocxx` calls these types POD ("plain old data"). Alternatively,
* This C++ type has a non-trivial destructor or move constructor, so we can't allow Rust to move this around. `autocxx` calls these types non-POD.

POD types are nicer:

* You can just use them as regular Rust types.
* You get direct field access.
* No funny business.

Non-POD types are awkward:

* You can't just _have_ one as a Rust variable. Normally you hold them in a [`cxx::UniquePtr`], though there are other options.
* There is no access to fields.
* You can't even have a `&mut` reference to one, because then you might be able to use [`std::mem::swap`] or similara. You can have a `Pin<&mut>` reference, which is more fiddly.

By default, `autocxx` generates non-POD types. You can request a POD type using `generate_pod!`. Don't worry: you can't mess this up. If the C++ type doesn't in fact comply with the requirements for a POD type, your build will fail thanks to some static assertions generated in the C++.

See [the chapter on storage](storage.md) for lots more detail on how you can hold onto non-POD types.
