// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#![forbid(unsafe_code)]

use autocxx_parser::{IncludeCpp, SubclassAttrs};
use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use proc_macro_error::{abort, abort_call_site, proc_macro_error};
use quote::{quote, ToTokens};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::token::Comma;
use syn::{
    parse_macro_input, parse_quote, Expr, Fields, FnArg, Item, ItemFn, ItemStruct, Visibility,
};

/// Implementation of the `include_cpp` macro. See documentation for `autocxx` crate.
#[proc_macro_error]
#[proc_macro]
pub fn include_cpp_impl(input: TokenStream) -> TokenStream {
    let include_cpp = parse_macro_input!(input as IncludeCpp);
    TokenStream::from(include_cpp.generate_rs())
}

/// Attribute to state that a Rust `struct` is a C++ subclass.
/// This adds an additional field to the struct which autocxx uses to
/// track a C++ instantiation of this Rust subclass.
#[proc_macro_error]
#[proc_macro_attribute]
pub fn subclass(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut s: ItemStruct =
        syn::parse(item).unwrap_or_else(|_| abort!(Span::call_site(), "Expected a struct"));
    if !matches!(s.vis, Visibility::Public(..)) {
        use syn::spanned::Spanned;
        abort!(s.vis.span(), "Rust subclasses of C++ types must by public");
    }
    let id = &s.ident;
    let cpp_ident = Ident::new(&format!("{}Cpp", id), Span::call_site());
    let input = quote! {
        cpp_peer: autocxx::subclass::CppSubclassCppPeerHolder<ffi:: #cpp_ident>
    };
    let parser = syn::Field::parse_named;
    let new_field = parser.parse2(input).unwrap();
    s.fields = match &mut s.fields {
        Fields::Named(fields) => {
            fields.named.push(new_field);
            s.fields
        },
        Fields::Unit => Fields::Named(parse_quote! {
            {
                #new_field
            }
        }),
        _ => abort!(Span::call_site(), "Expect a struct with named fields - use struct A{} or struct A; as opposed to struct A()"),
    };
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

/// Attribute to state that a Rust type is to be exported to C++
/// in the `extern "Rust"` section of the generated `cxx` bindings.
#[proc_macro_error]
#[proc_macro_attribute]
pub fn extern_rust_type(attr: TokenStream, input: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        abort!(Span::call_site(), "Expected no attributes");
    }
    let i: Item =
        syn::parse(input.clone()).unwrap_or_else(|_| abort!(Span::call_site(), "Expected an item"));
    match i {
        Item::Struct(..) | Item::Enum(..) | Item::Fn(..) => {}
        _ => abort!(Span::call_site(), "Expected a struct or enum"),
    }
    input
}

/// Attribute to state that a Rust function is to be exported to C++
/// in the `extern "Rust"` section of the generated `cxx` bindings.
#[proc_macro_error]
#[proc_macro_attribute]
pub fn extern_rust_function(attr: TokenStream, input: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        abort!(Span::call_site(), "Expected no attributes");
    }
    let i: Item =
        syn::parse(input.clone()).unwrap_or_else(|_| abort!(Span::call_site(), "Expected an item"));
    match i {
        Item::Fn(..) => {}
        _ => abort!(Span::call_site(), "Expected a function"),
    }
    input
}

/// Attribute which should never be encountered in real life.
/// This is something which features in the Rust source code generated
/// by autocxx-bindgen and passed to autocxx-engine, which should never
/// normally be compiled by rustc before it undergoes further processing.
#[proc_macro_error]
#[proc_macro_attribute]
pub fn cpp_semantics(_attr: TokenStream, _input: TokenStream) -> TokenStream {
    abort!(
        Span::call_site(),
        "Please do not attempt to compile this code. \n\
        This code is the output from the autocxx-specific version of bindgen, \n\
        and should be interpreted by autocxx-engine before further usage."
    );
}

/// Derive a `make_unique` method for any constructor which implements `New`
/// and `UniquePtrTarget`.
#[proc_macro_error]
#[proc_macro_attribute]
pub fn derive_make_unique(_attr: TokenStream, input: TokenStream) -> TokenStream {
    // Expect to parse:
    // pub fn new() -> impl autocxx::moveit::new::New<Output = Self> {
    // Need
    // pub fn make_unique() -> impl autocxx::cxx::UniquePtr<Self>
    let mut input: proc_macro2::TokenStream = input.into();
    let mut extra_fn: ItemFn =
        syn::parse2(input.clone()).unwrap_or_else(|_| abort_call_site!("Expected function"));
    let orig_ident = extra_fn.sig.ident;
    let name = orig_ident.to_string().replace("new", "make_unique");
    extra_fn.sig.output = parse_quote! {
        -> autocxx::cxx::UniquePtr<Self>
    };
    extra_fn.sig.ident = Ident::new(&name, orig_ident.span());
    let arg_list = args_from_sig(&extra_fn.sig.inputs);
    extra_fn.block = parse_quote! {
        {
            use autocxx::moveit::EmplaceUnpinned;
            autocxx::cxx::UniquePtr::emplace(Self::#orig_ident(#(#arg_list),*))
        }
    };
    extra_fn.to_tokens(&mut input);
    input.into()
}

fn args_from_sig(params: &Punctuated<FnArg, Comma>) -> impl Iterator<Item = Expr> + '_ {
    params.iter().filter_map(|fnarg| match fnarg {
        syn::FnArg::Receiver(_) => None,
        syn::FnArg::Typed(fnarg) => match &*fnarg.pat {
            syn::Pat::Ident(id) => Some(id_to_expr(&id.ident)),
            _ => None,
        },
    })
}

fn id_to_expr(id: &Ident) -> Expr {
    parse_quote! { #id }
}
