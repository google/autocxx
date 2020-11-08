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
///     allow("do_math")
/// );
///
/// # mod ffi { pub mod cxxbridge { pub fn do_math(a: u32) -> u32 { a+3 } } }
/// # fn main() {
/// ffi::cxxbridge::do_math(3);
/// # }
/// ```
///
/// # Configuring the build
///
/// To build this, you'll need to:
/// * Educate the procedural macro about where to find the C++ headers. Set the
///   `AUTOCXX_INC` environment variable to a list of directories to search.
/// * Build the C++ side of the bindings. You'll need to use the `autocxx-gen`
///   crate (or similar) to process the same .rs code into C++ header and
///   implementation files.
///
/// # Syntax
///
/// Within the brackets of the `include_cxx!(...)` macro, you should provide
/// a list of at least the following:
///
/// * `#include "cpp_header.h"`: a header filename to parse and include
/// * `allow("type_or_function_name")`: a type or function name whose declaration
///   should be made available to C++.
///
/// Other directives are possible as documented in this crate.
///
/// # How to allow structs
///
/// All C++ types can be owned within a [UniquePtr][autocxx_engine::cxx::UniquePtr]
/// within Rust. To let this be possible, simply pass the names of these
/// types within [allow] (or just [allow] any function which requires these types).
///
/// However, only _some_ C++ `struct`s can be owned _by value_ within Rust. Those
/// types must be freely byte-copyable, because Rust is free to do that at
/// any time. If you believe your `struct` meets those criteria, you can
/// use [allow_pod] instead.
///
/// Use [allow] under normal circumstances, but [allow_pod] only for structs
/// where you absolutely do need to pass them truly by value and have direct field access.
///
/// This doesn't just make a difference to the generated code for the type;
/// it also makes a difference to any functions which take or return that type.
/// If there's a C++ function which takes a struct by value, but that struct
/// is not declared as POD-safe, then we'll generate wrapper functions to move
/// that type into and out of [UniquePtr][autocxx_engine::cxx::UniquePtr]s.
///
///
/// # Generated code
///
/// You will find that this macro expands to the equivalent of:
///
/// ```
/// mod ffi {
///     pub fn do_math(a: u32) -> u32
/// #   { a+3 }
///     pub const kMyCxxConst: i32 = 3;
///     pub const MY_PREPROCESSOR_DEFINITION: i64 = 3i64;
/// }
/// ```
///
/// # Namespaces
///
/// At present, C++ namespaces are partially handled. autocxx will understand
/// and generate Rust code from C++ code which has namespaces, but a flat structure
/// will be generated on the Rust side. That means that you can't have two
/// types or two functions with the same name within different namespaces. This
/// is a temporary restriction.
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
/// See also [allow_pod].
#[macro_export]
macro_rules! allow {
    ($($tt:tt)*) => { $crate::usage!{$($tt)*} };
}

/// Generate as "plain old data".
/// Generate Rust bindings for the given C++ type such that
/// it can be passed and owned by value in Rust. This only works
/// for C++ types which have trivial move constructors and no
/// destructor - you'll encounter a compile error otherwise.
/// If your type doesn't match that description, use [allow]
/// instead, and own the type using [UniquePtr][autocxx_engine::cxx::UniquePtr].
/// A directive to be included inside
/// [include_cpp] - see [include_cpp] for general information.
#[macro_export]
macro_rules! allow_pod {
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

#[doc(hidden)]
#[macro_export]
macro_rules! usage {
    (__docs) => {};
    ($($tt:tt)*) => {
        compile_error! {r#"usage:  include_cpp! {
                   #include "path/to/header.h"
                   allow!(...)
                   allow_pod!(...)
               }
"#}
    };
}

#[doc(hidden)]
pub use autocxx_macro::include_cpp_impl;
