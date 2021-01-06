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

use syn::Item;

use crate::{
    byvalue_checker::ByValueChecker, conversion::ConvertError, type_database::TypeDatabase,
    typedef_analyzer::analyze_typedef_target, typedef_analyzer::TypedefTarget, types::Namespace,
    types::TypeName,
};

struct ByValueScanner<'a> {
    byvalue_checker: ByValueChecker,
    type_database: &'a TypeDatabase,
}

pub(crate) fn identify_byvalue_safe_types(
    items: &[Item],
    type_database: &TypeDatabase,
) -> Result<ByValueChecker, ConvertError> {
    let mut bvs = ByValueScanner {
        byvalue_checker: ByValueChecker::new(),
        type_database,
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
                    let typedef_type = analyze_typedef_target(ity.ty.as_ref());
                    let name = TypeName::new(ns, &ity.ident.to_string());
                    match typedef_type {
                        TypedefTarget::NoArguments(tn) => {
                            self.byvalue_checker.ingest_simple_typedef(name, tn)
                        }
                        TypedefTarget::HasArguments | TypedefTarget::SomethingComplex => {
                            self.byvalue_checker.ingest_nonpod_type(name)
                        }
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

    fn find_nested_pod_types(&mut self, items: &[Item]) -> Result<(), ConvertError> {
        self.find_nested_pod_types_in_mod(items, &Namespace::new())?;
        self.byvalue_checker
            .satisfy_requests(self.type_database.get_pod_requests().to_vec())
            .map_err(ConvertError::UnsafePODType)
    }
}
