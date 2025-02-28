// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::minisyn::Ident;
use crate::parse_callbacks::CppOriginalName;
use itertools::Itertools;
use proc_macro2::Span;
use quote::ToTokens;
use std::iter::Peekable;
use std::{fmt::Display, sync::Arc};
use syn::{parse_quote, PathSegment, TypePath};
use thiserror::Error;

use crate::known_types::known_types;

pub(crate) fn make_ident<S: AsRef<str>>(id: S) -> Ident {
    Ident::new(id.as_ref(), Span::call_site())
}

/// Newtype wrapper for a C++ namespace.
#[derive(Debug, PartialEq, PartialOrd, Eq, Hash, Clone)]
#[allow(clippy::rc_buffer)]
pub struct Namespace(Arc<Vec<String>>);

impl Namespace {
    pub(crate) fn new() -> Self {
        Self(Arc::new(Vec::new()))
    }

    #[must_use]
    pub(crate) fn push(&self, segment: String) -> Self {
        let mut bigger = (*self.0).clone();
        bigger.push(segment);
        Namespace(Arc::new(bigger))
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|s| s.as_str())
    }

    #[cfg(test)]
    pub(crate) fn from_user_input(input: &str) -> Self {
        Self(Arc::new(input.split("::").map(|x| x.to_string()).collect()))
    }

    pub(crate) fn depth(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn to_cpp_path(&self) -> String {
        self.0.join("::")
    }
}

impl Display for Namespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_cpp_path())
    }
}

impl<'a> IntoIterator for &'a Namespace {
    type Item = &'a String;

    type IntoIter = std::slice::Iter<'a, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Any time we store a qualified name, we should use this. Stores the type
/// and its namespace. Namespaces should be stored without any
/// 'bindgen::root' prefix; that means a type not in any C++
/// namespace should have an empty namespace segment list.
/// Some types have names that change as they flow through the
/// autocxx pipeline. e.g. you start with std::string
/// and end up with CxxString. This TypeName type can store
/// either. It doesn't directly have functionality to convert
/// from one to the other; `replace_type_path_without_arguments`
/// does that.
#[derive(PartialEq, PartialOrd, Eq, Hash, Clone)]
pub struct QualifiedName(Namespace, String);

impl QualifiedName {
    /// From a TypePath which starts with 'root'
    pub(crate) fn from_type_path(typ: &TypePath) -> Self {
        let mut seg_iter = typ.path.segments.iter().peekable();
        let first_seg = seg_iter.next().unwrap().ident.clone();
        if first_seg == "root" || first_seg == "output" {
            // This is a C++ type prefixed with a namespace,
            // e.g. std::string or something the user has defined.
            Self::from_segments(seg_iter) // all but 'root'
        } else {
            // This is actually a Rust type e.g.
            // std::os::raw::c_ulong. Start iterating from the beginning again.
            Self::from_segments(typ.path.segments.iter().peekable())
        }
    }

    fn from_segments<'a, T: Iterator<Item = &'a PathSegment>>(mut seg_iter: Peekable<T>) -> Self {
        let mut ns = Namespace::new();
        while let Some(seg) = seg_iter.next() {
            if seg_iter.peek().is_some() {
                ns = ns.push(seg.ident.to_string());
            } else {
                return Self(ns, seg.ident.to_string());
            }
        }
        unreachable!()
    }

    /// Create from a type encountered in the code.
    pub(crate) fn new(ns: &Namespace, id: Ident) -> Self {
        Self(ns.clone(), id.to_string())
    }

    /// Create from user input, e.g. a name in an AllowPOD directive.
    pub(crate) fn new_from_cpp_name(id: &str) -> Self {
        let mut seg_iter = id.split("::").peekable();
        let mut ns = Namespace::new();
        while let Some(seg) = seg_iter.next() {
            if seg_iter.peek().is_some() {
                if !seg.to_string().is_empty() {
                    ns = ns.push(seg.to_string());
                }
            } else {
                return Self(ns, seg.to_string());
            }
        }
        unreachable!()
    }

    /// Return the actual type name, without any namespace
    /// qualification. Avoid unless you have a good reason.
    pub(crate) fn get_final_item(&self) -> &str {
        &self.1
    }

    /// cxx doesn't accept names containing double underscores,
    /// but these are OK elsewhere in our output mod.
    pub(crate) fn validate_ok_for_cxx(&self) -> Result<(), InvalidIdentError> {
        validate_ident_ok_for_cxx(self.get_final_item())
    }

    /// Return the actual type name as an [Ident], without any namespace
    /// qualification. Avoid unless you have a good reason.
    pub(crate) fn get_final_ident(&self) -> Ident {
        make_ident(self.get_final_item())
    }

    pub(crate) fn get_namespace(&self) -> &Namespace {
        &self.0
    }

    pub(crate) fn get_bindgen_path_idents(&self) -> Vec<Ident> {
        ["bindgen", "root"]
            .iter()
            .map(make_ident)
            .chain(self.get_root_path_idents())
            .collect()
    }

    pub(crate) fn get_root_path_idents(&self) -> Vec<Ident> {
        self.ns_segment_iter()
            .map(make_ident)
            .chain(std::iter::once(self.get_final_ident()))
            .collect()
    }

