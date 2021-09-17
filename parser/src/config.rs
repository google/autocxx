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

use std::collections::HashSet;

use proc_macro2::Span;
use syn::{
    parse::{Parse, ParseStream},
    LitStr, Token,
};
use syn::{Ident, Result as ParseResult};

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
pub enum AllowlistItem {
    LiberalIfMissing(String),
    StrictIfMissing(String)
}

impl AllowlistItem {
    fn get(&self) -> &String {
        match self {
            AllowlistItem::LiberalIfMissing(item) => item,
            AllowlistItem::StrictIfMissing(item) => item,
        }
    }
}

/// Allowlist configuration.
#[derive(Hash, Debug)]
pub enum Allowlist {
    Unspecified(Vec<AllowlistItem>),
    All,
    Specific(Vec<AllowlistItem>),
}

impl Allowlist {
    pub(crate) fn push(&mut self, item: LitStr) -> ParseResult<()> {
        match self {
            Allowlist::Unspecified(ref mut uncommitted_list) => {
                let new_list = uncommitted_list
                    .drain(..)
                    .chain(std::iter::once(AllowlistItem::StrictIfMissing(item.value())))
                    .collect();
                *self = Allowlist::Specific(new_list);
            }
            Allowlist::All => {
                return Err(syn::Error::new(
                    item.span(),
                    "use either generate!/generate_pod! or generate_all!, not both.",
                ))
            }
            Allowlist::Specific(list) => list.push(AllowlistItem::StrictIfMissing(item.value())),
        };
        Ok(())
    }

    /// Sometimes we need to push things onto the allowlist without making
    /// a commitment whether we're in `allowlist` or `allowlist_all` mode.
    pub(crate) fn push_uncommitted(&mut self, item: AllowlistItem) {
        match self {
            Allowlist::Unspecified(list) | Allowlist::Specific(list) => 
                list.push(item),
            Allowlist::All => {}
        }
    }

    pub(crate) fn set_all(&mut self, ident: &Ident) -> ParseResult<()> {
        if matches!(self, Allowlist::Specific(..)) {
            return Err(syn::Error::new(
                ident.span(),
                "use either generate!/generate_pod! or generate_all!, not both.",
            ));
        }
        *self = Allowlist::All;
        Ok(())
    }
}

impl Default for Allowlist {
    fn default() -> Self {
        Allowlist::Unspecified(Vec::new())
    }
}

#[derive(Hash, Debug)]
pub struct Subclass {
    pub superclass: String,
    pub subclass: Ident,
}

#[derive(Hash, Debug)]
pub struct IncludeCppConfig {
    pub inclusions: Vec<String>,
    pub unsafe_policy: UnsafePolicy,
    pub parse_only: bool,
    pub exclude_impls: bool,
    pod_requests: Vec<String>,
    allowlist: Allowlist,
    blocklist: Vec<String>,
    exclude_utilities: bool,
    mod_name: Option<Ident>,
    pub rust_types: Vec<Ident>,
    pub subclasses: Vec<Subclass>,
}

impl Parse for IncludeCppConfig {
    fn parse(input: ParseStream) -> ParseResult<Self> {
        // Takes as inputs:
        // 1. List of headers to include
        // 2. List of #defines to include
        // 3. Allowlist

        let mut inclusions = Vec::new();
        let mut parse_only = false;
        let mut exclude_impls = false;
        let mut unsafe_policy = UnsafePolicy::AllFunctionsUnsafe;
        let mut allowlist = Allowlist::default();
        let mut blocklist = Vec::new();
        let mut pod_requests = Vec::new();
        let mut rust_types = Vec::new();
        let mut exclude_utilities = false;
        let mut mod_name = None;
        let mut subclasses = Vec::new();

        while !input.is_empty() {
            let has_hexathorpe = input.parse::<Option<syn::token::Pound>>()?.is_some();
            let ident: syn::Ident = input.parse()?;
            if has_hexathorpe {
                if ident != "include" {
                    return Err(syn::Error::new(ident.span(), "expected include"));
                }
                let hdr: syn::LitStr = input.parse()?;
                inclusions.push(hdr.value());
            } else {
                input.parse::<Option<syn::token::Bang>>()?;
                if ident == "generate" {
                    let args;
                    syn::parenthesized!(args in input);
                    let generate: syn::LitStr = args.parse()?;
                    allowlist.push(generate)?;
                } else if ident == "generate_pod" {
                    let args;
                    syn::parenthesized!(args in input);
                    let generate_pod: syn::LitStr = args.parse()?;
                    pod_requests.push(generate_pod.value());
                    allowlist.push(generate_pod)?;
                } else if ident == "pod" {
                    let args;
                    syn::parenthesized!(args in input);
                    let pod: syn::LitStr = args.parse()?;
                    pod_requests.push(pod.value());
                } else if ident == "block" {
                    let args;
                    syn::parenthesized!(args in input);
                    let generate: syn::LitStr = args.parse()?;
                    blocklist.push(generate.value());
                } else if ident == "rust_type" {
                    let args;
                    syn::parenthesized!(args in input);
                    let ident: syn::Ident = args.parse()?;
                    rust_types.push(ident);
                } else if ident == "subclass" {
                    let args;
                    syn::parenthesized!(args in input);
                    let superclass: syn::LitStr = args.parse()?;
                    args.parse::<syn::token::Comma>()?;
                    let subclass: syn::Ident = args.parse()?;
                    let sub_string = subclass.to_string();
                    let sub_string_cpp = format!("{}Cpp", sub_string);
                    subclasses.push(Subclass {
                        superclass: superclass.value(),
                        subclass,
                    });
                    allowlist.push_uncommitted(AllowlistItem::StrictIfMissing(superclass.value()));
                    allowlist.push_uncommitted(AllowlistItem::StrictIfMissing(sub_string));
                    allowlist.push_uncommitted(AllowlistItem::LiberalIfMissing(sub_string_cpp));
                } else if ident == "parse_only" {
                    parse_only = true;
                    swallow_parentheses(&input, &ident)?;
                } else if ident == "exclude_impls" {
                    exclude_impls = true;
                    swallow_parentheses(&input, &ident)?;
                } else if ident == "generate_all" {
                    allowlist.set_all(&ident)?;
                    swallow_parentheses(&input, &ident)?;
                } else if ident == "name" {
                    let args;
                    syn::parenthesized!(args in input);
                    let ident: syn::Ident = args.parse()?;
                    mod_name = Some(ident);
                } else if ident == "exclude_utilities" {
                    exclude_utilities = true;
                    swallow_parentheses(&input, &ident)?;
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
            parse_only,
            exclude_impls,
            pod_requests,
            rust_types,
            allowlist,
            blocklist,
            exclude_utilities,
            mod_name,
            subclasses,
        })
    }
}

fn swallow_parentheses(input: &ParseStream, latest_ident: &Ident) -> ParseResult<()> {
    let args;
    syn::parenthesized!(args in input);
    if args.is_empty() {
        Ok(())
    } else {
        Err(syn::Error::new(
            latest_ident.span(),
            "expected no arguments to directive",
        ))
    }
}

impl IncludeCppConfig {
    pub fn get_pod_requests(&self) -> &[String] {
        &self.pod_requests
    }

