// Copyright 2023 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use syn::{
    AngleBracketedGenericArguments, GenericArgument, Path, PathArguments, PathSegment, Type,
    TypePath, TypeReference,
};

/// Looks in a `core::pin::Pin<&mut Something>` and returns the `Something`
/// if it's found.
/// This code could _almost_ be used from various other places around autocxx
/// but they each have slightly different requirements. Over time we should
/// try to migrate other instances to use this, though.
pub(crate) fn extract_pinned_mutable_reference_type(tp: &TypePath) -> Option<&Type> {
    if !is_pin(tp) {
        return None;
    }
    if let Some(PathSegment {
        arguments: PathArguments::AngleBracketed(AngleBracketedGenericArguments { args, .. }),
        ..
    }) = tp.path.segments.last()
    {
        if args.len() == 1 {
            if let Some(GenericArgument::Type(Type::Reference(TypeReference {
                mutability: Some(_),
                elem,
                ..
            }))) = args.first()
            {
                return Some(elem);
            }
        }
    }
    None
}

/// Whether this type path is a `Pin`
fn is_pin(tp: &TypePath) -> bool {
    if tp.path.segments.len() != 3 {
        return false;
    }
    static EXPECTED_SEGMENTS: &[&[&str]] = &[&["std", "core"], &["pin"], &["Pin"]];

    for (seg, expected_name) in tp.path.segments.iter().zip(EXPECTED_SEGMENTS.iter()) {
        if !expected_name
            .iter()
            .any(|expected_name| seg.ident == expected_name)
        {
            return false;
        }
    }
    true
}

fn marker_for_reference(search_for_rvalue: bool) -> &'static str {
    if search_for_rvalue {
        "__bindgen_marker_RValueReference"
    } else {
        "__bindgen_marker_Reference"
    }
}

pub(crate) fn type_is_reference(ty: &syn::Type, search_for_rvalue: bool) -> bool {
    matches_bindgen_marker(ty, marker_for_reference(search_for_rvalue))
}

fn matches_bindgen_marker(ty: &syn::Type, type_name: &str) -> bool {
    matches!(&ty, Type::Path(TypePath {
                  path: Path { segments, .. },..
               }) if segments.first().map(|seg| seg.ident == type_name).unwrap_or_default())
}

fn unwrap_newtype<'a>(seg: &'a PathSegment, marker_name: &str) -> Option<&'a syn::Type> {
    if seg.ident != marker_name {
        return None;
    }

    let PathArguments::AngleBracketed(ref angle_bracketed_args) = seg.arguments else {
        return None;
    };

    if let GenericArgument::Type(ty) = angle_bracketed_args.args.first()? {
        Some(ty)
    } else {
        None
    }
}

pub(crate) fn unwrap_reference(ty: &TypePath, search_for_rvalue: bool) -> Option<&syn::TypePtr> {
    match unwrap_newtype(
        ty.path.segments.first()?,
        marker_for_reference(search_for_rvalue),
    ) {
        // Our behavior here if we see __bindgen_marker_Reference <something that isn't a pointer>
        // is to ignore the type. This should never happen.
        Some(Type::Ptr(typ)) => Some(typ),
        _ => None,
    }
}

pub(crate) fn unwrap_has_unused_template_param(ty: &TypePath) -> Option<&syn::Type> {
    unwrap_newtype(
        ty.path.segments.first()?,
        "__bindgen_marker_UnusedTemplateParam",
    )
}

pub(crate) fn unwrap_has_opaque(ty: &TypePath) -> Option<&syn::Type> {
    unwrap_newtype(ty.path.segments.first()?, "__bindgen_marker_Opaque")
}
