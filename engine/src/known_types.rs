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

use crate::{
    conversion::ConvertError,
    types::{make_ident, QualifiedName},
};
use indoc::indoc;
use once_cell::sync::OnceCell;
use std::collections::HashMap;
use syn::{parse_quote, GenericArgument, PathArguments, Type, TypePath, TypePtr};

//// The behavior of the type.
#[derive(Debug)]
enum Behavior {
    CxxContainerByValueSafe,
    CxxContainerNotByValueSafe,
    CxxString,
    RustStr,
    RustString,
    RustByValue,
    CByValue,
    CVariableLengthByValue,
    CVoid,
}

/// Details about known special types, mostly primitives.
#[derive(Debug)]
struct TypeDetails {
    /// The name used by cxx (in Rust code) for this type.
    rs_name: String,
    /// C++ equivalent name for a Rust type.
    cpp_name: String,
    //// The behavior of the type.
    behavior: Behavior,
    /// Any extra non-canonical names
    extra_non_canonical_name: Option<String>,
}

impl TypeDetails {
    fn new(
        rs_name: impl Into<String>,
        cpp_name: impl Into<String>,
        behavior: Behavior,
        extra_non_canonical_name: Option<String>,
    ) -> Self {
        TypeDetails {
            rs_name: rs_name.into(),
            cpp_name: cpp_name.into(),
            behavior,
            extra_non_canonical_name,
        }
    }

