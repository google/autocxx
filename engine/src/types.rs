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

use std::fmt::Display;
use std::iter::Peekable;
use syn::{Ident, PathSegment, Type, TypePath};

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
pub struct TypeName(Vec<String>, String);

impl TypeName {
    pub(crate) fn from_ident(id: &Ident) -> Self {
        Self(Vec::new(), id.to_string())
    }

    /// From a TypePath which does not start with 'root'.
    pub(crate) fn from_cxx_type_path(typ: &TypePath) -> Self {
        let seg_iter = typ.path.segments.iter().peekable();
        Self::from_segments(seg_iter)
    }

    /// From a TypePath which starts with 'root'
    pub(crate) fn from_bindgen_type_path(typ: &TypePath) -> Self {
        let mut seg_iter = typ.path.segments.iter().peekable();
        let first_seg = seg_iter.next().unwrap().ident.clone();
        if first_seg.to_string() == "root" {
            // This is a C++ type prefixed with a namespace,
            // e.g. std::string or something the user has defined.
            Self::from_segments(seg_iter) // all but 'root'
        } else {
            // This is a primitive e.g. u32
            assert!(seg_iter.next().is_none());
            Self::from_ident(&first_seg)
        }
    }

    fn from_segments<'a, T: Iterator<Item = &'a PathSegment>>(mut seg_iter: Peekable<T>) -> Self {
        let mut ns = Vec::new();
        while let Some(seg) = seg_iter.next() {
            if seg_iter.peek().is_some() {
                ns.push(seg.ident.to_string());
            } else {
                return Self(ns, seg.ident.to_string());
            }
        }
        unreachable!()
    }

    fn from_type<F>(ty: &Type, func: F) -> Self
    where
        F: FnOnce(&TypePath) -> Self,
    {
        match ty {
            Type::Path(typ) => func(typ),
            _ => panic!("Stringifying unknown type, not yet supported"), // TODO
        }
    }

    /// From a Type found in bindgen-generated Rust code.
    /// The Type starts with 'root' typically.
    pub(crate) fn from_bindgen_type(ty: &Type) -> Self {
        Self::from_type(ty, Self::from_bindgen_type_path)
    }

    /// From a Type found in code we've already generated.
    pub(crate) fn from_cxx_type(ty: &Type) -> Self {
        Self::from_type(ty, Self::from_cxx_type_path)
    }

    /// Create from a type encountered in the code.
    pub(crate) fn new(ns: &Vec<String>, id: &str) -> Self {
        Self(ns.clone(), id.to_string())
    }

    /// Create from user input, e.g. a name in an AllowPOD directive.
    pub(crate) fn new_from_user_input(id: &str) -> Self {
        let mut seg_iter = id.split("::").peekable();
        let mut ns = Vec::new();
        while let Some(seg) = seg_iter.next() {
            if seg_iter.peek().is_some() {
                ns.push(seg.to_string());
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
