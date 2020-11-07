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

use autocxx_engine::IncludeCpp;
use proc_macro::TokenStream;
use proc_macro_error::{abort_call_site, proc_macro_error};
use syn::parse_macro_input;

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
/// Within the brackets of the `include_cpp!(...)` macro, you should provide
/// a list of the following:
///
/// * *Header(filename)*: a header filename to parse and include
/// * *Allow(type or function name)*: a type or function name whose declaration
///   should be made available to C++.
/// * *AllowPOD(type name)*: a struct name whose declaration
///   should be made available to C++, and whose fields are sufficient simple
///   that they can allow being passed back and forth by value (POD = 'plain old
///   data').
///
/// # How to allow structs
///
/// A C++ struct can be listed under 'Allow' or 'AllowPOD' (or may be implicitly
/// 'Allowed' because it's a type referenced by something else you've allowed.)
///
/// The current plan is to use 'Allow' under normal circumstances, but
/// 'AllowPOD' only for structs where you absolutely do need to pass them
/// truly by value and have direct field access.
/// Some structs can't be represented as POD, e.g. those containing `std::string`
/// due to self-referential pointers. We will always handle such things using
/// `UniquePtr` to an opaque type in Rust, but still allow calling existing C++
/// APIs which take such things by value - we generate automatic
/// unwrappers. This won't work in all cases.
///
/// # Generated code
///
/// You will find that this macro expands to the equivalent of:
///
/// ```
/// mod ffi {
///     pub mod cxxbridge {
///         pub fn do_math(a: u32) -> u32
/// #       { a+3 }
///     }
///
///      pub const kMyCxxConst: i32 = 3;
///
///      pub mod defs {
///          pub const MY_PREPROCESSOR_DEFINITION: i64 = 3i64;
///      }
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

#[proc_macro_error]
#[proc_macro]
pub fn include_cpp_impl(input: TokenStream) -> TokenStream {
    let mut include_cpp = parse_macro_input!(input as IncludeCpp);
    match include_cpp.generate() {
        Ok(_) => TokenStream::from(include_cpp.generate_rs()),
        Err(e) => abort_call_site!(format!("{:?}", e)),
    }
}
