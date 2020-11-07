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

/// Implementation of the `include_cpp` macro. See documentation for `autocxx` crate.
#[proc_macro_error]
#[proc_macro]
pub fn include_cpp_impl(input: TokenStream) -> TokenStream {
    let mut include_cpp = parse_macro_input!(input as IncludeCpp);
    match include_cpp.generate() {
        Ok(_) => TokenStream::from(include_cpp.generate_rs()),
        Err(e) => abort_call_site!(format!("{:?}", e)),
    }
}
