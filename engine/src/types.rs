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

use indoc::indoc;
use lazy_static::lazy_static;
use proc_macro2::Span;
use std::collections::HashMap;
use std::fmt::Display;
use syn::{Ident, Type, TypePath};

/// Any time we store a type name, we should use this.
/// At the moment it's just a string, but one day it will need to become
/// sufficiently intelligent to handle namespaces.
/// This should store the canonical Rust-side name, e.g.
/// u32, or CxxString. Not uint32_t, nor std_string, etc.
#[derive(Debug, PartialEq, PartialOrd, Eq, Hash, Clone)]
pub struct TypeName(String);

impl TypeName {
    pub(crate) fn from_ident(id: &Ident) -> Self {
        TypeName::new(&id.to_string())
    }

    pub(crate) fn from_type_path(p: &TypePath) -> Self {
        // TODO better handle generics, multi-segment paths, etc.
        TypeName::from_ident(TypeName::parse_type_path(p))
    }

    pub(crate) fn from_type(ty: &Type) -> Self {
        match ty {
            Type::Path(typ) => TypeName::from_type_path(typ),
            _ => panic!("Stringifying unknown type, not yet supported"), // TODO
        }
    }

    pub(crate) fn new(id: &str) -> Self {
        let canonical_name = DEADNAME_MAP.get(id);
        if let Some(canonical_name) = canonical_name {
            // This is already a cxx replacement name, e.g. CxxString.
            TypeName(canonical_name.into())
        } else {
            TypeName(id.into())
        }
    }

    pub(crate) fn to_ident(&self) -> Ident {
        Ident::new(&self.0, Span::call_site())
    }

    pub(crate) fn to_cpp_name(&self) -> &str {
        match KNOWN_TYPES.get(&self).and_then(|x| x.cpp_name.as_ref()) {
            None => &self.0,
            Some(replacement) => &replacement.as_str(),
        }
    }

    /// Whether the given function name is prefixed by this type name
    /// and an underscore.
    /// If so, returns the suffix after that point.
    pub(crate) fn prefixes<'a>(&self, func_name: &'a str) -> Option<&'a str> {
        if func_name.starts_with(&self.0) {
            Some(&func_name[self.0.len() + 1..])
        } else {
            None
        }
    }

    fn parse_type_path(p: &TypePath) -> &Ident {
        &p.path.segments.last().unwrap().ident
    }
}

impl Display for TypeName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug)]
pub(crate) struct TypeDetails {
    /// This type may be called this in bindgen-generated code.
    /// We want to expunge that Bad Old Name as quickly as possible.
    cpp_deadname: Option<String>,
    /// C++ equivalent name for a Rust type.
    pub(crate) cpp_name: Option<String>,
    /// Whether this can be safely represented by value.
    pub(crate) by_value_safe: bool,
}

impl TypeDetails {
    fn new(cpp_deadname: Option<String>, cpp_name: Option<String>, by_value_safe: bool) -> Self {
        TypeDetails {
            cpp_deadname,
            cpp_name,
            by_value_safe,
        }
    }
}

lazy_static! {
    pub(crate) static ref KNOWN_TYPES: HashMap<TypeName, TypeDetails> = {
        let mut map = HashMap::new();
        map.insert(
            TypeName::new("UniquePtr"),
            TypeDetails::new(Some("std_unique_ptr".into()), None, true),
        );
        map.insert(
            TypeName::new("CxxString"),
            TypeDetails::new(Some("std_string".into()), Some("std::string".into()), false),
        );
        for (cpp_type, rust_type) in (3..7)
            .map(|x| 2i32.pow(x))
            .map(|x| {
                vec![
                    (format!("uint{}_t", x), format!("u{}", x)),
                    (format!("int{}_t", x), format!("i{}", x)),
                ]
            })
            .flatten()
        {
            map.insert(
                TypeName::new(&rust_type),
                TypeDetails::new(None, Some(cpp_type), true),
            );
        }
        map
    };
}

lazy_static! {
    static ref DEADNAME_MAP: HashMap<String, String> = {
        let mut map = HashMap::new();
        map.insert("std_unique_ptr".into(), "UniquePtr".into());
        map.insert("std_string".into(), "CxxString".into());
        map
    };
}

pub(crate) fn get_prelude() -> String {
    return PRELUDE.into();
}

pub(crate) fn to_cpp_name(typ: &Type) -> String {
    match typ {
        Type::Path(ref typ) => TypeName::from_type_path(typ).to_cpp_name().to_string(),
        Type::Reference(ref typr) => {
            let const_bit = match typr.mutability {
                None => "const ",
                Some(_) => "",
            };
            format!(
                "{}{}&",
                const_bit,
                TypeName::from_type(typr.elem.as_ref())
                    .to_cpp_name()
                    .to_string()
            )
        }
        _ => unimplemented!(),
    }
}

/// Prelude of C++ for squirting into bindgen. This configures
/// bindgen to output simpler types to replace some STL types
/// that bindgen just can't cope with. Although we then replace
/// those types with cxx types (e.g. UniquePtr), this intermediate
/// step is still necessary because bindgen can't otherwise
/// give us the templated types (e.g. when faced with the STL
/// unique_ptr, bindgen would normally give us std_unique_ptr
/// as opposed to std_unique_ptr<T>.)
static PRELUDE: &str = indoc! {"
    /**
    * <div rustbindgen=\"true\" replaces=\"std::unique_ptr\">
    */
    template<typename T> class UniquePtr {
        T* ptr;
    };

    /**
    * <div rustbindgen=\"true\" replaces=\"std::string\">
    */
    class CxxString {
        char* str_data;
    };
    \n"};

#[cfg(test)]
mod tests {
    use crate::TypeName;

    #[test]
    fn test_ints() {
        assert_eq!(TypeName::new("i8").to_cpp_name(), "int8_t");
        assert_eq!(TypeName::new("u64").to_cpp_name(), "uint64_t");
    }
}
