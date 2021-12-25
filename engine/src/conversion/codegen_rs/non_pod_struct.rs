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

use crate::conversion::api::Layout;
use crate::types::make_ident;
use proc_macro2::{Ident, Span};
use quote::quote;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::{parse_quote, Field, Fields, GenericParam, ItemStruct, LitInt};

pub(crate) fn new_non_pod_struct(id: Ident) -> ItemStruct {
    let mut s = parse_quote! {
        pub struct #id {
        }
    };
    make_non_pod(&mut s, None);
    s
}

pub(crate) fn make_non_pod(s: &mut ItemStruct, layout: Option<Layout>) {
    // Keep only doc attrs, plus add a #[repr(C,packed)].
    // Thanks to dtolnay@ for this explanation of why the following
    // is needed:
    // If the real alignment of the C++ type is smaller and a reference
    // is returned from C++ to Rust, mere existence of an insufficiently
    // aligned reference in Rust causes UB even if never dereferenced
    // by Rust code
    // (see https://doc.rust-lang.org/1.47.0/reference/behavior-considered-undefined.html).
    // Rustc can use least-significant bits of the reference for other storage.
    let attrs = s
        .attrs
        .iter()
        .filter(|a| a.path.get_ident().iter().any(|p| *p == "doc"))
        .cloned();
    let repr_attr = if let Some(layout) = &layout {
        let align = make_lit_int(layout.align);
        Some(if layout.packed {
            parse_quote! {
                #[repr(C,align(#align),packed)]
            }
        } else {
            parse_quote! {
                #[repr(C,align(#align))]
            }
        })
    } else {
        None
    }
    .into_iter();
    let attrs = attrs.chain(repr_attr);
    s.attrs = attrs.collect();
    // Now fill in fields. Usually, we just want a single field
    // but if this is a generic type we need to faff a bit.
    let generic_type_fields = s
        .generics
        .params
        .iter()
        .enumerate()
        .filter_map(|(counter, gp)| match gp {
            GenericParam::Type(gpt) => {
                let id = &gpt.ident;
                let field_name = make_ident(&format!("_phantom_{}", counter));
                let toks = quote! {
                    #field_name: ::std::marker::PhantomData<::std::cell::UnsafeCell< #id >>
                };
                Some(Field::parse_named.parse2(toks).unwrap())
            }
            _ => None,
        });
    let data_field = if let Some(layout) = layout {
        let size = make_lit_int(layout.size);
        Some(
            syn::Field::parse_named
                .parse2(quote! {
                    _data: [u8; #size]
                })
                .unwrap(),
        )
    } else {
        None
    }
    .into_iter();
    let pin_field = syn::Field::parse_named
        .parse2(quote! {
            _pinned: core::marker::PhantomData<core::marker::PhantomPinned>
        })
        .unwrap();

    let non_send_sync_field = syn::Field::parse_named
        .parse2(quote! {
            _non_send_sync: core::marker::PhantomData<[*const u8;0]>
        })
        .unwrap();
    let all_fields: Punctuated<_, syn::token::Comma> = std::iter::once(pin_field)
        .chain(std::iter::once(non_send_sync_field))
        .chain(generic_type_fields)
        .chain(data_field)
        .collect();
    s.fields = Fields::Named(parse_quote! { {
        #all_fields
    } })
}

fn make_lit_int(val: usize) -> LitInt {
    let size = LitInt::new(&val.to_string(), Span::call_site());
    size
}