    pub fn get_mod_name(&self) -> Ident {
        self.mod_name
            .as_ref()
            .cloned()
            .unwrap_or_else(|| Ident::new("ffi", Span::call_site()))
    }

    /// Whether to avoid generating the standard helpful utility
    /// functions which we normally include in every mod.
    pub fn exclude_utilities(&self) -> bool {
        self.exclude_utilities
    }

    /// Items which the user has explicitly asked us to generate;
    /// we should raise an error if we weren't able to do so.
    pub fn must_generate_list(&self) -> Box<dyn Iterator<Item = String> + '_> {
        if let Allowlist::Specific(items) = &self.allowlist {
            Box::new(items.iter().map(|item| match item {
                AllowlistItem::LiberalIfMissing(_) => None,
                AllowlistItem::StrictIfMissing(item) => Some(item),
            }).flatten().chain(self.pod_requests.iter()).cloned())
        } else {
            Box::new(self.pod_requests.iter().cloned())
        }
    }

    /// The allowlist of items to be passed into bindgen, if any.
    pub fn bindgen_allowlist(&self) -> Option<Box<dyn Iterator<Item = String> + '_>> {
        match &self.allowlist {
            Allowlist::All => None,
            Allowlist::Specific(items) => Some(Box::new(
                items
                    .iter()
                    .map(AllowlistItem::get)
                    .chain(self.pod_requests.iter())
                    .cloned()
                    .chain(self.active_utilities()),
            )),
            Allowlist::Unspecified(_) => unreachable!(),
        }
    }

    fn active_utilities(&self) -> Vec<String> {
        if self.exclude_utilities {
            Vec::new()
        } else {
            vec![self.get_makestring_name()]
        }
    }

    /// Whether this type is on the allowlist specified by the user.
    ///
    /// A note on the allowlist handling in general. It's used in two places:
    /// 1) As directives to bindgen
    /// 2) After bindgen has generated code, to filter the APIs which
    ///    we pass to cxx.
    /// This second pass may seem redundant. But sometimes bindgen generates
    /// unnecessary stuff.
    pub fn is_on_allowlist(&self, cpp_name: &str) -> bool {
        match self.bindgen_allowlist() {
            None => true,
            Some(mut items) => {
                items.any(|item| item == cpp_name)
                    || self.active_utilities().iter().any(|item| *item == cpp_name)
            }
        }
    }

    pub fn is_on_blocklist(&self, cpp_name: &str) -> bool {
        self.blocklist.contains(&cpp_name.to_string())
    }

    pub fn get_blocklist(&self) -> impl Iterator<Item = &String> {
        self.blocklist.iter()
    }

    pub fn get_makestring_name(&self) -> String {
        format!(
            "autocxx_make_string_{}",
            self.mod_name
                .as_ref()
                .map(|i| i.to_string())
                .unwrap_or_else(|| "default".into())
        )
    }

    pub fn is_rust_type(&self, id: &Ident) -> bool {
        self.rust_types.contains(id)
    }

    pub fn superclasses(&self) -> impl Iterator<Item = &String> {
        let mut uniquified = HashSet::new();
        uniquified.extend(self.subclasses.iter().map(|sc| &sc.superclass));
        uniquified.into_iter()
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
