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

use autocxx_parser::IncludeCppConfig;
use syn::ItemType;

use crate::{
    conversion::{
        analysis::type_converter::{add_analysis, Annotated, TypeConversionContext, TypeConverter},
        api::{AnalysisPhase, Api, ApiName, TypedefKind, UnanalyzedApi},
        convert_error::{ConvertErrorWithContext, ErrorContext},
        error_reporter::report_any_error,
        ConvertError,
    },
    types::QualifiedName,
};

use super::remove_bindgen_attrs;

pub(crate) struct TypedefAnalysisBody {
    pub(crate) kind: TypedefKind,
    pub(crate) deps: HashSet<QualifiedName>,
}

/// Analysis phase where typedef analysis has been performed but no other
/// analyses just yet.
pub(crate) struct TypedefAnalysis;

impl AnalysisPhase for TypedefAnalysis {
    type TypedefAnalysis = TypedefAnalysisBody;
    type StructAnalysis = ();
    type FunAnalysis = ();
}

#[allow(clippy::needless_collect)] // we need the extra collect because the closure borrows extra_apis
pub(crate) fn convert_typedef_targets(
    config: &IncludeCppConfig,
    apis: Vec<UnanalyzedApi>,
) -> Vec<Api<TypedefAnalysis>> {
    let mut type_converter = TypeConverter::new(config, &apis);
    let mut extra_apis = Vec::new();
    let mut problem_apis = Vec::new();
    let new_apis = apis
        .into_iter()
        .filter_map(|api| match api {
            Api::ForwardDeclaration { name } => Some(Api::ForwardDeclaration { name }),
            Api::ConcreteType {
                name,
                rs_definition,
                cpp_definition,
            } => Some(Api::ConcreteType {
                name,
                rs_definition,
                cpp_definition,
            }),
            Api::StringConstructor { name } => Some(Api::StringConstructor { name }),
            Api::Function {
                name,
                fun,
                analysis,
            } => Some(Api::Function {
                name,
                fun,
                analysis,
            }),
            Api::Const { name, const_item } => Some(Api::Const { name, const_item }),
            Api::Typedef {
                name,
                item: TypedefKind::Type(ity),
                old_tyname,
                analysis: _,
            } => report_any_error(
                &name.name.get_namespace().clone(),
                &mut problem_apis,
                || {
                    get_replacement_typedef(
                        name,
                        ity,
                        old_tyname,
                        &mut type_converter,
                        &mut extra_apis,
                    )
                },
            ),
            Api::Typedef {
                name,
                item,
                old_tyname,
                analysis: _,
            } => Some(Api::Typedef {
                name,
                item: item.clone(),
                old_tyname,
                analysis: TypedefAnalysisBody {
                    kind: item,
                    deps: HashSet::new(),
                },
            }),
            Api::Struct {
                name,
                item,
                analysis,
            } => Some(Api::Struct {
                name,
                item,
                analysis,
            }),
            Api::Enum { name, item } => Some(Api::Enum { name, item }),
            Api::CType { name, typename } => Some(Api::CType { name, typename }),
            Api::IgnoredItem { name, err, ctx } => Some(Api::IgnoredItem { name, err, ctx }),
        })
        .collect::<Vec<_>>();
    new_apis
        .into_iter()
        .chain(extra_apis.into_iter().chain(problem_apis).map(add_analysis))
        .collect()
}

fn get_replacement_typedef(
    name: ApiName,
    ity: ItemType,
    old_tyname: Option<QualifiedName>,
    type_converter: &mut TypeConverter,
    extra_apis: &mut Vec<UnanalyzedApi>,
) -> Result<Api<TypedefAnalysis>, ConvertErrorWithContext> {
    let mut converted_type = ity.clone();
    let id = ity.ident.clone();
    remove_bindgen_attrs(&mut converted_type.attrs)
        .map_err(|e| ConvertErrorWithContext(e, Some(ErrorContext::Item(id))))?;
    let type_conversion_results = type_converter.convert_type(
        (*ity.ty).clone(),
        name.name.get_namespace(),
        &TypeConversionContext::CxxInnerType,
    );
    match type_conversion_results {
        Err(err) => Err(ConvertErrorWithContext(
            err,
            Some(ErrorContext::Item(name.name.get_final_ident())),
        )),
        Ok(Annotated {
            ty: syn::Type::Path(ref typ),
            ..
        }) if QualifiedName::from_type_path(typ) == name.name => Err(ConvertErrorWithContext(
            ConvertError::InfinitelyRecursiveTypedef(name.name.clone()),
            Some(ErrorContext::Item(name.name.get_final_ident())),
        )),
        Ok(mut final_type) => {
            converted_type.ty = Box::new(final_type.ty.clone());
            extra_apis.append(&mut final_type.extra_apis);
            Ok(Api::Typedef {
                name,
                item: TypedefKind::Type(ity),
                old_tyname,
                analysis: TypedefAnalysisBody {
                    kind: TypedefKind::Type(converted_type),
                    deps: final_type.types_encountered,
                },
            })
        }
    }
}
