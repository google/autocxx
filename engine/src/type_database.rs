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
use syn::Type;

use crate::types::TypeName;
use std::collections::HashSet;

/// Central registry of all information known about types.
/// At present this is very minimal; in future we should roll
/// known_types.rs into this and possibly other things as well.
#[derive(Default)]
pub(crate) struct TypeDatabase {
    pod_requests: HashSet<TypeName>,
    allowlist: HashSet<String>, // not TypeName as it may be funcs not types.
    blocklist: HashSet<String>, // not TypeName as it may be funcs not types.
}

impl TypeDatabase {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn note_pod_request(&mut self, tn: TypeName) {
        self.pod_requests.insert(tn);
    }

    pub(crate) fn add_to_allowlist(&mut self, item: String) {
        self.allowlist.insert(item);
    }

    pub(crate) fn add_to_blocklist(&mut self, item: String) {
        self.blocklist.insert(item);
    }

    pub(crate) fn get_pod_requests(&self) -> &HashSet<TypeName> {
        &self.pod_requests
    }

    pub(crate) fn allowlist(&self) -> impl Iterator<Item = &String> {
        self.allowlist.iter()
    }

    pub(crate) fn allowlist_is_empty(&self) -> bool {
        self.allowlist.is_empty()
    }

    /// Whether this type is on the allowlist specified by the user.
    ///
    /// A note on the allowlist handling in general. It's used in two places:
    /// 1) As directives to bindgen
    /// 2) After bindgen has generated code, to filter the APIs which
    ///    we pass to cxx.
    /// This second pass may seem redundant. But sometimes bindgen generates
    /// unnecessary stuff.
    pub(crate) fn is_on_allowlist(&self, tn: &TypeName) -> bool {
        self.allowlist.contains(&tn.to_cpp_name())
    }

    pub(crate) fn is_on_blocklist(&self, tn: &TypeName) -> bool {
        self.blocklist.contains(&tn.to_cpp_name())
    }

    pub(crate) fn type_to_cpp(&self, ty: &Type) -> String {
        match ty {
            Type::Path(typ) => {
                // If this is a std::unique_ptr we do need to pass
                // its argument through.
                let root = TypeName::from_type_path(typ);
                let root = root.to_cpp_name();
                if root == "Pin" {
                    // Strip all Pins from type names when describing them in C++.
                    let inner_type = &typ.path.segments.last().unwrap().arguments;
                    if let syn::PathArguments::AngleBracketed(ab) = inner_type {
                        let inner_type = ab.args.iter().next().unwrap();
                        if let syn::GenericArgument::Type(gat) = inner_type {
                            return self.type_to_cpp(gat);
                        }
                    }
                    panic!("Pin<...> didn't contain the inner types we expected");
                }
                let suffix = match &typ.path.segments.last().unwrap().arguments {
                    syn::PathArguments::AngleBracketed(ab) => Some(
                        ab.args
                            .iter()
                            .map(|x| match x {
                                syn::GenericArgument::Type(gat) => self.type_to_cpp(gat),
                                _ => "".to_string(),
                            })
                            .join(", "),
                    ),
                    syn::PathArguments::None | syn::PathArguments::Parenthesized(_) => None,
                };
                match suffix {
                    None => root,
                    Some(suffix) => format!("{}<{}>", root, suffix),
                }
            }
            Type::Reference(typr) => {
                let const_bit = match typr.mutability {
                    None => "const ",
                    Some(_) => "",
                };
                format!("{}{}&", const_bit, self.type_to_cpp(typr.elem.as_ref()))
            }
            Type::Array(_)
            | Type::BareFn(_)
            | Type::Group(_)
            | Type::ImplTrait(_)
            | Type::Infer(_)
            | Type::Macro(_)
            | Type::Never(_)
            | Type::Paren(_)
            | Type::Ptr(_)
            | Type::Slice(_)
            | Type::TraitObject(_)
            | Type::Tuple(_)
            | Type::Verbatim(_) => panic!("Unsupported type"),
            _ => panic!("Unknown type"),
        }
    }
}
