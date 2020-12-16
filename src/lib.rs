// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// The crazy macro_rules magic in this file is thanks to dtolnay@
// and is a way of attaching rustdoc to each of the possible directives
// within the include_cpp outer macro. None of the directives actually
// do anything - all the magic is handled entirely by
// autocxx_macro::include_cpp_impl.

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
/// #   parse_only
///     #include "input.h"
///     generate!("do_math")
/// );
///
/// # mod ffi { pub fn do_math(a: u32) -> u32 { a+3 } }
/// # fn main() {
/// ffi::do_math(3);
/// # }
/// ```
///
/// # Configuring the build
///
/// To build this, you'll need to:
/// * Build the C++ side of the bindings. You'll need to use the `autocxx-gen`
///   crate or the `autocxx-build` crate to process the .rs code into C++ header and
///   implementation files.
/// * Educate the procedural macro about where to find the C++ headers. Set the
///   `AUTOCXX_INC` environment variable to a list of directories to search.
///   If you use `autocxx-build`, this happens automatically.

///
/// # Syntax
///
/// Within the brackets of the `include_cxx!(...)` macro, you should provide
/// a list of at least the following:
///
/// * `#include "cpp_header.h"`: a header filename to parse and include
/// * `generate!("type_or_function_name")`: a type or function name whose declaration
///   should be made available to C++.
///
/// Other directives are possible as documented in this crate.
///
/// # How to generate structs
///
/// All C++ types can be owned within a [UniquePtr][autocxx_engine::cxx::UniquePtr]
/// within Rust. To let this be possible, simply pass the names of these
/// types within [generate] (or just [generate] any function which requires these types).
///
/// However, only _some_ C++ `struct`s can be owned _by value_ within Rust. Those
/// types must be freely byte-copyable, because Rust is free to do that at
/// any time. If you believe your `struct` meets those criteria, you can
/// use [generate_pod] instead.
///
/// Use [generate] under normal circumstances, but [generate_pod] only for structs
/// where you absolutely do need to pass them truly by value and have direct field access.
///
/// This doesn't just make a difference to the generated code for the type;
/// it also makes a difference to any functions which take or return that type.
/// If there's a C++ function which takes a struct by value, but that struct
/// is not declared as POD-safe, then we'll generate wrapper functions to move
/// that type into and out of [UniquePtr][autocxx_engine::cxx::UniquePtr]s.
///
/// # Generated code
///
/// You will find that this macro expands to the equivalent of:
///
/// ```no_run
/// mod ffi {
///     pub fn do_math(a: u32) -> u32
/// #   { a+3 }
///     pub const kMyCxxConst: i32 = 3;
///     pub const MY_PREPROCESSOR_DEFINITION: i64 = 3i64;
/// }
/// ```
///
/// # Built-in types
///
/// The generated code uses `cxx` for interop: see that crate for many important
/// considerations including safety and the list of built-in types, for example
/// [UniquePtr][autocxx_engine::cxx::UniquePtr] and
/// [CxxString][autocxx_engine::cxx::CxxString].
///
/// # Making strings
///
/// Unless you use [exclude_utilities], you will find a trait called
/// `ffi::ToCppString` which you can use to convert any Rust string into a C++
/// `std::unique_ptr<std::string>` like this:
///
/// ```ignore
/// use ffi::ToCpp;
/// let unique_ptr_to_cxx_string = "my_string".to_cpp();
/// ```
///
/// # Making other C++ types
///
/// Types gain a `make_unique` associated function, but currently only
/// if they have an explicit constructor. This will (of course) return a
/// `UniquePtr` containing that type.
///
/// # Preprocessor symbols
///
/// `#define` and other preprocessor symbols will appear as constants.
/// At present there is no way to do compile-time disablement of code
/// (equivalent of `#ifdef`).
///
/// # String constants
///
/// Whether from a preprocessor symbol or from a C++ `char*` constant,
/// strings appear as `[u8]` with a null terminator. To get a Rust string,
/// do this:
///
/// ```cpp
/// #define BOB "Hello"
/// ```
///
/// ```
/// # mod ffi { pub static BOB: [u8; 6] = [72u8, 101u8, 108u8, 108u8, 111u8, 0u8]; }
/// assert_eq!(std::str::from_utf8(&ffi::BOB).unwrap().trim_end_matches(char::from(0)), "Hello");
/// ```
///
/// # Namespaces
///
/// The C++ namespace structure is reflected in mods within the generated
/// ffi mod. However, at present there is an internal limitation that
/// autocxx can't handle multiple symbols with the same identifier, even
/// if they're in different namespaces. This will be fixed in future.
///
/// # Overloads - and identifiers ending in digits
///
/// C++ allows function overloads; Rust doesn't. `autocxx` follows the lead
/// of `bindgen` here and generating overloads as `func`, `func1`, `func2` etc.
///
/// # C++ classes - why do I get warnings?
///
/// autocxx is not currently able to distinguish a C++ struct from a C++ class.
/// It currently assumes they're all structs. This results in warnings
/// from most compilers, but could cause actual binary mismatches
/// on some ABIs. This is a temporary known limitation.
///
/// # Forward declarations
///
/// A type which is incomplete in the C++ headers (i.e. represented only by a forward
/// declaration) can't be held in a `UniquePtr` within Rust (because Rust can't know
/// if it has a destructor that will need to be called if the object is `Drop`ped.)
/// Naturally, such an object can't be passed by value either; it can still be
/// referenced in Rust references.
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

/// Generate as "plain old data".
/// Generate Rust bindings for the given C++ type such that
/// it can be passed and owned by value in Rust. This only works
/// for C++ types which have trivial move constructors and no
/// destructor - you'll encounter a compile error otherwise.
/// If your type doesn't match that description, use [generate]
/// instead, and own the type using [UniquePtr][autocxx_engine::cxx::UniquePtr].
/// A directive to be included inside
/// [include_cpp] - see [include_cpp] for general information.
#[macro_export]
macro_rules! generate_pod {
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

/// Specify that a struct (or other type) is nested within
/// some outer type. autocxx can't currently determine
/// this itself; it's hoped that this is a temporary restriction
/// and that the need for this directive will evaporate soon.
/// Meanwhile, specify two arguments - first the path to the
/// type without any nesting; and secondly the path with nesting.
/// So for instance for a situation like
/// ```cpp
/// namespace A {
///   struct B {
///     struct C {
///       uint32_t b;
///     }
///     uint32_t a;
///   }
/// ```
/// specify
/// `nested_type("A::C", "A::B::C")`
///
/// A directive to be included inside
/// [include_cpp] - see [include_cpp] for general information.
#[macro_export]
macro_rules! nested_type {
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

#[doc(hidden)]
pub use autocxx_macro::include_cpp_impl;
