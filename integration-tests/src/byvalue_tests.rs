// Copyright 2021 Google LLC
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

use crate::test_utils::run_test_ex;
use indoc::indoc;
use proc_macro2::TokenStream;
use quote::quote;

fn run_byvalue_test(
    cxx_code: &str,
    header_code: &str,
    rust_code: TokenStream,
    generate_byvalue: &[&str],
) {
    let directives = generate_byvalue.iter().map(|s| {
        quote! {
            generate_byvalue!(#s)
        }
    });
    let directives = quote! {
        #(#directives)*
    };
    run_test_ex(
        cxx_code,
        header_code,
        rust_code,
        directives,
        None,
        None,
        None,
    )
}

#[test]
fn test_return_nontrvial() {
    let hdr = indoc! {"
    class NonTrivial {
    public:
        NonTrivial() {}
        ~NonTrivial() {}
        NonTrivial(NonTrivial&&) {}
    };
    NonTrivial get() {
        NonTrivial a;
        return a;
    }
    "};
    let rs = quote! {
        ffi::get();
    };
    run_byvalue_test("", hdr, rs, &["get"]);
}
