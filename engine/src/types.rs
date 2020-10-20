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
use std::iter::Peekable;
use syn::parse_quote;
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
        let td = KNOWN_TYPES.by_rs_name.get(&self);
        if let Some(td) = td {
            td.cpp_name.to_string()
        } else {
            let mut s = String::new();
            for seg in &self.0 {
                s.push_str(&seg);
                s.push_str("::");
            }
            s.push_str(&self.1);
            s
        }
    }

    /// Iterator over segments in the namespace of this type.
    pub(crate) fn ns_segment_iter(&self) -> impl Iterator<Item = &String> {
        self.0.iter()
    }
}

fn make_ident(id: &str) -> Ident {
    Ident::new(id, Span::call_site())
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

/// Whether this type should be included in the 'prelude'
/// passed to bindgen, and if so, how.
#[derive(Debug)]
enum PreludePolicy {
    Exclude,
    IncludeNormal,
    IncludeTemplated,
}

/// Details about known special types, mostly primitives.
#[derive(Debug)]
struct TypeDetails {
    /// The name used by cxx (in Rust code) for this type.
    rs_name: String,
    /// C++ equivalent name for a Rust type.
    cpp_name: String,
    /// Whether this can be safely represented by value.
    by_value_safe: bool,
    /// Whether and how to include this in the prelude given to bindgen.
    prelude_policy: PreludePolicy,
    /// Whether this is a & on the Rust side but a value on the C++
    /// side. Only applies to &str.
    de_referencicate: bool,
}

impl TypeDetails {
    fn new(
        rs_name: String,
        cpp_name: String,
        by_value_safe: bool,
        prelude_policy: PreludePolicy,
        de_referencicate: bool,
    ) -> Self {
        TypeDetails {
            rs_name,
            cpp_name,
            by_value_safe,
            prelude_policy,
            de_referencicate,
        }
    }

    fn get_prelude_entry(&self) -> Option<String> {
        match self.prelude_policy {
            PreludePolicy::Exclude => None,
            PreludePolicy::IncludeNormal | PreludePolicy::IncludeTemplated => {
                let cxx_name = &self.rs_name;
                let (templating, payload) = match self.prelude_policy {
                    PreludePolicy::IncludeNormal => ("", "char* ptr"),
                    PreludePolicy::IncludeTemplated => ("template<typename T> ", "T* ptr"),
                    _ => unreachable!(),
                };
                Some(format!(
                    indoc! {"
                    /**
                    * <div rustbindgen=\"true\" replaces=\"{}\">
                    */
                    {}class {} {{
                        {};
                    }};

                    "},
                    self.cpp_name, templating, cxx_name, payload
                ))
            }
        }
    }
}

/// Database of known types.
struct TypeDatabase {
    by_rs_name: HashMap<TypeName, TypeDetails>,
    by_cppname: HashMap<String, String>,
}

lazy_static! {
    /// Database of known types.
    static ref KNOWN_TYPES: TypeDatabase = create_type_database();
}

fn create_type_database() -> TypeDatabase {
    let mut by_rs_name = HashMap::new();

    let mut do_insert =
        |td: TypeDetails| by_rs_name.insert(TypeName::new_from_user_input(&td.rs_name), td);

    do_insert(TypeDetails::new(
        "UniquePtr".into(),
        "std::unique_ptr".into(),
        true,
        PreludePolicy::IncludeTemplated,
        false,
    ));
    do_insert(TypeDetails::new(
        "CxxString".into(),
        "std::string".into(),
        false,
        PreludePolicy::IncludeNormal,
        false,
    ));
    do_insert(TypeDetails::new(
        "str".into(),
        "rust::Str".into(),
        true,
        PreludePolicy::IncludeNormal,
        true,
    ));
    do_insert(TypeDetails::new(
        "String".into(),
        "rust::String".into(),
        true,
        PreludePolicy::IncludeNormal,
        false,
    ));
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
        do_insert(TypeDetails::new(
            rust_type.into(),
            cpp_type,
            true,
            PreludePolicy::Exclude,
            false,
        ));
    }

    let mut by_cppname = HashMap::new();
    for td in by_rs_name.values() {
        by_cppname.insert(td.cpp_name.clone(), td.rs_name.clone());
    }

    TypeDatabase {
        by_rs_name,
        by_cppname,
    }
}

/// This is worked out basically using trial and error.
/// Excluding std* and rust* is obvious, but the other items...
/// in theory bindgen ought to be smart enough to work out that
/// they're not used and therefore not generate code for them.
/// But it doesm unless we blocklist them. This is obviously
/// a bit sensitive to the particular STL in use so one day
/// it would be good to dig into bindgen's behavior here - TODO.
const BINDGEN_BLOCKLIST: &[&str] = &["std.*", "__gnu.*", ".*mbstate_t.*", "rust.*"];

/// Prelude of C++ for squirting into bindgen. This configures
/// bindgen to output simpler types to replace some STL types
/// that bindgen just can't cope with. Although we then replace
/// those types with cxx types (e.g. UniquePtr), this intermediate
/// step is still necessary because bindgen can't otherwise
/// give us the templated types (e.g. when faced with the STL
/// unique_ptr, bindgen would normally give us std_unique_ptr
/// as opposed to std_unique_ptr<T>.)
pub(crate) fn get_prelude() -> String {
    itertools::join(
        KNOWN_TYPES
            .by_rs_name
            .values()
            .filter_map(|t| t.get_prelude_entry()),
        "\n",
    )
}

/// Types which are known to be safe (or unsafe) to hold and pass by
/// value in Rust.
pub(crate) fn get_pod_safe_types() -> Vec<(TypeName, bool)> {
    KNOWN_TYPES
        .by_rs_name
        .iter()
        .map(|(_, td)| {
            (
                TypeName::from_ident(&make_ident(&td.rs_name)),
                td.by_value_safe,
            )
        })
        .collect()
}

/// Get the list of types to give to bindgen to ask it _not_ to
/// generate code for.
pub(crate) fn get_initial_blocklist() -> Vec<String> {
    BINDGEN_BLOCKLIST.iter().map(|s| s.to_string()).collect()
}

/// Whether this TypePath should be treated as a value in C++
/// but a reference in Rust. This only applies to rust::Str
/// (C++ name) which is &str in Rust.
pub(crate) fn should_dereference_in_cpp(typ: &TypePath) -> bool {
    let tn = TypeName::from_cxx_type_path(typ);
    let td = KNOWN_TYPES.by_rs_name.get(&tn);
    if let Some(td) = td {
        td.de_referencicate
    } else {
        false
    }
}

/// Here we substitute any names which we know are Special from
/// our type database, e.g. std::unique_ptr -> UniquePtr.
/// The 'without_arguments' bit means we strip off and ignore
/// any PathArguments within this TypePath - callers should
/// put them back again if needs be.
pub(crate) fn replace_type_path_without_arguments(typ: TypePath) -> TypePath {
    let name = TypeName::from_cxx_type_path(&typ).to_cpp_name();
    match KNOWN_TYPES.by_cppname.get(&name) {
        Some(replacement_name) => {
            let id = make_ident(replacement_name);
            parse_quote! {
                #id
            }
        }
        None => typ.clone(),
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
