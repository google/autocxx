// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

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
    // Make an opaque struct. If we have layout information, we pass
    // that through to Rust. We keep only doc attrs, plus add a #[repr(C)]
    // if necessary.
    // Constraints here (thanks to dtolnay@ for this explanation of why the
    // following is needed:)
    // (1) If the real alignment of the C++ type is smaller and a reference
    // is returned from C++ to Rust, mere existence of an insufficiently
    // aligned reference in Rust causes UB even if never dereferenced
    // by Rust code
    // (see https://doc.rust-lang.org/1.47.0/reference/behavior-considered-undefined.html).
    // Rustc can use least-significant bits of the reference for other storage.
    // (if we have layout information from bindgen we use that instead)
    // (2) We want to ensure the type is !Unpin
    // (3) We want to ensure it's not Send or Sync
    // In addition, we want to avoid UB:
    // (4) By marking the data as MaybeUninit we ensure there's no UB
    //     by Rust assuming it's initialized
    // (5) By marking it as UnsafeCell we perhaps help reduce aliasing UB.
    //     This is on the assumption that references to this type may pass
    //     through C++ and get duplicated, so there may be multiple Rust
    //     references to the same underlying data.
    //     The correct solution to this is to put autocxx into the mode
    //     where it uses CppRef<T> instead of Rust references, but otherwise,
    //     using UnsafeCell here may help a bit. It definitely does not
    //     eliminate the UB here for the following reasons:
    //     a) The references floating around are to the outer type, not the
    //        data stored within the UnsafeCell.
    //     b) C++ may have multiple mutable references, or may have mutable
    //        references coexisting with immutable references, and no amount
    //        of UnsafeCell can make that safe.
    //     Nevertheless the use of UnsafeCell here may (*may*) reduce the
    //     opportunities for aliasing UB. Again, the only actual way to
    //     eliminate UB is to use CppRef<T> everywhere instead of &T and &mut T.
    //
    // For opaque types, the Rusty opaque structure could in fact be generated
    // by three different things:
    // a) bindgen, using its --opaque-type command line argument or the library
    //    equivalent;
    // b) us (autocxx), which is what this code does
    // c) cxx, using "type B;" in an "extern "C++"" section
    // We never use (a) because bindgen requires an allowlist of opaque types.
    // Furthermore, it sometimes then discards struct definitions entirely
    // and says "type A = [u8;2];" or something else which makes our life
    // much more difficult.
    // We use (c) for abstract types. For everything else, we do it ourselves
    // for maximal control. See codegen_rs/mod.rs generate_type for more notes.
    //
    // It is worth noting that our constraints here are a bit more severe than
    // for cxx. In the case of cxx, C++ types are usually represented as
    // zero-sized types within Rust. Zero-sized types, by definition, can't
    // have overlapping references and thus can't have aliasing UB. We can't
    // do that because we want C++ types to be representable on the Rust stack,
    // and thus we need to tell Rust their real size and alignment.
    // First work out attributes.
    let doc_attr = s
        .attrs
        .iter()
        .filter(|a| a.path().get_ident().iter().any(|p| *p == "doc"))
        .cloned();
    let repr_attr = if let Some(layout) = &layout {
        let align = make_lit_int(layout.align);
        if layout.packed {
            parse_quote! {
                #[repr(C,align(#align),packed)]
            }
        } else {
            parse_quote! {
                #[repr(C,align(#align))]
            }
        }
    } else {
        parse_quote! {
            #[repr(C, packed)]
        }
    };
    let attrs = doc_attr.chain(std::iter::once(repr_attr));
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
                let field_name = make_ident(format!("_phantom_{counter}"));
                let toks = quote! {
                    #field_name: ::core::marker::PhantomData<::core::cell::UnsafeCell< #id >>
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
                    _data: ::core::cell::UnsafeCell<::core::mem::MaybeUninit<[u8; #size]>>
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
    LitInt::new(&val.to_string(), Span::call_site())
}