    /// Whether and how to include this in the prelude given to bindgen.
    fn get_prelude_entry(&self) -> Option<String> {
        match self.behavior {
            Behavior::RustString
            | Behavior::RustStr
            | Behavior::CxxString
            | Behavior::CxxContainerByValueSafe
            | Behavior::CxxContainerNotByValueSafe => {
                let tn = QualifiedName::new_from_user_input(&self.rs_name);
                let cxx_name = tn.get_final_ident();
                let (templating, payload) = match self.behavior {
                    Behavior::CxxContainerByValueSafe | Behavior::CxxContainerNotByValueSafe => {
                        ("template<typename T> ", "T* ptr")
                    }
                    _ => ("", "char* ptr"),
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
            _ => None,
        }
    }

    fn to_type_path(&self) -> TypePath {
        let segs = self.rs_name.split("::").map(make_ident);
        parse_quote! {
            #(#segs)::*
        }
    }

    fn to_typename(&self) -> QualifiedName {
        QualifiedName::new_from_user_input(&self.rs_name)
    }
}

/// Database of known types.
#[derive(Default)]
pub(crate) struct TypeDatabase {
    by_rs_name: HashMap<QualifiedName, TypeDetails>,
    canonical_names: HashMap<QualifiedName, QualifiedName>,
}

/// Returns a database of known types.
pub(crate) fn known_types() -> &'static TypeDatabase {
    static KNOWN_TYPES: OnceCell<TypeDatabase> = OnceCell::new();
    KNOWN_TYPES.get_or_init(create_type_database)
}

impl TypeDatabase {
    fn get(&self, ty: &QualifiedName) -> Option<&TypeDetails> {
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
    pub(crate) fn get_pod_safe_types(&self) -> impl Iterator<Item = (&QualifiedName, bool)> {
        self.by_rs_name.iter().map(|(tn, td)| {
            (
                tn,
                match td.behavior {
                    Behavior::CxxContainerByValueSafe
                    | Behavior::RustStr
                    | Behavior::RustString
                    | Behavior::RustByValue
                    | Behavior::CByValue
                    | Behavior::CVariableLengthByValue => true,
                    Behavior::CxxString
                    | Behavior::CxxContainerNotByValueSafe
                    | Behavior::CVoid => false,
                },
            )
        })
    }

    /// Whether this TypePath should be treated as a value in C++
    /// but a reference in Rust. This only applies to rust::Str
    /// (C++ name) which is &str in Rust.
    pub(crate) fn should_dereference_in_cpp(&self, typ: &TypePath) -> bool {
        let tn = QualifiedName::from_type_path(typ);
        self.get(&tn)
            .map(|td| matches!(td.behavior, Behavior::RustStr))
            .unwrap_or(false)
    }

    /// Here we substitute any names which we know are Special from
    /// our type database, e.g. std::unique_ptr -> UniquePtr.
    /// We strip off and ignore
    /// any PathArguments within this TypePath - callers should
    /// put them back again if needs be.
    pub(crate) fn known_type_substitute_path(&self, typ: &TypePath) -> Option<TypePath> {
        let tn = QualifiedName::from_type_path(typ);
        self.get(&tn).map(|td| td.to_type_path())
    }

    pub(crate) fn special_cpp_name(&self, rs: &QualifiedName) -> Option<String> {
        self.get(rs).map(|x| x.cpp_name.to_string())
    }

    pub(crate) fn is_known_type(&self, ty: &QualifiedName) -> bool {
        self.get(ty).is_some()
    }

    pub(crate) fn known_type_type_path(&self, ty: &QualifiedName) -> Option<TypePath> {
        self.get(ty).map(|td| td.to_type_path())
    }

    /// Whether this is one of the ctypes (mostly variable length integers)
    /// which we need to wrap.
    pub(crate) fn is_ctype(&self, ty: &QualifiedName) -> bool {
        self.get(ty)
            .map(|td| {
                matches!(
                    td.behavior,
                    Behavior::CVariableLengthByValue | Behavior::CVoid
                )
            })
            .unwrap_or(false)
    }

    /// Whether this is a generic type acceptable to cxx. Otherwise,
    /// if we encounter a generic, we'll replace it with a synthesized concrete
    /// type.
    pub(crate) fn is_cxx_acceptable_generic(&self, ty: &QualifiedName) -> bool {
        self.get(ty)
            .map(|x| {
                matches!(
                    x.behavior,
                    Behavior::CxxContainerByValueSafe | Behavior::CxxContainerNotByValueSafe
                )
            })
            .unwrap_or(false)
    }

    pub(crate) fn convertible_from_strs(&self, ty: &QualifiedName) -> bool {
        self.get(ty)
            .map(|x| matches!(x.behavior, Behavior::CxxString))
            .unwrap_or(false)
    }

    fn insert(&mut self, td: TypeDetails) {
        let rs_name = td.to_typename();
        if let Some(extra_non_canonical_name) = &td.extra_non_canonical_name {
            self.canonical_names.insert(
                QualifiedName::new_from_user_input(extra_non_canonical_name),
                rs_name.clone(),
            );
        }
        self.canonical_names.insert(
            QualifiedName::new_from_user_input(&td.cpp_name),
            rs_name.clone(),
        );
        self.by_rs_name.insert(rs_name, td);
    }
}

fn create_type_database() -> TypeDatabase {
    let mut db = TypeDatabase::default();
    db.insert(TypeDetails::new(
        "cxx::UniquePtr",
        "std::unique_ptr",
        Behavior::CxxContainerByValueSafe,
        None,
    ));
    db.insert(TypeDetails::new(
        "cxx::CxxVector",
        "std::vector",
        Behavior::CxxContainerNotByValueSafe,
        None,
    ));
    db.insert(TypeDetails::new(
        "cxx::SharedPtr",
        "std::shared_ptr",
        Behavior::CxxContainerByValueSafe,
        None,
    ));
    db.insert(TypeDetails::new(
        "cxx::CxxString",
        "std::string",
        Behavior::CxxString,
        None,
    ));
    db.insert(TypeDetails::new(
        "str",
        "rust::Str",
        Behavior::RustStr,
        None,
    ));
    db.insert(TypeDetails::new(
        "String",
        "rust::String",
        Behavior::RustString,
        None,
    ));
    db.insert(TypeDetails::new(
        "i8",
        "int8_t",
        Behavior::CByValue,
        Some("std::os::raw::c_schar".into()),
    ));
    db.insert(TypeDetails::new(
        "u8",
        "uint8_t",
        Behavior::CByValue,
        Some("std::os::raw::c_uchar".into()),
    ));
    for (cpp_type, rust_type) in (4..7)
        .map(|x| 2i32.pow(x))
        .map(|x| {
            vec![
                (format!("uint{}_t", x), format!("u{}", x)),
                (format!("int{}_t", x), format!("i{}", x)),
            ]
        })
        .flatten()
    {
        db.insert(TypeDetails::new(
            rust_type,
            cpp_type,
            Behavior::CByValue,
            None,
        ));
    }
    db.insert(TypeDetails::new("bool", "bool", Behavior::CByValue, None));

    db.insert(TypeDetails::new(
        "std::pin::Pin",
        "Pin",
        Behavior::RustByValue, // because this is actually Pin<&something>
        None,
    ));

    let mut insert_ctype = |cname: &str| {
        let concatenated_name = cname.replace(" ", "");
        db.insert(TypeDetails::new(
            format!("autocxx::c_{}", concatenated_name),
            cname,
            Behavior::CVariableLengthByValue,
            Some(format!("std::os::raw::c_{}", concatenated_name)),
        ));
        db.insert(TypeDetails::new(
            format!("autocxx::c_u{}", concatenated_name),
            format!("unsigned {}", cname),
            Behavior::CVariableLengthByValue,
            Some(format!("std::os::raw::c_u{}", concatenated_name)),
        ));
    };

    insert_ctype("long");
    insert_ctype("int");
    insert_ctype("short");
    insert_ctype("long long");

    db.insert(TypeDetails::new("f32", "float", Behavior::CByValue, None));
    db.insert(TypeDetails::new("f64", "double", Behavior::CByValue, None));
    db.insert(TypeDetails::new(
        "std::os::raw::c_char",
        "char",
        Behavior::CByValue,
        None,
    ));
    db.insert(TypeDetails::new(
        "autocxx::c_void",
        "void",
        Behavior::CVoid,
        Some("std::os::raw::c_void".into()),
    ));
    db
}

/// This is worked out basically using trial and error.
/// Excluding std* and rust* is obvious, but the other items...
/// in theory bindgen ought to be smart enough to work out that
/// they're not used and therefore not generate code for them.
/// But it doesn't unless we blocklist them. This is obviously
/// a bit sensitive to the particular STL in use so one day
/// it would be good to dig into bindgen's behavior here - TODO.
///
/// We import types from `std::` as opaque types instead of
/// blocklisting them entirely because
/// *  It's what bindgen recommends:
///    https://rust-lang.github.io/rust-bindgen/cpp.html
/// *  We still pass APIs that use these types to cxx, and if we
///    blocklisted these types entirely, cxx would complain that
///    it doesn't know about them.
const BINDGEN_BLOCKLIST: &[&str] = &["__gnu.*", ".*mbstate_t.*", "rust::.*"];
const BINDGEN_FUNCTION_BLOCKLIST: &[&str] = &["std::.*"];
const BINDGEN_OPAQUE_TYPES: &[&str] = &["std::.*"];

/// Get a list of regexes that match items for which bindgen should
/// _not_ generate code.
pub(crate) fn get_bindgen_blocklist() -> Vec<String> {
    BINDGEN_BLOCKLIST.iter().map(|s| s.to_string()).collect()
}

/// Get a list of regexes that match functions for which bindgen should
/// _not_ generate code.
pub(crate) fn get_bindgen_function_blocklist() -> Vec<String> {
    BINDGEN_FUNCTION_BLOCKLIST
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Get a list of regexes that match type which bindgen should import
/// as opaque types.
pub(crate) fn get_bindgen_opaque_types() -> Vec<String> {
    BINDGEN_OPAQUE_TYPES.iter().map(|s| s.to_string()).collect()
}

/// If a given type lacks a copy constructor, we should always use
/// std::move in wrapper functions.
pub(crate) fn type_lacks_copy_constructor(ty: &Type) -> bool {
    // In future we may wish to look this up in KNOWN_TYPES.
    match ty {
        Type::Path(typ) => {
            let tn = QualifiedName::from_type_path(typ);
            tn.to_cpp_name().starts_with("std::unique_ptr")
        }
        _ => false,
    }
}

pub(crate) fn confirm_inner_type_is_acceptable_generic_payload(
    path_args: &PathArguments,
    desc: &QualifiedName,
) -> Result<(), ConvertError> {
    // For now, all supported generics accept the same payloads. This
    // may change in future in which case we'll need to accept more arguments here.
    match path_args {
        PathArguments::None => Ok(()),
        PathArguments::Parenthesized(_) => Err(ConvertError::TemplatedTypeContainingNonPathArg(
            desc.clone(),
        )),
        PathArguments::AngleBracketed(ab) => {
            for inner in &ab.args {
                match inner {
                    GenericArgument::Type(Type::Path(typ)) => {
                        if let Some(more_generics) = typ.path.segments.last() {
                            confirm_inner_type_is_acceptable_generic_payload(
                                &more_generics.arguments,
                                desc,
                            )?;
                        }
                    }
                    _ => {
                        return Err(ConvertError::TemplatedTypeContainingNonPathArg(
                            desc.clone(),
                        ))
                    }
                }
            }
            Ok(())
        }
    }
}

pub(crate) fn ensure_pointee_is_valid(ptr: &TypePtr) -> Result<(), ConvertError> {
    match *ptr.elem {
        Type::Path(..) => Ok(()),
        _ => Err(ConvertError::InvalidPointee),
    }
}
