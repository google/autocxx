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
use proc_macro_error::{proc_macro_error, abort_call_site};
use syn::parse_macro_input;

/// Include some C++ headers in your Rust project.
///
/// This macro allows you to include one or more C++ headers within
/// your Rust code, and call their functions fairly naturally.
///
/// # Examples
///
/// ```text
/// #include <cstdint>
///
/// uint32_t do_math(uint32_t a);
/// ```
///
/// ```
/// # use autocxx::include_cxx;
/// include_cxx!(
/// #   ParseOnly,
///     Header("input.h"),
///     Allow("do_math"),
/// );
///
/// # mod ffi { pub fn do_math(a: u32) -> u32 { a+3 } }
/// # fn main() {
/// ffi::do_math(3);
/// # }
/// ```
#[proc_macro_error]
#[proc_macro]
pub fn include_cxx(input: TokenStream) -> TokenStream {
    let include_cpp = parse_macro_input!(input as IncludeCpp);
    match include_cpp.generate_rs() {
        Ok(ts) => TokenStream::from(ts),
        Err(e) => abort_call_site!(format!("{:?}", e))
    }
}
