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

use crate::conversion::doc_attr::get_doc_attr;
use crate::types::make_ident;
use proc_macro2::Ident;
use quote::{quote, ToTokens};
use syn::parse::Parser;
use syn::{parse_quote, Field, GenericParam, ItemStruct};

pub(crate) fn new_non_pod_struct(id: Ident) -> ItemStruct {
    let mut s = parse_quote! {
        pub struct #id {
        }
    };
    make_non_pod(&mut s);
    s
}

pub(crate) fn make_non_pod(s: &mut ItemStruct) {
    // Keep only doc attrs, plus add a #[repr(C,packed)].
    // Thanks to dtolnay@ for this explanation of why the following
    // is needed:
    // If the real alignment of the C++ type is smaller and a reference
    // is returned from C++ to Rust, mere existence of an insufficiently
    // aligned reference in Rust causes UB even if never dereferenced
    // by Rust code
    // (see https://doc.rust-lang.org/1.47.0/reference/behavior-considered-undefined.html).
    // Rustc can use least-significant bits of the reference for other storage.
    log::info!("Before: {}", s.to_token_stream());

    let attrs = s
        .attrs
        .iter()
        .filter(|a| {
            a.path
                .get_ident()
                .iter()
                .any(|p| *p == "repr" || *p == "doc")
        })
        .cloned();
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
                let parser = Field::parse_named;
                Some(parser.parse2(toks).unwrap())
            }
            _ => None,
        });
    // See cxx's opaque::Opaque for rationale for this type... in
    // short, it's to avoid being Send/Sync.
    let bindgen_opaque_blob = if let syn::Fields::Named(fieldlist) = &s.fields {
        fieldlist
            .named
            .iter()
            .filter(|f| f.ident.as_ref().unwrap().to_string() == "_bindgen_opaque_blob")
            .next()
    } else {
        None
    };
    s.fields = syn::Fields::Named(parse_quote! {
        {
            #bindgen_opaque_blob
            do_not_attempt_to_allocate_nonpod_types: [*const u8; 0],
            _pinned: core::marker::PhantomData<core::marker::PhantomPinned>,
            #(#generic_type_fields),*
        }
    });
}
