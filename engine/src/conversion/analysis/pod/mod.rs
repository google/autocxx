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

mod byvalue_checker;
mod byvalue_scanner;

use std::collections::HashSet;

pub(crate) use byvalue_checker::ByValueChecker;
pub(crate) use byvalue_scanner::identify_byvalue_safe_types;
use syn::{Item, ItemStruct};

use crate::{conversion::{ConvertError, api::{Api, ApiAnalysis, ApiDetail, TypeKind, UnanalyzedApi}, codegen_rs::make_non_pod, parse::type_converter::TypeConverter}, types::{Namespace, TypeName}};

pub(crate) struct PodAnalysis;

impl ApiAnalysis for PodAnalysis {
    type TypeAnalysis = TypeKind;
}

pub(crate) fn analyze_pod_apis(
    apis: Vec<UnanalyzedApi>,
    byvalue_checker: &ByValueChecker,
    type_converter: &mut TypeConverter
) -> Result<Vec<Api<PodAnalysis>>,ConvertError> {
    apis.into_iter()
        .map(|api| analyze_pod_api(api, &byvalue_checker, type_converter))
        .collect()
}

fn analyze_pod_api(api: UnanalyzedApi, byvalue_checker: &ByValueChecker, type_converter: &mut TypeConverter) -> Result<Api<PodAnalysis>, ConvertError> {
    let mut new_deps = api.deps;
    let api_detail = match api.detail {
        // No changes to any of these...
        ApiDetail::ConcreteType(details) => ApiDetail::ConcreteType(details),
        ApiDetail::StringConstructor => ApiDetail::StringConstructor,
        ApiDetail::ImplEntry { impl_entry } => ApiDetail::ImplEntry { impl_entry },
        ApiDetail::Function { extern_c_mod_item } => ApiDetail::Function { extern_c_mod_item },
        ApiDetail::Const { const_item } => ApiDetail::Const { const_item },
        ApiDetail::Typedef { type_item } => ApiDetail::Typedef { type_item },
        ApiDetail::CType { id } => ApiDetail::CType { id },
        // Just changes to this one...
        ApiDetail::Type {
            ty_details,
            for_extern_c_ts,
            is_forward_declaration,
            mut bindgen_mod_item,
            analysis: _,
        } => {
            let ty_id = TypeName::new(&api.ns, &api.id.to_string());
            let type_kind = if is_forward_declaration {
                TypeKind::ForwardDeclaration
            } else if byvalue_checker.is_pod(&ty_id) {
                // It's POD so let's mark dependencies on things in its field
                if let Some(Item::Struct(ref s)) = bindgen_mod_item {
                    new_deps.extend(get_struct_field_types(type_converter, &api.ns, &s)?);
                } // otherwise might be an enum, etc.
                TypeKind::POD
            } else {
                // It's non-POD. So also, make the fields opaque...
                if let Some(Item::Struct(ref mut s)) = bindgen_mod_item {
                    make_non_pod(s);
                } // otherwise might be an enum, etc.
                  // ... and say we don't depend on other types.
                new_deps.clear();
                TypeKind::NonPOD
            };
            ApiDetail::Type {
                ty_details,
                for_extern_c_ts,
                is_forward_declaration,
                bindgen_mod_item,
                analysis: type_kind,
            }
        }
    };
    Ok(Api {
        ns: api.ns,
        id: api.id,
        use_stmt: api.use_stmt,
        deps: new_deps,
        id_for_allowlist: api.id_for_allowlist,
        additional_cpp: api.additional_cpp,
        detail: api_detail,
    })
}

fn get_struct_field_types(
    type_converter: &mut TypeConverter,
    ns: &Namespace,
    s: &ItemStruct,
) -> Result<HashSet<TypeName>, ConvertError> {
    let mut results = HashSet::new();
    for f in &s.fields {
        let annotated = type_converter.convert_type(f.ty.clone(), ns, false)?;
        // self.results.apis.extend(annotated.extra_apis); // TODO
        results.extend(annotated.types_encountered);
    }
    Ok(results)
}