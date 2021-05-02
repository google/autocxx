// Copyright 2021 Google LLC
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

use std::collections::HashSet;

use autocxx_parser::TypeConfig;
use syn::ItemType;

use crate::{
    conversion::{
        analysis::type_converter::{add_analysis, Annotated, TypeConversionContext, TypeConverter},
        api::{Api, ApiAnalysis, ApiDetail, TypedefKind, UnanalyzedApi},
        convert_error::{ConvertErrorWithContext, ErrorContext},
        error_reporter::report_any_error,
        ConvertError,
    },
    types::QualifiedName,
};

/// Analysis phase where typedef analysis has been performed but no other
/// analyses just yet.
pub(crate) struct TypedefAnalysis;

impl ApiAnalysis for TypedefAnalysis {
    type TypedefAnalysis = TypedefKind;
    type TypeAnalysis = ();
    type FunAnalysis = ();
}

#[allow(clippy::needless_collect)] // we need the extra collect because the closure borrows extra_apis
pub(crate) fn convert_typedef_targets(
    type_config: &TypeConfig,
    apis: Vec<UnanalyzedApi>,
) -> Vec<Api<TypedefAnalysis>> {
    let mut type_converter = TypeConverter::new(type_config, &apis);
    let mut extra_apis = Vec::new();
    let mut problem_apis = Vec::new();
    let new_apis = apis
        .into_iter()
        .filter_map(|api| {
            let name = api.name();
            let original_name = api.original_name;
            let ns = name.get_namespace();
            let mut newdeps = api.deps;
            let detail = match api.detail {
                ApiDetail::ForwardDeclaration => Some(ApiDetail::ForwardDeclaration),
                ApiDetail::ConcreteType {
                    rs_definition,
                    cpp_definition,
                } => Some(ApiDetail::ConcreteType {
                    rs_definition,
                    cpp_definition,
                }),
                ApiDetail::StringConstructor => Some(ApiDetail::StringConstructor),
                ApiDetail::Function { fun, analysis } => {
                    Some(ApiDetail::Function { fun, analysis })
                }
                ApiDetail::Const { const_item } => Some(ApiDetail::Const { const_item }),
                ApiDetail::Typedef {
                    item: TypedefKind::Type(ity),
                    analysis: _,
                } => report_any_error(ns, &mut problem_apis, || {
                    get_replacement_typedef(
                        &name,
                        ity,
                        &mut type_converter,
                        &mut extra_apis,
                        &mut newdeps,
                    )
                }),
                ApiDetail::Typedef { item, analysis: _ } => Some(ApiDetail::Typedef {
                    item: item.clone(),
                    analysis: item,
                }),
                ApiDetail::Type {
                    bindgen_mod_item,
                    analysis,
                } => Some(ApiDetail::Type {
                    bindgen_mod_item,
                    analysis,
                }),
                ApiDetail::CType { typename } => Some(ApiDetail::CType { typename }),
                ApiDetail::IgnoredItem { err, ctx } => Some(ApiDetail::IgnoredItem { err, ctx }),
            };
            detail.map(|detail| Api {
                detail,
                name,
                original_name,
                deps: newdeps,
            })
        })
        .collect::<Vec<_>>();
    new_apis
        .into_iter()
        .chain(extra_apis.into_iter().chain(problem_apis).map(add_analysis))
        .collect()
}

fn get_replacement_typedef(
    name: &QualifiedName,
    ity: ItemType,
    type_converter: &mut TypeConverter,
    extra_apis: &mut Vec<UnanalyzedApi>,
    deps: &mut HashSet<QualifiedName>,
) -> Result<ApiDetail<TypedefAnalysis>, ConvertErrorWithContext> {
    let mut converted_type = ity.clone();
    let type_conversion_results = type_converter.convert_type(
        (*ity.ty).clone(),
        name.get_namespace(),
        &TypeConversionContext::CxxInnerType,
    );
    match type_conversion_results {
        Err(err) => Err(ConvertErrorWithContext(
            err,
            Some(ErrorContext::Item(name.get_final_ident())),
        )),
        Ok(Annotated {
            ty: syn::Type::Path(ref typ),
            ..
        }) if QualifiedName::from_type_path(typ) == *name => Err(ConvertErrorWithContext(
            ConvertError::InfinitelyRecursiveTypedef(name.clone()),
            Some(ErrorContext::Item(name.get_final_ident())),
        )),
        Ok(mut final_type) => {
            converted_type.ty = Box::new(final_type.ty.clone());
            extra_apis.append(&mut final_type.extra_apis);
            deps.extend(final_type.types_encountered);
            Ok(ApiDetail::Typedef {
                item: TypedefKind::Type(ity),
                analysis: TypedefKind::Type(converted_type),
            })
        }
    }
}
