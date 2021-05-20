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

use autocxx_parser::IncludeCppConfig;
use byvalue_checker::ByValueChecker;
use syn::{ItemStruct, Type};

use crate::{
    conversion::{
        analysis::type_converter::{add_analysis, TypeConversionContext, TypeConverter},
        api::{AnalysisPhase, Api, ApiDetail, TypeKind, TypedefKind, UnanalyzedApi},
        codegen_rs::make_non_pod,
        error_reporter::convert_item_apis,
        ConvertError,
    },
    types::{Namespace, QualifiedName},
};

use super::tdef::TypedefAnalysis;

pub(crate) struct PodStructAnalysisBody {
    pub(crate) kind: TypeKind,
    pub(crate) bases: HashSet<QualifiedName>,
}

pub(crate) struct PodAnalysis;

impl AnalysisPhase for PodAnalysis {
    type TypedefAnalysis = TypedefKind;
    type StructAnalysis = PodStructAnalysisBody;
    type EnumAnalysis = TypeKind;
    type FunAnalysis = ();
}

/// In our set of APIs, work out which ones are safe to represent
/// by value in Rust (e.g. they don't have a destructor) and record
/// as such. Return a set of APIs annotated with extra metadata,
/// and an object which can be used to query the POD status of any
/// type whether or not it's one of the [Api]s.
pub(crate) fn analyze_pod_apis(
    apis: Vec<Api<TypedefAnalysis>>,
    config: &IncludeCppConfig,
) -> Result<Vec<Api<PodAnalysis>>, ConvertError> {
    // This next line will return an error if any of the 'generate_pod'
    // directives from the user can't be met because, for instance,
    // a type contains a std::string or some other type which can't be
    // held safely by value in Rust.
    let byvalue_checker = ByValueChecker::new_from_apis(&apis, config)?;
    let mut extra_apis = Vec::new();
    let mut type_converter = TypeConverter::new(config, &apis);
    let mut results = Vec::new();
    convert_item_apis(apis, &mut results, |api| {
        analyze_pod_api(api, &byvalue_checker, &mut type_converter, &mut extra_apis).map(Some)
    });
    // Conceivably, the process of POD-analysing the first set of APIs could result
    // in us creating new APIs to concretize generic types.
    let mut more_extra_apis = Vec::new();
    convert_item_apis(extra_apis, &mut results, |api| {
        analyze_pod_api(
            add_analysis(api),
            &byvalue_checker,
            &mut type_converter,
            &mut more_extra_apis,
        )
        .map(Some)
    });
    assert!(more_extra_apis.is_empty());
    Ok(results)
}

fn analyze_pod_api(
    api: Api<TypedefAnalysis>,
    byvalue_checker: &ByValueChecker,
    type_converter: &mut TypeConverter,
    extra_apis: &mut Vec<UnanalyzedApi>,
) -> Result<Api<PodAnalysis>, ConvertError> {
    let ty_id = api.name();
    let mut new_deps = api.deps;
    let api_detail = match api.detail {
        // No changes to any of these...
        ApiDetail::ConcreteType {
            rs_definition,
            cpp_definition,
        } => ApiDetail::ConcreteType {
            rs_definition,
            cpp_definition,
        },
        ApiDetail::ForwardDeclaration => ApiDetail::ForwardDeclaration,
        ApiDetail::StringConstructor => ApiDetail::StringConstructor,
        ApiDetail::Function { fun, analysis } => ApiDetail::Function { fun, analysis },
        ApiDetail::Const { const_item } => ApiDetail::Const { const_item },
        ApiDetail::Typedef { item, analysis } => ApiDetail::Typedef { item, analysis },
        ApiDetail::CType { typename } => ApiDetail::CType { typename },
        // Just changes to these two...
        ApiDetail::Enum {
            mut item,
            analysis: _,
        } => {
            super::remove_bindgen_attrs(&mut item.attrs)?;
            let analysis = if byvalue_checker.is_pod(&ty_id) {
                TypeKind::Pod
            } else {
                TypeKind::NonPod
            };
            ApiDetail::Enum { item, analysis }
        }
        ApiDetail::Struct {
            mut item,
            analysis: _,
        } => {
            super::remove_bindgen_attrs(&mut item.attrs)?;
            let bases = get_bases(&item);
            let type_kind = if byvalue_checker.is_pod(&ty_id) {
                // It's POD so let's mark dependencies on things in its field
                get_struct_field_types(
                    type_converter,
                    &api.name.get_namespace(),
                    &item,
                    &mut new_deps,
                    extra_apis,
                )?;
                TypeKind::Pod
            } else {
                // It's non-POD. So also, make the fields opaque...
                make_non_pod(&mut item);
                // ... and say we don't depend on other types.
                new_deps.clear();
                TypeKind::NonPod
            };
            ApiDetail::Struct {
                item,
                analysis: PodStructAnalysisBody {
                    kind: type_kind,
                    bases,
                },
            }
        }
        ApiDetail::IgnoredItem { err, ctx } => ApiDetail::IgnoredItem { err, ctx },
    };
    Ok(Api {
        name: api.name,
        original_name: api.original_name,
        deps: new_deps,
        detail: api_detail,
    })
}

fn get_struct_field_types(
    type_converter: &mut TypeConverter,
    ns: &Namespace,
    s: &ItemStruct,
    deps: &mut HashSet<QualifiedName>,
    extra_apis: &mut Vec<UnanalyzedApi>,
) -> Result<(), ConvertError> {
    for f in &s.fields {
        let annotated =
            type_converter.convert_type(f.ty.clone(), ns, &TypeConversionContext::CxxInnerType)?;
        extra_apis.extend(annotated.extra_apis);
        deps.extend(annotated.types_encountered);
    }
    Ok(())
}

fn get_bases(item: &ItemStruct) -> HashSet<QualifiedName> {
    item.fields
        .iter()
        .filter_map(|f| match &f.ty {
            Type::Path(typ) => f
                .ident
                .as_ref()
                .filter(|id| id.to_string().starts_with("_base"))
                .map(|_| QualifiedName::from_type_path(&typ)),
            _ => None,
        })
        .collect()
}
