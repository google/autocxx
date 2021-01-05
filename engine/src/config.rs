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

use proc_macro2::Span;
use syn::Result as ParseResult;
use syn::{
    parse::{Parse, ParseStream},
    Token,
};

#[derive(PartialEq, Clone, Debug)]
pub(crate) enum UnsafePolicy {
    AllFunctionsSafe,
    AllFunctionsUnsafe,
}

impl Parse for UnsafePolicy {
    fn parse(input: ParseStream) -> ParseResult<Self> {
        if input.parse::<Option<Token![unsafe]>>()?.is_some() {
            return Ok(UnsafePolicy::AllFunctionsSafe);
        }
        let r = match input.parse::<Option<syn::Ident>>()? {
            Some(id) => {
                if id == "unsafe_ffi" {
                    Ok(UnsafePolicy::AllFunctionsSafe)
                } else {
                    Err(syn::Error::new(id.span(), "expected unsafe_ffi"))
                }
            }
            None => Ok(UnsafePolicy::AllFunctionsUnsafe),
        };
        if !input.is_empty() {
            return Err(syn::Error::new(
                Span::call_site(),
                "unexpected tokens within safety directive",
            ));
        }
        r
    }
}

#[cfg(test)]
mod parse_tests {
    use crate::UnsafePolicy;
    use syn::parse_quote;
    #[test]
    fn test_safety_unsafe() {
        let us: UnsafePolicy = parse_quote! {
            unsafe
        };
        assert_eq!(us, UnsafePolicy::AllFunctionsSafe)
    }

    #[test]
    fn test_safety_unsafe_ffi() {
        let us: UnsafePolicy = parse_quote! {
            unsafe_ffi
        };
        assert_eq!(us, UnsafePolicy::AllFunctionsSafe)
    }

    #[test]
    fn test_safety_safe() {
        let us: UnsafePolicy = parse_quote! {};
        assert_eq!(us, UnsafePolicy::AllFunctionsUnsafe)
    }
}
