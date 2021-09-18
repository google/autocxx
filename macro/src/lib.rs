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

use autocxx_parser::IncludeCpp;
use proc_macro2::{Ident, Span};
use proc_macro_error::{abort, proc_macro_error};
use quote::{quote, ToTokens};
use syn::parse::{Parse, ParseStream, Parser};
use syn::token::Comma;
use syn::{parse_macro_input, Fields, ItemStruct, Result as ParseResult};


/// Implementation of the `include_cpp` macro. See documentation for `autocxx` crate.
#[proc_macro_error]
#[proc_macro]
pub fn include_cpp_impl(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let include_cpp = parse_macro_input!(input as IncludeCpp);
    proc_macro::TokenStream::from(include_cpp.generate_rs())
}

/// Attribute to state that a Rust `struct` is a C++ subclass.
/// This adds an additional field to the struct which autocxx uses to
/// track a C++ instantiation of this Rust subclass.
#[proc_macro_error]
#[proc_macro_attribute]
pub fn is_subclass(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let mut s: ItemStruct =
        syn::parse(item).unwrap_or_else(|_| abort!(Span::call_site(), "Expected a struct"));
    let id = &s.ident;
    let cpp_ident = Ident::new(&format!("{}Cpp", id.to_string()), Span::call_site());
    match &mut s.fields {
        Fields::Named(fields) => {
            let input = quote! {
                cpp_peer: autocxx::subclass::CppSubclassCppPeerHolder<ffi:: #cpp_ident>
            };
            let parser = syn::Field::parse_named;
            let f = parser.parse2(input).unwrap();
            fields.named.push(f);
        }
        _ => abort!(Span::call_site(), "Expect a struct with named fields - use struct A{} as opposed to struct A; or struct A()"),
    }
    let subclass_attrs: SubclassAttrs = syn::parse(attr)
        .unwrap_or_else(|_| abort!(Span::call_site(), "Unable to parse attributes"));
    let self_owned_bit = if subclass_attrs.self_owned {
        Some(quote! {
            impl autocxx::subclass::CppSubclassSelfOwned<ffi::#cpp_ident> for #id {}
        })
    } else {
        None
    };
    let toks = quote! {
        #s

        impl autocxx::subclass::CppSubclass<ffi::#cpp_ident> for #id {
            fn peer_holder_mut(&mut self) -> &mut autocxx::subclass::CppSubclassCppPeerHolder<ffi::#cpp_ident> {
                &mut self.cpp_peer
            }
            fn peer_holder(&self) -> &autocxx::subclass::CppSubclassCppPeerHolder<ffi::#cpp_ident> {
                &self.cpp_peer
            }
        }

        #self_owned_bit
    };
    toks.into()
}
