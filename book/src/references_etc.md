# Pointers, references, values

`autocxx` knows how to deal with C++ APIs which take C++ types:
* By value
* By reference (const or not)
* By raw pointer
* By `std::unique_ptr`
* By `std::shared_ptr`
* By `std::weak_ptr`
* (Soon to come) By rvalue reference (that is, as a move parameter)

(all of this is because the underlying [`cxx`] crate has such versatility).
Some of these have some quirks in the way they're exposed in Rust, described below.



### Passing between C++ and Rust by value

Rust is free to move data around at any time. That's _not OK_ for some C++ types
which have non-trivial move constructors or destructors. Such types are common
in C++ (for example, even C++ `std::string`s) and these types commonly appear
in API declarations which we want to make available in Rust. Worse still, Rust
has no visibility into whether a C++ type meets these criteria. What do we do?

You have a choice:
* As standard, any C++ type passed by value will be `std::move`d on the C++ side
  into a `std::unique_ptr` before being passed to Rust, and similarly moved out
  of a `std::unique_ptr` when passed from Rust to C++.
* If you know that your C++ type can be safely byte-copied, then you can
  override this behavior by using [`generate_pod`] instead of [`generate`].

There's not a significant ergonomic problem from the use of [`cxx::UniquePtr`].
The main negative of the automatic boxing into [`cxx::UniquePtr`] is performance:
specifically, the need to
allocate heap cells on the C++ side and move data into and out of them.
You don't want to be doing this inside a tight loop (but if you're calling
across the C++/Rust boundary in a tight loop, perhaps reconsider that boundary
anyway).

If you want your type to be transferred between Rust and C++ truly _by value_
then use [`generate_pod`] instead of [`generate`].

Specifically, to be compatible with [`generate_pod`], your C++ type must either:
* Lack a move constructor _and_ lack a destructor
* Or contain a human promise that it's relocatable, by implementing
  the C++ trait `IsRelocatable` per the instructions in
  [cxx.h](https://github.com/dtolnay/cxx/blob/master/include/cxx.h)

Otherwise, your build will fail.

This doesn't just make a difference to the generated code for the type;
it also makes a difference to any functions which take or return that type.
If there's a C++ function which takes a struct by value, but that struct
is not declared as POD-safe, then we'll generate wrapper functions to move
that type into and out of [`cxx::UniquePtr`]s.

There is one other option under construction. The `moveit` crate replicates
C++ value move and copying semantics in Rust. There is limited early support
for `moveit` within autocxx; specifically, you can call C++ constructors
to emplace even non-trivial objects on the Rust stack using `moveit`, and
then call methods on them or use references to them in other function calls.
At present, you can't copy or move such objects - once they're created,
that's it, until it's dropped. At present therefore such objects can't be
passed into C++ functions which take non-POD types by value. This facility
is therefore best avoided for now until it's more complete - but see
`examples/non-trivial-type-on-stack` if you want to see how to use it.

### References and pointers

We follow [cxx] norms here. Specifically:
* A C++ reference becomes a Rust reference
* A C++ pointer becomes a Rust pointer.
* If a reference is returned with an ambiguous lifetime, we don't generate
  code for the function
* Pointers require use of `unsafe`, references don't necessarily.

That last point is key. If your C++ API takes pointers, you're going
to have to use `unsafe`. Similarly, if your C++ API returns a pointer,
you'll have to use `unsafe` to do anything useful with the pointer in Rust.
This is intentional: a pointer from C++ might be subject to concurrent
mutation, or it might have a lifetime that could disappear at any moment.
As a human, you must promise that you understand the constraints around
use of that pointer and that's what the `unsafe` keyword is for.

Exactly the same issues apply to C++ references _in theory_, but in practice,
they usually don't. Therefore [cxx] has taken the view that we can "trust"
a C++ reference to a higher degree than a pointer, and autocxx follows that
lead. In practice, of course, references are rarely return values from C++
APIs so we rarely have to navel-gaze about the trustworthiness of a
reference.

(See also the discussion of [`safety`] - if you haven't specified
an unsafety policy, _all_ C++ APIs require `unsafe` so the discussion is moot.)

If you're given a C++ object by pointer, and you want to interact with it,
you'll need to figure out the guarantees attached to the C++ object - most
notably its lifetime. To see some of the decision making process involved
see the [Steam example](https://github.com/google/autocxx/tree/main/examples/steam-mini/src/main.rs).

### [`cxx::UniquePtr`]s

We use [`cxx::UniquePtr`] in completely the normal way, but there are a few
quirks which you're more likely to run into with `autocxx`.

* Calling methods: you may need to use [`cxx::UniquePtr::pin_mut`] to get
  a reference on which you can call a method.
* Getting a raw pointer in order to pass to some pre-existing function:
  at present you need to do:
  ```rust,ignore
     let mut a = ffi::A::make_unique();
     unsafe { ffi::TakePointerToA(std::pin::Pin::<&mut ffi::A>::into_inner_unchecked(a.pin_mut())) };
  ```
  This may be simplified in future.
// 

