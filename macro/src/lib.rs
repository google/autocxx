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

#![feature(proc_macro_diagnostic)]
#![feature(proc_macro_span)]


mod engine;

use proc_macro::TokenStream;

use syn::parse_macro_input;

use engine::IncludeCpp;

#[proc_macro]
pub fn include_cxx(input: TokenStream) -> TokenStream {
    let include_cpp = parse_macro_input!(input as IncludeCpp);

    let ts = include_cpp.run();
    // TODO: consider that this quoted section invokes a different procedural
    // macro and what that means.
    TokenStream::from(ts)
}
