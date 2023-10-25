#![doc = include_str!("../README.md")]
#![cfg_attr(nightly, feature(unsize))]
#![cfg_attr(nightly, feature(dispatch_from_dyn))]

// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// The crazy macro_rules magic in this file is thanks to dtolnay@
// and is a way of attaching rustdoc to each of the possible directives
// within the include_cpp outer macro. None of the directives actually
// do anything - all the magic is handled entirely by
// autocxx_macro::include_cpp_impl.

mod reference_wrapper;
mod rvalue_param;
pub mod subclass;
mod value_param;

pub use reference_wrapper::{AsCppMutRef, AsCppRef, CppMutRef, CppPin, CppRef, CppUniquePtrPin};

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Include some C++ headers in your Rust project.
///
/// This macro allows you to include one or more C++ headers within
/// your Rust code, and call their functions fairly naturally.
///
/// # Examples
///
/// C++ header (`input.h`):
/// ```cpp
/// #include <cstdint>
///
/// uint32_t do_math(uint32_t a);
/// ```
///
/// Rust code:
/// ```
/// # use autocxx_macro::include_cpp_impl as include_cpp;
/// include_cpp!(
/// #   parse_only!()
///     #include "input.h"
///     generate!("do_math")
///     safety!(unsafe)
/// );
///
/// # mod ffi { pub fn do_math(a: u32) -> u32 { a+3 } }
/// # fn main() {
/// ffi::do_math(3);
/// # }
/// ```
///
/// The resulting bindings will use idiomatic Rust wrappers for types from the [cxx]
/// crate, for example [`cxx::UniquePtr`] or [`cxx::CxxString`]. Due to the care and thought
/// that's gone into the [cxx] crate, such bindings are pleasant and idiomatic to use
/// from Rust, and usually don't require the `unsafe` keyword.
///
/// For full documentation, see [the manual](https://google.github.io/autocxx/).
///
/// # The [`include_cpp`] macro
///
/// Within the braces of the `include_cpp!{...}` macro, you should provide
/// a list of at least the following:
///
/// * `#include "cpp_header.h"`: a header filename to parse and include
/// * `generate!("type_or_function_name")`: a type or function name whose declaration
///   should be made available to C++. (See the section on Allowlisting, below).
/// * Optionally, `safety!(unsafe)` - see discussion of [`safety`].
///
/// Other directives are possible as documented in this crate.
///
/// Now, try to build your Rust project. `autocxx` may fail to generate bindings
/// for some of the items you specified with [generate] directives: remove
/// those directives for now, then see the next section for advice.
///
/// # Allowlisting
///
/// How do you inform autocxx which bindings to generate? There are three
/// strategies:
///
/// * *Recommended*: provide various [`generate`] directives in the
///   [`include_cpp`] macro. This can specify functions or types.
/// * *Not recommended*: in your `build.rs`, call `Builder::auto_allowlist`.
///   This will attempt to spot _uses_ of FFI bindings anywhere in your Rust code
///   and build the allowlist that way. This is experimental and has known limitations.
/// * *Strongly not recommended*: use [`generate_all`]. This will attempt to
///   generate Rust bindings for _any_ C++ type or function discovered in the
///   header files. This is generally a disaster if you're including any
///   remotely complex header file: we'll try to generate bindings for all sorts
///   of STL types. This will be slow, and some may well cause problems.
///   Effectively this is just a debug option to discover such problems. Don't
///   use it!
///
/// # Internals
///
/// For documentation on how this all actually _works_, see
/// `IncludeCppEngine` within the `autocxx_engine` crate.
#[macro_export]
macro_rules! include_cpp {
    (
        $(#$include:ident $lit:literal)*
        $($mac:ident!($($arg:tt)*))*
    ) => {
        $($crate::$include!{__docs})*
        $($crate::$mac!{__docs})*
        $crate::include_cpp_impl! {
            $(#include $lit)*
            $($mac!($($arg)*))*
        }
    };
}

/// Include a C++ header. A directive to be included inside
/// [include_cpp] - see [include_cpp] for details
#[macro_export]
macro_rules! include {
    ($($tt:tt)*) => { $crate::usage!{$($tt)*} };
}

/// Generate Rust bindings for the given C++ type or function.
/// A directive to be included inside
/// [include_cpp] - see [include_cpp] for general information.
/// See also [generate_pod].
#[macro_export]
macro_rules! generate {
    ($($tt:tt)*) => { $crate::usage!{$($tt)*} };
}

/// Generate as "plain old data" and add to allowlist.
/// Generate Rust bindings for the given C++ type such that
/// it can be passed and owned by value in Rust. This only works
/// for C++ types which have trivial move constructors and no
/// destructor - you'll encounter a compile error otherwise.
/// If your type doesn't match that description, use [generate]
/// instead, and own the type using [UniquePtr][cxx::UniquePtr].
/// A directive to be included inside
/// [include_cpp] - see [include_cpp] for general information.
#[macro_export]
macro_rules! generate_pod {
    ($($tt:tt)*) => { $crate::usage!{$($tt)*} };
}

/// Generate Rust bindings for all C++ types and functions
/// in a given namespace.
/// A directive to be included inside
/// [include_cpp] - see [include_cpp] for general information.
/// See also [generate].
#[macro_export]
macro_rules! generate_ns {
    ($($tt:tt)*) => { $crate::usage!{$($tt)*} };
}

/// Generate Rust bindings for all C++ types and functions
/// found. Highly experimental and not recommended.
/// A directive to be included inside
/// [include_cpp] - see [include_cpp] for general information.
/// See also [generate].
#[macro_export]
macro_rules! generate_all {
    ($($tt:tt)*) => { $crate::usage!{$($tt)*} };
}

/// Generate as "plain old data". For use with [generate_all]
/// and similarly experimental.
#[macro_export]
macro_rules! pod {
    ($($tt:tt)*) => { $crate::usage!{$($tt)*} };
}

/// Skip the normal generation of a `make_string` function
/// and other utilities which we might generate normally.
/// A directive to be included inside
/// [include_cpp] - see [include_cpp] for general information.
#[macro_export]
macro_rules! exclude_utilities {
    ($($tt:tt)*) => { $crate::usage!{$($tt)*} };
}

/// Entirely block some type from appearing in the generated
/// code. This can be useful if there is a type which is not
/// understood by bindgen or autocxx, and incorrect code is
/// otherwise generated.
/// This is 'greedy' in the sense that any functions/methods
/// which take or return such a type will _also_ be blocked.
///
/// A directive to be included inside
/// [include_cpp] - see [include_cpp] for general information.
#[macro_export]
macro_rules! block {
    ($($tt:tt)*) => { $crate::usage!{$($tt)*} };
}

/// Avoid generating implicit constructors for this type.
/// The rules for when to generate C++ implicit constructors
/// are complex, and if autocxx gets it wrong, you can block
/// such constructors using this.
///
/// A directive to be included inside
/// [include_cpp] - see [include_cpp] for general information.
#[macro_export]
macro_rules! block_constructors {
    ($($tt:tt)*) => { $crate::usage!{$($tt)*} };
}

/// The name of the mod to be generated with the FFI code.
/// The default is `ffi`.
///
/// A directive to be included inside
/// [include_cpp] - see [include_cpp] for general information.
#[macro_export]
macro_rules! name {
    ($($tt:tt)*) => { $crate::usage!{$($tt)*} };
}

/// A concrete type to make, for example
/// `concrete!("Container<Contents>")`.
/// All types must already be on the allowlist by having used
/// `generate!` or similar.
///
/// A directive to be included inside
/// [include_cpp] - see [include_cpp] for general information.
#[macro_export]
macro_rules! concrete {
    ($($tt:tt)*) => { $crate::usage!{$($tt)*} };
}

/// Specifies a global safety policy for functions generated
/// from these headers. By default (without such a `safety!`
/// directive) all such functions are marked as `unsafe` and
/// therefore can only be called within an `unsafe {}` block
/// or some `unsafe` function which you create.
///
/// Alternatively, by specifying a `safety!` block you can
/// declare that most generated functions are in fact safe.
/// Specifically, you'd specify:
/// `safety!(unsafe)`
/// or
/// `safety!(unsafe_ffi)`
/// These two options are functionally identical. If you're
/// unsure, simply use `unsafe`. The reason for the
/// latter option is if you have code review policies which
/// might want to give a different level of scrutiny to
/// C++ interop as opposed to other types of unsafe Rust code.
/// Maybe in your organization, C++ interop is less scary than
/// a low-level Rust data structure using pointer manipulation.
/// Or maybe it's more scary. Either way, using `unsafe` for
/// the data structure and using `unsafe_ffi` for the C++
/// interop allows you to apply different linting tools and
/// policies to the different options.
///
/// Irrespective, C++ code is of course unsafe. It's worth
/// noting that use of C++ can cause unexpected unsafety at
/// a distance in faraway Rust code. As with any use of the
/// `unsafe` keyword in Rust, *you the human* are declaring
/// that you've analyzed all possible ways that the code
/// can be used and you are guaranteeing to the compiler that
/// no badness can occur. Good luck.
///
/// Generated C++ APIs which use raw pointers remain `unsafe`
/// no matter what policy you choose.
///
/// There's an additional possible experimental safety
/// policy available here:
/// `safety!(unsafe_references_wrapped)`
/// This policy treats C++ references as scary and requires
/// them to be wrapped in a `CppRef` type: see [`CppRef`].
/// This only works on nightly Rust because it
/// depends upon an unstable feature
/// (`arbitrary_self_types`). However, it should
/// eliminate all undefined behavior related to Rust's
/// stricter aliasing rules than C++.
#[macro_export]
macro_rules! safety {
    ($($tt:tt)*) => { $crate::usage!{$($tt)*} };
}

/// Whether to avoid generating [`cxx::UniquePtr`] and [`cxx::Vector`]
/// implementations. This is primarily useful for reducing test cases and
/// shouldn't be used in normal operation.
///
/// A directive to be included inside
/// [include_cpp] - see [include_cpp] for general information.
#[macro_export]
macro_rules! exclude_impls {
    ($($tt:tt)*) => { $crate::usage!{$($tt)*} };
}

/// Indicates that a C++ type is not to be generated by autocxx in this case,
/// but instead should refer to some pre-existing Rust type.
///
/// If you wish for the type to be POD, you can use a `pod!` directive too
/// (but see the "requirements" section below).
///
/// The syntax is:
/// `extern_cpp_type!("CppNameGoesHere", path::to::rust::type)`
///
/// Generally speaking, this should be used only to refer to types
/// generated elsewhere by `autocxx` or `cxx` to ensure that they meet
/// all the right requirements. It's possible - but fragile - to
/// define such types yourself.
///
/// # Requirements for externally defined Rust types
///
/// It's generally expected that you would make such a type
/// in Rust using a separate `include_cpp!` macro, or
/// a manual `#[cxx::bridge]` directive somehwere. That is, this
/// directive is intended mainly for use in cross-linking different
/// sets of bindings in different mods, rather than truly to point to novel
/// external types.
///
/// But with that in mind, here are the requirements you must stick to.
///
/// For non-POD external types:
/// * The size and alignment of this type *must* be correct.
///
/// For POD external types:
/// * As above
/// * Your type must correspond to the requirements of
///   [`cxx::kind::Trivial`]. In general, that means, no move constructor
///   and no destructor. If you generate this type using `cxx` itself
///   (or `autocxx`) this will be enforced using `static_assert`s
///   within the generated C++ code. Without using those tools, you're
///   on your own for determining this... and it's hard because the presence
///   of particular fields or base classes may well result in your type
///   violating those rules.
///
/// A directive to be included inside
/// [include_cpp] - see [include_cpp] for general information.
#[macro_export]
macro_rules! extern_cpp_type {
    ($($tt:tt)*) => { $crate::usage!{$($tt)*} };
}

/// Indicates that a C++ type is not to be generated by autocxx in this case,
/// but instead should refer to some pre-existing Rust type. Unlike
/// `extern_cpp_type!`, there's no need for the size and alignment of this
/// type to be correct.
///
/// The syntax is:
/// `extern_cpp_opaque_type!("CppNameGoesHere", path::to::rust::type)`
///
/// A directive to be included inside
/// [include_cpp] - see [include_cpp] for general information.
#[macro_export]
macro_rules! extern_cpp_opaque_type {
    ($($tt:tt)*) => { $crate::usage!{$($tt)*} };
}

/// Deprecated - use [`extern_rust_type`] instead.
#[macro_export]
#[deprecated]
macro_rules! rust_type {
    ($($tt:tt)*) => { $crate::usage!{$($tt)*} };
}

/// See [`extern_rust::extern_rust_type`].
#[macro_export]
macro_rules! extern_rust_type {
    ($($tt:tt)*) => { $crate::usage!{$($tt)*} };
}

/// See [`subclass::subclass`].
#[macro_export]
macro_rules! subclass {
    ($($tt:tt)*) => { $crate::usage!{$($tt)*} };
}

/// Indicates that a C++ type can definitely be instantiated. This has effect
/// only in a very specific case:
/// * the type is a typedef to something else
/// * the 'something else' can't be fully inspected by autocxx, possibly
///   becaue it relies on dependent qualified types or some other template
///   arrangement that bindgen cannot fully understand.
/// In such circumstances, autocxx normally has to err on the side of caution
/// and assume that some type within the 'something else' is itself a forward
/// declaration. That means, the opaque typedef won't be storable within
/// a [`cxx::UniquePtr`]. If you know that no forward declarations are involved,
/// you can declare the typedef type is instantiable and then you'll be able to
/// own it within Rust.
///
/// The syntax is:
/// `instantiable!("CppNameGoesHere")`
///
/// A directive to be included inside
/// [include_cpp] - see [include_cpp] for general information.
#[macro_export]
macro_rules! instantiable {
    ($($tt:tt)*) => { $crate::usage!{$($tt)*} };
}

#[doc(hidden)]
#[macro_export]
macro_rules! usage {
    (__docs) => {};
    ($($tt:tt)*) => {
        compile_error! {r#"usage:  include_cpp! {
                   #include "path/to/header.h"
                   generate!(...)
                   generate_pod!(...)
               }
"#}
    };
}

use std::pin::Pin;

#[doc(hidden)]
pub use autocxx_macro::include_cpp_impl;

#[doc(hidden)]
pub use autocxx_macro::cpp_semantics;

macro_rules! ctype_wrapper {
    ($r:ident, $c:expr, $d:expr) => {
        #[doc=$d]
        #[derive(Debug, Eq, Copy, Clone, PartialEq, Hash)]
        #[allow(non_camel_case_types)]
        #[repr(transparent)]
        pub struct $r(pub ::std::os::raw::$r);

        /// # Safety
        ///
        /// We assert that the namespace and type ID refer to a C++
        /// type which is equivalent to this Rust type.
        unsafe impl cxx::ExternType for $r {
            type Id = cxx::type_id!($c);
            type Kind = cxx::kind::Trivial;
        }

        impl From<::std::os::raw::$r> for $r {
            fn from(val: ::std::os::raw::$r) -> Self {
                Self(val)
            }
        }

        impl From<$r> for ::std::os::raw::$r {
            fn from(val: $r) -> Self {
                val.0
            }
        }
    };
}

ctype_wrapper!(
    c_ulonglong,
    "c_ulonglong",
    "Newtype wrapper for an unsigned long long"
);
ctype_wrapper!(c_longlong, "c_longlong", "Newtype wrapper for a long long");
ctype_wrapper!(c_ulong, "c_ulong", "Newtype wrapper for an unsigned long");
ctype_wrapper!(c_long, "c_long", "Newtype wrapper for a long");
ctype_wrapper!(
    c_ushort,
    "c_ushort",
    "Newtype wrapper for an unsigned short"
);
ctype_wrapper!(c_short, "c_short", "Newtype wrapper for an short");
ctype_wrapper!(c_uint, "c_uint", "Newtype wrapper for an unsigned int");
ctype_wrapper!(c_int, "c_int", "Newtype wrapper for an int");
ctype_wrapper!(c_uchar, "c_uchar", "Newtype wrapper for an unsigned char");

/// Newtype wrapper for a C void. Only useful as a `*c_void`
#[allow(non_camel_case_types)]
#[repr(transparent)]
pub struct c_void(pub ::std::os::raw::c_void);

/// # Safety
///
/// We assert that the namespace and type ID refer to a C++
/// type which is equivalent to this Rust type.
unsafe impl cxx::ExternType for c_void {
    type Id = cxx::type_id!(c_void);
    type Kind = cxx::kind::Trivial;
}

/// A C++ `char16_t`
#[allow(non_camel_case_types)]
#[repr(transparent)]
pub struct c_char16_t(pub u16);

/// # Safety
///
/// We assert that the namespace and type ID refer to a C++
/// type which is equivalent to this Rust type.
unsafe impl cxx::ExternType for c_char16_t {
    type Id = cxx::type_id!(c_char16_t);
    type Kind = cxx::kind::Trivial;
}

/// autocxx couldn't generate these bindings.
/// If you come across a method, type or function which refers to this type,
/// it indicates that autocxx couldn't generate that binding. A documentation
/// comment should be attached indicating the reason.
pub struct BindingGenerationFailure {
    _unallocatable: [*const u8; 0],
    _pinned: core::marker::PhantomData<core::marker::PhantomPinned>,
}

/// Tools to export Rust code to C++.
// These are in a mod to avoid shadowing the definitions of the
// directives above, which, being macro_rules, are unavoidably
// in the crate root but must be function-style macros to keep
// the include_cpp impl happy.
pub mod extern_rust {

    /// Declare that this is a Rust type which is to be exported to C++.
    /// You can use this in two ways:
    /// * as an attribute macro on a Rust type, for instance:
    ///   ```
    ///   # use autocxx_macro::extern_rust_type as extern_rust_type;
    ///   #[extern_rust_type]
    ///   struct Bar;
    ///   ```
    /// * as a directive within the [include_cpp] macro, in which case
    ///   provide the type path in brackets:
    ///   ```
    ///   # use autocxx_macro::include_cpp_impl as include_cpp;
    ///   include_cpp!(
    ///   #   parse_only!()
    ///       #include "input.h"
    ///       extern_rust_type!(Bar)
    ///       safety!(unsafe)
    ///   );
    ///   struct Bar;
    ///   ```
    /// These may be used within references in the signatures of C++ functions,
    /// for instance. This will contribute to an `extern "Rust"` section of the
    /// generated `cxx` bindings, and this type will appear in the C++ header
    /// generated for use in C++.
    ///
    /// # Finding these bindings from C++
    ///
    /// You will likely need to forward-declare this type within your C++ headers
    /// before you can use it in such function signatures. autocxx can't generate
    /// headers (with this type definition) until it's parsed your header files;
    /// logically therefore if your header files mention one of these types
    /// it's impossible for them to see the definition of the type.
    ///
    /// If you're using multiple sets of `include_cpp!` directives, or
    /// a mixture of `include_cpp!` and `#[cxx::bridge]` bindings, then you
    /// may be able to `#include "cxxgen.h"` to refer to the generated C++
    /// function prototypes. In this particular circumstance, you'll want to know
    /// how exactly the `cxxgen.h` header is named, because one will be
    /// generated for each of the sets of bindings encountered. The pattern
    /// can be set manually using `autocxxgen`'s command-line options. If you're
    /// using `autocxx`'s `build.rs` support, those headers will be named
    /// `cxxgen.h`, `cxxgen1.h`, `cxxgen2.h` according to the order in which
    /// the `include_cpp` or `cxx::bridge` bindings are encountered.
    pub use autocxx_macro::extern_rust_type;

    /// Declare that a given function is a Rust function which is to be exported
    /// to C++. This is used as an attribute macro on a Rust function, for instance:
    /// ```
    /// # use autocxx_macro::extern_rust_function as extern_rust_function;
    /// #[extern_rust_function]
    /// pub fn call_me_from_cpp() { }
    /// ```
    ///
    /// See [`extern_rust_type`] for details of how to find the generated
    /// declarations from C++.
    pub use autocxx_macro::extern_rust_function;
}

/// Equivalent to [`std::convert::AsMut`], but returns a pinned mutable reference
/// such that cxx methods can be called on it.
pub trait PinMut<T>: AsRef<T> {
    /// Return a pinned mutable reference to a type.
    fn pin_mut(&mut self) -> std::pin::Pin<&mut T>;
}

/// Provides utility functions to emplace any [`moveit::New`] into a
/// [`cxx::UniquePtr`]. Automatically imported by the autocxx prelude
/// and implemented by any (autocxx-related) [`moveit::New`].
pub trait WithinUniquePtr {
    type Inner: UniquePtrTarget + MakeCppStorage;
    /// Create this item within a [`cxx::UniquePtr`].
    fn within_unique_ptr(self) -> cxx::UniquePtr<Self::Inner>;
}

/// Provides utility functions to emplace any [`moveit::New`] into a
/// [`Box`]. Automatically imported by the autocxx prelude
/// and implemented by any (autocxx-related) [`moveit::New`].
pub trait WithinBox {
    type Inner;
    /// Create this item inside a pinned box. This is a good option if you
    /// want to own this object within Rust, and want to create Rust references
    /// to it.
    fn within_box(self) -> Pin<Box<Self::Inner>>;
    /// Create this item inside a [`CppPin`]. This is a good option if you
    /// want to own this option within Rust, but you want to create [`CppRef`]
    /// C++ references to it. You'd only want to choose that option if you have
    /// enabled the C++ reference wrapper support by using the
    /// `safety!(unsafe_references_wrapped`) directive. If you haven't done
    /// that, ignore this function.
    fn within_cpp_pin(self) -> CppPin<Self::Inner>;
}

use cxx::kind::Trivial;
use cxx::ExternType;
use moveit::Emplace;
use moveit::MakeCppStorage;

impl<N, T> WithinUniquePtr for N
where
    N: New<Output = T>,
    T: UniquePtrTarget + MakeCppStorage,
{
    type Inner = T;
    fn within_unique_ptr(self) -> cxx::UniquePtr<T> {
        UniquePtr::emplace(self)
    }
}

impl<N, T> WithinBox for N
where
    N: New<Output = T>,
{
    type Inner = T;
    fn within_box(self) -> Pin<Box<T>> {
        Box::emplace(self)
    }
    fn within_cpp_pin(self) -> CppPin<Self::Inner> {
        CppPin::from_pinned_box(Box::emplace(self))
    }
}

/// Emulates the [`WithinUniquePtr`] trait, but for trivial (plain old data) types.
/// This allows such types to behave identically if a type is changed from
/// `generate!` to `generate_pod!`.
///
/// (Ideally, this would be the exact same trait as [`WithinUniquePtr`] but this runs
/// the risk of conflicting implementations. Negative trait bounds would solve
/// this!)
pub trait WithinUniquePtrTrivial: UniquePtrTarget + Sized + Unpin {
    fn within_unique_ptr(self) -> cxx::UniquePtr<Self>;
}

impl<T> WithinUniquePtrTrivial for T
where
    T: UniquePtrTarget + ExternType<Kind = Trivial> + Sized + Unpin,
{
    fn within_unique_ptr(self) -> cxx::UniquePtr<T> {
        UniquePtr::new(self)
    }
}

/// Emulates the [`WithinBox`] trait, but for trivial (plain old data) types.
/// This allows such types to behave identically if a type is changed from
/// `generate!` to `generate_pod!`.
///
/// (Ideally, this would be the exact same trait as [`WithinBox`] but this runs
/// the risk of conflicting implementations. Negative trait bounds would solve
/// this!)
pub trait WithinBoxTrivial: Sized + Unpin {
    fn within_box(self) -> Pin<Box<Self>>;
}

impl<T> WithinBoxTrivial for T
where
    T: ExternType<Kind = Trivial> + Sized + Unpin,
{
    fn within_box(self) -> Pin<Box<T>> {
        Pin::new(Box::new(self))
    }
}

use cxx::memory::UniquePtrTarget;
use cxx::UniquePtr;
use moveit::New;
pub use rvalue_param::RValueParam;
pub use rvalue_param::RValueParamHandler;
pub use value_param::as_copy;
pub use value_param::as_mov;
pub use value_param::as_new;
pub use value_param::ValueParam;
pub use value_param::ValueParamHandler;

/// Imports which you're likely to want to use.
pub mod prelude {
    pub use crate::as_copy;
    pub use crate::as_mov;
    pub use crate::as_new;
    pub use crate::c_int;
    pub use crate::c_long;
    pub use crate::c_longlong;
    pub use crate::c_short;
    pub use crate::c_uchar;
    pub use crate::c_uint;
    pub use crate::c_ulong;
    pub use crate::c_ulonglong;
    pub use crate::c_ushort;
    pub use crate::c_void;
    pub use crate::cpp_semantics;
    pub use crate::include_cpp;
    pub use crate::AsCppMutRef;
    pub use crate::AsCppRef;
    pub use crate::CppMutRef;
    pub use crate::CppPin;
    pub use crate::CppRef;
    pub use crate::CppUniquePtrPin;
    pub use crate::PinMut;
    pub use crate::RValueParam;
    pub use crate::ValueParam;
    pub use crate::WithinBox;
    pub use crate::WithinBoxTrivial;
    pub use crate::WithinUniquePtr;
    pub use crate::WithinUniquePtrTrivial;
    pub use cxx::UniquePtr;
    pub use moveit::moveit;
    pub use moveit::new::New;
    pub use moveit::Emplace;
}

/// Re-export moveit for ease of consumers.
pub use moveit;

/// Re-export cxx such that clients can use the same version as
/// us. This doesn't enable clients to avoid depending on the cxx
/// crate too, unfortunately, since generated cxx::bridge code
/// refers explicitly to ::cxx. See
/// <https://github.com/google/autocxx/issues/36>
pub use cxx;
