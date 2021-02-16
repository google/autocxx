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

use autocxx_parser::TypeConfig;
use syn::{Item, Type, TypePath};

use crate::{
    conversion::ConvertError, known_types::KNOWN_TYPES, types::Namespace, types::TypeName,
};

use super::ByValueChecker;

struct ByValueScanner<'a> {
    byvalue_checker: ByValueChecker,
    type_config: &'a TypeConfig,
}

pub(crate) fn identify_byvalue_safe_types(
    items: &[Item],
    type_database: &TypeConfig,
) -> Result<ByValueChecker, ConvertError> {
    let mut bvs = ByValueScanner {
        byvalue_checker: ByValueChecker::new(),
        type_config: type_database,
    };
    bvs.find_nested_pod_types(items)?;
    Ok(bvs.byvalue_checker)
}

impl<'a> ByValueScanner<'a> {
    fn find_nested_pod_types_in_mod(
        &mut self,
        items: &[Item],
        ns: &Namespace,
    ) -> Result<(), ConvertError> {
        for item in items {
            match item {
                Item::Struct(s) => self.byvalue_checker.ingest_struct(s, ns),
                Item::Enum(e) => self
                    .byvalue_checker
                    .ingest_pod_type(TypeName::new(&ns, &e.ident.to_string())),
                Item::Type(ity) => {
                    let name = TypeName::new(ns, &ity.ident.to_string());
                    let typedef_type = Self::analyze_typedef_target(ity.ty.as_ref());
                    match &typedef_type {
                        Some(typ) => self
                            .byvalue_checker
                            .ingest_simple_typedef(name, TypeName::from_type_path(&typ)),
                        None => self.byvalue_checker.ingest_nonpod_type(name),
                    }
                }
                Item::Mod(itm) => {
                    if let Some((_, nested_items)) = &itm.content {
                        let new_ns = ns.push(itm.ident.to_string());
                        self.find_nested_pod_types_in_mod(nested_items, &new_ns)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn analyze_typedef_target(ty: &Type) -> Option<TypePath> {
        match ty {
            Type::Path(typ) => KNOWN_TYPES.known_type_substitute_path(&typ),
            _ => None,
        }
    }

    fn find_nested_pod_types(&mut self, items: &[Item]) -> Result<(), ConvertError> {
        self.find_nested_pod_types_in_mod(items, &Namespace::new())?;
        let pod_requests = self
            .type_config
            .get_pod_requests()
            .iter()
            .map(|ty| TypeName::new_from_user_input(ty))
            .collect();
        self.byvalue_checker
            .satisfy_requests(pod_requests)
            .map_err(ConvertError::UnsafePodType)
    }
}
