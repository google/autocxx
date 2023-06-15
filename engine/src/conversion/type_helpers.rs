// Copyright 2023 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use syn::{
    AngleBracketedGenericArguments, GenericArgument, PathArguments, PathSegment, Type, TypePath,
    TypeReference,
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