    /// Output the fully-qualified C++ name of this type.
    pub(crate) fn to_cpp_name(&self) -> String {
        let special_cpp_name = known_types().special_cpp_name(self);
        match special_cpp_name {
            Some(name) => name,
            None => self
                .0
                .iter()
                .chain(std::iter::once(self.1.as_str()))
                .join("::"),
        }
    }

    /// Generates a type path prefixed with `output::`
    pub(crate) fn to_type_path(&self) -> TypePath {
        if let Some(known_type_path) = known_types().known_type_type_path(self) {
            known_type_path
        } else {
            let segs = std::iter::once("output")
                .chain(self.ns_segment_iter())
                .chain(std::iter::once(self.1.as_str()))
                .map(make_ident);
            parse_quote! {
                #(#segs)::*
            }
        }
    }

    /// Iterator over segments in the namespace of this name.
    pub(crate) fn ns_segment_iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter()
    }

    /// Iterate over all segments of this name.
    pub(crate) fn segment_iter(&self) -> impl Iterator<Item = &str> {
        self.ns_segment_iter()
            .chain(std::iter::once(self.get_final_item()))
    }
}

impl Display for QualifiedName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for seg in &self.0 {
            f.write_str(seg)?;
            f.write_str("::")?;
        }
        f.write_str(&self.1)
    }
}

impl std::fmt::Debug for QualifiedName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self, f)
    }
}

/// Problems representing C++ identifiers in a way which is compatible with
/// cxx.
#[derive(Error, Clone, Debug)]
pub enum InvalidIdentError {
    #[error("Union are not supported by autocxx (and their bindgen names have __ so are not acceptable to cxx)")]
    Union,
    #[error("Bitfields are not supported by autocxx (and their bindgen names have __ so are not acceptable to cxx)")]
    Bitfield,
    #[error("Names containing __ are reserved by C++ so not acceptable to cxx")]
    TooManyUnderscores,
    #[error("bindgen decided to call this type _bindgen_ty_N because it couldn't deduce the correct name for it. That means we can't generate C++ bindings to it.")]
    BindgenTy,
    #[error("The item name '{0}' is a reserved word in Rust.")]
    ReservedName(String),
}

/// cxx doesn't allow identifiers containing __. These are OK elsewhere
/// in our output mod. It would be nice in future to think of a way we
/// can enforce this using the Rust type system, e.g. a newtype
/// wrapper for a CxxCompatibleIdent which is used in any context
/// where code will be output as part of the `#[cxx::bridge]` mod.
pub fn validate_ident_ok_for_cxx(id: &str) -> Result<(), InvalidIdentError> {
    validate_str_ok_for_rust(id)?;
    // Provide a couple of more specific diagnostics if we can.
    if id.starts_with("__BindgenBitfieldUnit") {
        Err(InvalidIdentError::Bitfield)
    } else if id.starts_with("__BindgenUnionField") {
        Err(InvalidIdentError::Union)
    } else if id.contains("__") && !id.starts_with("__bindgen_marker") {
        Err(InvalidIdentError::TooManyUnderscores)
    } else if id.starts_with("_bindgen_ty_") {
        Err(InvalidIdentError::BindgenTy)
    } else {
        Ok(())
    }
}

pub fn validate_ident_ok_for_rust(label: &CppOriginalName) -> Result<(), InvalidIdentError> {
    validate_str_ok_for_rust(label.for_validation())
}

fn validate_str_ok_for_rust(label: &str) -> Result<(), InvalidIdentError> {
    let id = make_ident(label);
    syn::parse2::<syn::Ident>(id.into_token_stream())
        .map_err(|_| InvalidIdentError::ReservedName(label.to_string()))
        .map(|_| ())
}

/// When we're given a name like `some_function_bindgen_original1` returns
/// `some_function1`
pub(crate) fn strip_bindgen_original_suffix(effective_fun_name: &str) -> String {
    let bindgen_original_re = regex_static::static_regex!(r"(.*)_bindgen_original(\d*)");
    bindgen_original_re
        .captures(effective_fun_name)
        .map(|m| {
            format!(
                "{}{}",
                m.get(1).unwrap().as_str(),
                m.get(2).unwrap().as_str()
            )
        })
        .unwrap_or_else(|| effective_fun_name.to_string())
}

/// When we're given a name like `some_function_bindgen_original1` returns
/// `some_function1`
pub(crate) fn strip_bindgen_original_suffix_from_ident(
    effective_fun_name: &syn::Ident,
) -> syn::Ident {
    make_ident(strip_bindgen_original_suffix(
        &effective_fun_name.to_string(),
    ))
    .0
}

#[cfg(test)]
mod tests {
    use crate::types::strip_bindgen_original_suffix;

    use super::QualifiedName;

    #[test]
    fn test_ints() {
        assert_eq!(
            QualifiedName::new_from_cpp_name("i8").to_cpp_name(),
            "int8_t"
        );
        assert_eq!(
            QualifiedName::new_from_cpp_name("u64").to_cpp_name(),
            "uint64_t"
        );
    }

    #[test]
    fn test_strip() {
        assert_eq!(strip_bindgen_original_suffix("foo"), "foo");
        assert_eq!(strip_bindgen_original_suffix("foo_bindgen_original"), "foo");
        assert_eq!(
            strip_bindgen_original_suffix("foo_bindgen_original1"),
            "foo1"
        );
        assert_eq!(
            strip_bindgen_original_suffix("foo_bindgen_original1234"),
            "foo1234"
        );
    }
}
