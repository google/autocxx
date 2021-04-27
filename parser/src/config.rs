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

use crate::type_config::TypeConfig;

#[derive(PartialEq, Clone, Debug, Hash)]
pub enum UnsafePolicy {
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

#[derive(Hash, Debug)]
pub struct IncludeCppConfig {
    pub inclusions: Vec<String>,
    pub unsafe_policy: UnsafePolicy,
    pub type_config: TypeConfig,
    pub parse_only: bool,
}

impl Parse for IncludeCppConfig {
    fn parse(input: ParseStream) -> ParseResult<Self> {
        // Takes as inputs:
        // 1. List of headers to include
        // 2. List of #defines to include
        // 3. Allowlist

        let mut inclusions = Vec::new();
        let mut parse_only = false;
        let mut type_config = TypeConfig::new();
        let mut unsafe_policy = UnsafePolicy::AllFunctionsUnsafe;

        while !input.is_empty() {
            let has_hexathorpe = input.parse::<Option<syn::Token![#]>>()?.is_some();
            let ident: syn::Ident = input.parse()?;
            if has_hexathorpe {
                if ident != "include" {
                    return Err(syn::Error::new(ident.span(), "expected include"));
                }
                let hdr: syn::LitStr = input.parse()?;
                inclusions.push(hdr.value());
            } else {
                input.parse::<Option<syn::Token![!]>>()?;
                if ident == "generate" || ident == "generate_pod" {
                    let args;
                    syn::parenthesized!(args in input);
                    let generate: syn::LitStr = args.parse()?;
                    type_config.allowlist.push(generate.value());
                    if ident == "generate_pod" {
                        type_config.pod_requests.push(generate.value());
                    }
                } else if ident == "block" {
                    let args;
                    syn::parenthesized!(args in input);
                    let generate: syn::LitStr = args.parse()?;
                    type_config.blocklist.push(generate.value());
                } else if ident == "parse_only" {
                    parse_only = true;
                } else if ident == "exclude_utilities" {
                    type_config.exclude_utilities = true;
                } else if ident == "safety" {
                    let args;
                    syn::parenthesized!(args in input);
                    unsafe_policy = args.parse()?;
                } else {
                    return Err(syn::Error::new(
                        ident.span(),
                        "expected generate, generate_pod, nested_type, safety or exclude_utilities",
                    ));
                }
            }
            if input.is_empty() {
                break;
            }
        }

        Ok(IncludeCppConfig {
            inclusions,
            unsafe_policy,
            type_config,
            parse_only,
        })
    }
}

#[cfg(test)]
mod parse_tests {
    use crate::config::UnsafePolicy;
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
