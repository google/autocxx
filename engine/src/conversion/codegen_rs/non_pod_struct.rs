// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use syn::{parse_quote, Attribute, Item};

use crate::types::{make_ident, QualifiedName};

use super::find_output_mod_root;
use quote::quote;

/// Make an opaque wrapper around a bindgen type.
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
//     using UnsafeCell here may help a bit. It probably does not
//     eliminate the UB here for the following reasons:
//     a) The references floating around are to the outer type, not the
//        data stored within the UnsafeCell. (I think this is OK)
//     b) C++ may have multiple mutable references, or may have mutable
//        references coexisting with immutable references, and no amount
//        of UnsafeCell can make that safe.
//     Nevertheless the use of UnsafeCell here may (*may*) reduce the
//     opportunities for aliasing UB. Again, the only actual way to
//     eliminate UB is to use CppRef<T> everywhere instead of &T and &mut T.
//
// For opaque types, the Rusty opaque structure could in fact be generated
// by four different things:
// a) bindgen, using its --opaque-type command line argument or the library
//    equivalent;
// b) us (autocxx), by making a [u8; N] byte long structure
// c) us (autocxx), by making a struct containing the bindgen struct
//    in an inaccessible field (that's what we do here)
// d) cxx, using "type B;" in an "extern "C++"" section
// We never use (a) because bindgen requires an allowlist of opaque types.
// Furthermore, it sometimes then discards struct definitions entirely
// and says "type A = [u8;2];" or something else which makes our life
// much more difficult.
// We use (d) for abstract types. For everything else, we do (c)
// for maximal control. See codegen_rs/mod.rs generate_type for more notes.
// We could switch to (b) and earlier version of autocxx did that.
//
// It is worth noting that our constraints here are a bit more severe than
// for cxx. In the case of cxx, C++ types are usually represented as
// zero-sized types within Rust. Zero-sized types, by definition, can't
// have overlapping references and thus can't have aliasing UB. We can't
// do that because we want C++ types to be representable on the Rust stack,
// and thus we need to tell Rust their real size and alignment.
pub(super) fn generate_opaque_type(
    name: &QualifiedName,
    num_generics: usize,
    doc_attrs: &[Attribute],
) -> Item {
    let segs = find_output_mod_root(name.get_namespace()).chain(name.get_bindgen_path_idents());
    let final_name = name.get_final_ident().0;

    let generics = (0usize..usize::MAX)
        .take(num_generics)
        .map(|num| make_ident(format!("T{num}")).0);
    let generics = if num_generics == 0 {
        quote! {}
    } else {
        quote! {
            < #(#generics),* >
        }
    };
    Item::Struct(parse_quote! {
        #[repr(transparent)]
        #(#doc_attrs)*
        pub struct #final_name #generics {
            _hidden_contents: ::core::cell::UnsafeCell<::core::mem::MaybeUninit<#(#segs)::* #generics>>,
        }
    })
}
