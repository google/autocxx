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

use crate::types::{make_ident, TypeName};
use indoc::indoc;
use lazy_static::lazy_static;
use std::collections::HashMap;
use syn::{parse_quote, Type, TypePath};

/// Whether this type should be included in the 'prelude'
/// passed to bindgen, and if so, how.
#[derive(Debug)]
enum PreludePolicy {
    Exclude,
    IncludeNormal,
    IncludeTemplated,
}

#[derive(Debug)]
enum Qualification {
    None,
    Cxx,
    Autocxx,
    StdPin,
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
    /// Is a C type whose size is not fixed (e.g. int, short)
    is_ctype: bool,
    /// Whether this is a container type cxx allows. Otherwise,
    /// if we encounter such a type as a generic, we'll replace it with
    /// a concrete instantiation.
    is_cxx_container: bool,
    /// Any extra non-canonical names
    extra_non_canonical_name: Option<String>,
    /// Whether this needs to be qualified when used in bindgen
    /// bindings.
    qualification: Qualification,
}

impl TypeDetails {
    fn new(
        rs_name: String,
        cpp_name: String,
        by_value_safe: bool,
        prelude_policy: PreludePolicy,
        de_referencicate: bool,
        is_ctype: bool,
        is_cxx_container: bool,
        extra_non_canonical_name: Option<String>,
        qualification: Qualification,
    ) -> Self {
        TypeDetails {
            rs_name,
            cpp_name,
            by_value_safe,
            prelude_policy,
            de_referencicate,
            is_ctype,
            is_cxx_container,
            extra_non_canonical_name,
            qualification,
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

    fn to_type_path(&self) -> TypePath {
        let segs = self.rs_name.split("::").map(make_ident);
        parse_quote! {
            #(#segs)::*
        }
    }
}

/// Database of known types.
pub(crate) struct TypeDatabase {
    by_rs_name: HashMap<TypeName, TypeDetails>,
    canonical_names: HashMap<TypeName, TypeName>,
}

lazy_static! {
    /// Database of known types.
    pub(crate) static ref KNOWN_TYPES: TypeDatabase = create_type_database();
}

impl TypeDatabase {
    fn get(&self, ty: &TypeName) -> Option<&TypeDetails> {
        // The following line is important. It says that
        // when we encounter something like 'std::unique_ptr'
        // in the bindgen-generated bindings, we'll immediately
        // start to refer to that as 'UniquePtr' henceforth.
        let canonical_name = self.canonical_names.get(ty).unwrap_or(ty);
        self.by_rs_name.get(canonical_name)
    }

    /// Prelude of C++ for squirting into bindgen. This configures
    /// bindgen to output simpler types to replace some STL types
    /// that bindgen just can't cope with. Although we then replace
    /// those types with cxx types (e.g. UniquePtr), this intermediate
    /// step is still necessary because bindgen can't otherwise
    /// give us the templated types (e.g. when faced with the STL
    /// unique_ptr, bindgen would normally give us std_unique_ptr
    /// as opposed to std_unique_ptr<T>.)
    pub(crate) fn get_prelude(&self) -> String {
        itertools::join(
            self.by_rs_name
                .values()
                .filter_map(|t| t.get_prelude_entry()),
            "\n",
        )
    }

    /// Types which are known to be safe (or unsafe) to hold and pass by
    /// value in Rust.
    pub(crate) fn get_pod_safe_types(&self) -> Vec<(TypeName, bool)> {
        self.by_rs_name
            .iter()
            .map(|(tn, td)| (tn.clone(), td.by_value_safe))
            .collect()
    }

    /// Whether this TypePath should be treated as a value in C++
    /// but a reference in Rust. This only applies to rust::Str
    /// (C++ name) which is &str in Rust.
    pub(crate) fn should_dereference_in_cpp(&self, typ: &TypePath) -> bool {
        let tn = TypeName::from_type_path(typ);
        let td = self.get(&tn);
        if let Some(td) = td {
            td.de_referencicate
        } else {
            false
        }
    }

    /// Return the set of paths which should be `use`d in each bindgen mod
    /// (i.e. C++ namespace). Ideally we'd fully qualify all these types instead.
    /// This is https://github.com/google/autocxx/issues/236.
    pub(crate) fn get_bindgen_use_paths(&self) -> impl Iterator<Item = TypePath> + '_ {
        self.by_rs_name.values().filter_map(|td| {
            let id = make_ident(&td.rs_name);
            match &td.qualification {
                Qualification::None => None,
                Qualification::Cxx => Some(parse_quote! { cxx::#id }),
                Qualification::Autocxx => Some(parse_quote! { autocxx::#id }),
                Qualification::StdPin => Some(parse_quote! { std::pin::#id }),
            }
        })
    }

    /// Here we substitute any names which we know are Special from
    /// our type database, e.g. std::unique_ptr -> UniquePtr.
    /// We strip off and ignore
    /// any PathArguments within this TypePath - callers should
    /// put them back again if needs be.
    pub(crate) fn known_type_substitute_path(&self, typ: &TypePath) -> Option<TypePath> {
        let tn = TypeName::from_type_path(typ);
        self.get(&tn).map(|td| td.to_type_path())
    }

    pub(crate) fn special_cpp_name(&self, rs: &TypeName) -> Option<String> {
        self.get(rs).map(|x| x.cpp_name.to_string())
    }

    pub(crate) fn is_known_type(&self, ty: &TypeName) -> bool {
        self.get(ty).is_some()
    }

    pub(crate) fn known_type_type_path(&self, ty: &TypeName) -> Option<TypePath> {
        self.get(ty).map(|td| td.to_type_path())
    }

    pub(crate) fn is_ctype(&self, ty: &TypeName) -> bool {
        self.get(ty).map(|td| td.is_ctype).unwrap_or(false)
    }

    pub(crate) fn is_cxx_acceptable_generic(&self, ty: &TypeName) -> bool {
        self.get(ty).map(|x| x.is_cxx_container).unwrap_or(false)
    }
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
        false,
        true,
        None,
        Qualification::Cxx,
    ));
    do_insert(TypeDetails::new(
        "CxxVector".into(),
        "std::vector".into(),
        false,
        PreludePolicy::IncludeTemplated,
        false,
        false,
        true,
        None,
        Qualification::Cxx,
    ));
    do_insert(TypeDetails::new(
        "SharedPtr".into(),
        "std::shared_ptr".into(),
        true,
        PreludePolicy::IncludeTemplated,
        false,
        false,
        true,
        None,
        Qualification::Cxx,
    ));
    do_insert(TypeDetails::new(
        "CxxString".into(),
        "std::string".into(),
        false,
        PreludePolicy::IncludeNormal,
        false,
        false,
        false,
        None,
        Qualification::Cxx,
    ));
    do_insert(TypeDetails::new(
        "str".into(),
        "rust::Str".into(),
        true,
        PreludePolicy::IncludeNormal,
        true,
        false,
        false,
        None,
        Qualification::None,
    ));
    do_insert(TypeDetails::new(
        "String".into(),
        "rust::String".into(),
        true,
        PreludePolicy::IncludeNormal,
        false,
        false,
        false,
        None,
        Qualification::None,
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
            rust_type,
            cpp_type,
            true,
            PreludePolicy::Exclude,
            false,
            false,
            false,
            None,
            Qualification::None,
        ));
    }
    do_insert(TypeDetails::new(
        "bool".into(),
        "bool".into(),
        true,
        PreludePolicy::Exclude,
        false,
        false,
        false,
        None,
        Qualification::None,
    ));

    do_insert(TypeDetails::new(
        "Pin".into(),
        "Pin".into(),
        true, // because this is actually Pin<&something>
        PreludePolicy::Exclude,
        false,
        false,
        false,
        None,
        Qualification::StdPin,
    ));

    let mut insert_ctype = |cname: &str| {
        let td = TypeDetails::new(
            format!("c_{}", cname),
            cname.into(),
            true,
            PreludePolicy::Exclude,
            false,
            true,
            false,
            Some(format!("std::os::raw::c_{}", cname)),
            Qualification::Autocxx,
        );
        by_rs_name.insert(TypeName::new_from_user_input(&td.rs_name), td);
        let td = TypeDetails::new(
            format!("c_u{}", cname),
            format!("unsigned {}", cname),
            true,
            PreludePolicy::Exclude,
            false,
            true,
            false,
            Some(format!("std::os::raw::c_u{}", cname)),
            Qualification::Autocxx,
        );
        by_rs_name.insert(TypeName::new_from_user_input(&td.rs_name), td);
    };

    insert_ctype("long");
    insert_ctype("int");
    insert_ctype("short");
    insert_ctype("char");

    let td = TypeDetails::new(
        "f32".into(),
        "float".into(),
        true,
        PreludePolicy::Exclude,
        false,
        false,
        false,
        None,
        Qualification::None,
    );
    by_rs_name.insert(TypeName::new_from_user_input(&td.rs_name), td);

    let td = TypeDetails::new(
        "f64".into(),
        "double".into(),
        true,
        PreludePolicy::Exclude,
        false,
        false,
        false,
        None,
        Qualification::None,
    );
    by_rs_name.insert(TypeName::new_from_user_input(&td.rs_name), td);

    let mut by_cppname = HashMap::new();
    for td in by_rs_name.values() {
        let rs_name = TypeName::new_from_user_input(&td.rs_name);
        if let Some(extra_non_canonical_name) = &td.extra_non_canonical_name {
            by_cppname.insert(
                TypeName::new_from_user_input(extra_non_canonical_name),
                rs_name.clone(),
            );
        }
        by_cppname.insert(TypeName::new_from_user_input(&td.cpp_name), rs_name);
    }

    TypeDatabase {
        by_rs_name,
        canonical_names: by_cppname,
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

/// Get the list of types to give to bindgen to ask it _not_ to
/// generate code for.
pub(crate) fn get_initial_blocklist() -> Vec<String> {
    BINDGEN_BLOCKLIST.iter().map(|s| s.to_string()).collect()
}

/// If a given type lacks a copy constructor, we should always use
/// std::move in wrapper functions.
pub(crate) fn type_lacks_copy_constructor(ty: &Type) -> bool {
    // In future we may wish to look this up in KNOWN_TYPES.
    match ty {
        Type::Path(typ) => {
            let tn = TypeName::from_type_path(typ);
            tn.to_cpp_name().starts_with("std::unique_ptr")
        }
        _ => false,
    }
}
