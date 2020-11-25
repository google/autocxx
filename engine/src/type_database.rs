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
use std::collections::HashMap;

/// Central registry of all information known about types.
/// At present this is very minimal; in future we should roll
/// known_types.rs into this and possibly other things as well.
#[derive(Default)]
pub(crate) struct TypeDatabase {
    nested_types: HashMap<TypeName, TypeName>,
    pod_requests: Vec<TypeName>,
}

impl TypeDatabase {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn note_nested_type(&mut self, original: TypeName, replacement: TypeName) {
        self.nested_types.insert(original, replacement);
    }

    pub(crate) fn note_pod_request(&mut self, tn: TypeName) {
        self.pod_requests.push(tn);
    }

    pub(crate) fn get_effective_type(&self, original: &TypeName) -> Option<&TypeName> {
        self.nested_types.get(original)
    }

    pub(crate) fn get_pod_requests(&self) -> Vec<TypeName> {
        self.pod_requests.clone()
    }

    pub(crate) fn type_to_cpp(&self, ty: &Type) -> String {
        match ty {
            Type::Path(typ) => {
                // If this is a std::unique_ptr we do need to pass
                // its argument through.
                let root = TypeName::from_type_path(typ);
                let root = self.get_effective_type(&root).unwrap_or(&root);
                let root = root.to_cpp_name();
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
