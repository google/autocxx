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

use itertools::Itertools;
use proc_macro2::Span;
use std::iter::Peekable;
use std::{fmt::Display, sync::Arc};
use syn::{parse_quote, Ident, PathSegment, TypePath};

use crate::known_types::is_known_type;

pub(crate) fn make_ident(id: &str) -> Ident {
    Ident::new(id, Span::call_site())
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

    pub(crate) fn iter(&self) -> impl Iterator<Item = &String> {
        self.0.iter()
    }

    #[cfg(test)]
    pub(crate) fn from_user_input(input: &str) -> Self {
        Self(Arc::new(input.split("::").map(|x| x.to_string()).collect()))
    }

    pub(crate) fn depth(&self) -> usize {
        self.0.len()
    }
}

impl Display for Namespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.join("::"))
    }
}

impl<'a> IntoIterator for &'a Namespace {
    type Item = &'a String;

    type IntoIter = std::slice::Iter<'a, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Any time we store a type name, we should use this. Stores the type
/// and its namespace. Namespaces should be stored without any
/// 'bindgen::root' prefix; that means a type not in any C++
/// namespace should have an empty namespace segment list.
/// Some types have names that change as they flow through the
/// autocxx pipeline. e.g. you start with std::string
/// and end up with CxxString. This TypeName type can store
/// either. It doesn't directly have functionality to convert
/// from one to the other; `replace_type_path_without_arguments`
/// does that.
#[derive(Debug, PartialEq, PartialOrd, Eq, Hash, Clone)]
pub struct TypeName(Namespace, String);

impl TypeName {
    pub(crate) fn from_ident(id: &Ident) -> Self {
        Self(Namespace::new(), id.to_string())
    }

    /// From a TypePath which starts with 'root'
    pub(crate) fn from_type_path(typ: &TypePath) -> Self {
        let mut seg_iter = typ.path.segments.iter().peekable();
        let first_seg = seg_iter.next().unwrap().ident.clone();
        if first_seg == "root" {
            // This is a C++ type prefixed with a namespace,
            // e.g. std::string or something the user has defined.
            Self::from_segments(seg_iter) // all but 'root'
        } else if first_seg == "std" {
            // This is actually a Rust type e.g.
            // std::os::raw::c_ulong. Start iterating from the beginning again.
            Self::from_segments(typ.path.segments.iter().peekable())
        } else {
            // This is a primitive e.g. u32
            if seg_iter.next().is_some() {
                // Oh, dear.
                let type_name = typ
                    .path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .join("::");
                panic!(
                    "Unable to handle type found in bindgen output: {}",
                    type_name
                );
            }
            Self::from_ident(&first_seg)
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
    pub(crate) fn new(ns: &Namespace, id: &str) -> Self {
        Self(ns.clone(), id.to_string())
    }

    /// Create from user input, e.g. a name in an AllowPOD directive.
    pub(crate) fn new_from_user_input(id: &str) -> Self {
        let mut seg_iter = id.split("::").peekable();
        let mut ns = Namespace::new();
        while let Some(seg) = seg_iter.next() {
            if seg_iter.peek().is_some() {
                ns = ns.push(seg.to_string());
            } else {
                return Self(ns, seg.to_string());
            }
        }
        unreachable!()
    }

    /// Return the actual type name, without any namespace
    /// qualification. Avoid unless you have a good reason.
    pub(crate) fn get_final_ident(&self) -> &str {
        &self.1
    }

    pub(crate) fn has_namespace(&self) -> bool {
        !self.0.is_empty()
    }

    pub(crate) fn get_namespace(&self) -> &Namespace {
        &self.0
    }

    /// Output the fully-qualified C++ name of this type.
    pub(crate) fn to_cpp_name(&self) -> String {
        let special_cpp_name = crate::known_types::special_cpp_name(&self);
        match special_cpp_name {
            Some(name) => name,
            None => {
                let mut s = String::new();
                for seg in &self.0 {
                    s.push_str(&seg);
                    s.push_str("::");
                }
                s.push_str(&self.1);
                s
            }
        }
    }

    pub(crate) fn to_type_path(&self) -> TypePath {
        if is_known_type(self) {
            let id = make_ident(&self.1);
            parse_quote! {
                #id
            }
        } else {
            let root = "root".to_string();
            let segs = std::iter::once(&root)
                .chain(self.ns_segment_iter())
                .chain(std::iter::once(&self.1))
                .map(|x| make_ident(x));
            parse_quote! {
                #(#segs)::*
            }
        }
    }

    /// Iterator over segments in the namespace of this type.
    pub(crate) fn ns_segment_iter(&self) -> impl Iterator<Item = &String> {
        self.0.iter()
    }
}

impl Display for TypeName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for seg in &self.0 {
            f.write_str(&seg)?;
            f.write_str("::")?;
        }
        f.write_str(&self.1)
    }
}

#[cfg(test)]
mod tests {
    use crate::TypeName;

    #[test]
    fn test_ints() {
        assert_eq!(TypeName::new_from_user_input("i8").to_cpp_name(), "int8_t");
        assert_eq!(
            TypeName::new_from_user_input("u64").to_cpp_name(),
            "uint64_t"
        );
    }
}
