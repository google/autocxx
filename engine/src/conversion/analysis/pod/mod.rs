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

use std::collections::HashSet;

use autocxx_parser::TypeConfig;
use byvalue_checker::ByValueChecker;
use syn::{Item, ItemStruct};

use crate::{
    conversion::{
        api::{Api, ApiAnalysis, ApiDetail, TypeKind, UnanalyzedApi},
        codegen_rs::make_non_pod,
        parse::type_converter::TypeConverter,
        ConvertError,
    },
    types::{Namespace, TypeName},
};

pub(crate) struct PodAnalysis;

impl ApiAnalysis for PodAnalysis {
    type TypeAnalysis = TypeKind;
    type FunAnalysis = ();
}

/// In our set of APIs, work out which ones are safe to represent
/// by value in Rust (e.g. they don't have a destructor) and record
/// as such. Return a set of APIs annotated with extra metadata,
/// and an object which can be used to query the POD status of any
/// type whether or not it's one of the [Api]s.
pub(crate) fn analyze_pod_apis(
    apis: Vec<UnanalyzedApi>,
    type_config: &TypeConfig,
    type_converter: &mut TypeConverter,
) -> Result<Vec<Api<PodAnalysis>>, ConvertError> {
    // This next line will return an error if any of the 'generate_pod'
    // directives from the user can't be met because, for instance,
    // a type contains a std::string or some other type which can't be
    // held safely by value in Rust.
    let byvalue_checker = ByValueChecker::new_from_apis(&apis, type_config)?;
    let mut extra_apis = Vec::new();
    let mut results: Vec<_> = apis
        .into_iter()
        .map(|api| analyze_pod_api(api, &byvalue_checker, type_converter, &mut extra_apis))
        .collect::<Result<Vec<_>, ConvertError>>()?;
    // Conceivably, the process of POD-analysing the first set of APIs could result
    // in us creating new APIs to concretize generic types.
    let mut more_extra_apis = Vec::new();
    let mut more_results = extra_apis
        .into_iter()
        .map(|api| analyze_pod_api(api, &byvalue_checker, type_converter, &mut more_extra_apis))
        .collect::<Result<Vec<_>, ConvertError>>()?;
    assert!(more_extra_apis.is_empty());
    results.append(&mut more_results);
    Ok(results)
}

fn analyze_pod_api(
    api: UnanalyzedApi,
    byvalue_checker: &ByValueChecker,
    type_converter: &mut TypeConverter,
    extra_apis: &mut Vec<UnanalyzedApi>,
) -> Result<Api<PodAnalysis>, ConvertError> {
    let ty_id = api.typename();
    let mut new_deps = api.deps;
    let api_detail = match api.detail {
        // No changes to any of these...
        ApiDetail::ConcreteType {
            ty_details,
            additional_cpp,
        } => ApiDetail::ConcreteType {
            ty_details,
            additional_cpp,
        },
        ApiDetail::StringConstructor => ApiDetail::StringConstructor,
        ApiDetail::Function { fun, analysis } => ApiDetail::Function { fun, analysis },
        ApiDetail::Const { const_item } => ApiDetail::Const { const_item },
        ApiDetail::Typedef { payload } => ApiDetail::Typedef { payload },
        ApiDetail::CType { typename } => ApiDetail::CType { typename },
        // Just changes to this one...
        ApiDetail::Type {
            ty_details,
            is_forward_declaration,
            mut bindgen_mod_item,
            analysis: _,
        } => {
            let type_kind = if is_forward_declaration {
                TypeKind::ForwardDeclaration
            } else if byvalue_checker.is_pod(&ty_id) {
                // It's POD so let's mark dependencies on things in its field
                if let Some(Item::Struct(ref s)) = bindgen_mod_item {
                    get_struct_field_types(type_converter, &api.ns, &s, &mut new_deps, extra_apis)?;
                } // otherwise might be an enum, etc.
                TypeKind::Pod
            } else {
                // It's non-POD. So also, make the fields opaque...
                if let Some(Item::Struct(ref mut s)) = bindgen_mod_item {
                    make_non_pod(s);
                } // otherwise might be an enum, etc.
                  // ... and say we don't depend on other types.
                new_deps.clear();
                TypeKind::NonPod
            };
            ApiDetail::Type {
                ty_details,
                is_forward_declaration,
                bindgen_mod_item,
                analysis: type_kind,
            }
        }
        ApiDetail::OpaqueTypedef => ApiDetail::OpaqueTypedef,
        ApiDetail::IgnoredItem { err } => ApiDetail::IgnoredItem { err },
    };
    Ok(Api {
        ns: api.ns,
        id: api.id,
        deps: new_deps,
        detail: api_detail,
    })
}

fn get_struct_field_types(
    type_converter: &mut TypeConverter,
    ns: &Namespace,
    s: &ItemStruct,
    deps: &mut HashSet<TypeName>,
    extra_apis: &mut Vec<UnanalyzedApi>,
) -> Result<(), ConvertError> {
    for f in &s.fields {
        let annotated = type_converter.convert_type(f.ty.clone(), ns, false)?;
        extra_apis.extend(annotated.extra_apis);
        deps.extend(annotated.types_encountered);
    }
    Ok(())
}
