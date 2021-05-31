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

use autocxx_parser::IncludeCppConfig;
use syn::ItemType;

use crate::{
    conversion::{
        analysis::type_converter::{add_analysis, Annotated, TypeConversionContext, TypeConverter},
        api::{AnalysisPhase, Api, ApiCommon, TypedefKind, UnanalyzedApi},
        convert_error::{ConvertErrorWithContext, ErrorContext},
        error_reporter::report_any_error,
        ConvertError,
    },
    types::QualifiedName,
};

use super::remove_bindgen_attrs;

/// Analysis phase where typedef analysis has been performed but no other
/// analyses just yet.
pub(crate) struct TypedefAnalysis;

impl AnalysisPhase for TypedefAnalysis {
    type TypedefAnalysis = TypedefKind;
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
            Api::ForwardDeclaration { common } => Some(Api::ForwardDeclaration { common }),
            Api::ConcreteType {
                common,
                rs_definition,
                cpp_definition,
            } => Some(Api::ConcreteType {
                common,
                rs_definition,
                cpp_definition,
            }),
            Api::StringConstructor { common } => Some(Api::StringConstructor { common }),
            Api::Function {
                common,
                fun,
                analysis,
            } => Some(Api::Function {
                common,
                fun,
                analysis,
            }),
            Api::Const { common, const_item } => Some(Api::Const { common, const_item }),
            Api::Typedef {
                common,
                item: TypedefKind::Type(ity),
                analysis: _,
            } => report_any_error(
                &common.name.get_namespace().clone(),
                &mut problem_apis,
                || get_replacement_typedef(common, ity, &mut type_converter, &mut extra_apis),
            ),
            Api::Typedef {
                common,
                item,
                analysis: _,
            } => Some(Api::Typedef {
                common,
                item: item.clone(),
                analysis: item,
            }),
            Api::Struct {
                common,
                item,
                analysis,
            } => Some(Api::Struct {
                common,
                item,
                analysis,
            }),
            Api::Enum { common, item } => Some(Api::Enum { common, item }),
            Api::CType { common, typename } => Some(Api::CType { common, typename }),
            Api::IgnoredItem { common, err, ctx } => Some(Api::IgnoredItem { common, err, ctx }),
        })
        .collect::<Vec<_>>();
    new_apis
        .into_iter()
        .chain(extra_apis.into_iter().chain(problem_apis).map(add_analysis))
        .collect()
}

fn get_replacement_typedef(
    mut common: ApiCommon,
    ity: ItemType,
    type_converter: &mut TypeConverter,
    extra_apis: &mut Vec<UnanalyzedApi>,
) -> Result<Api<TypedefAnalysis>, ConvertErrorWithContext> {
    let mut converted_type = ity.clone();
    let id = ity.ident.clone();
    remove_bindgen_attrs(&mut converted_type.attrs)
        .map_err(|e| ConvertErrorWithContext(e, Some(ErrorContext::Item(id))))?;
    let type_conversion_results = type_converter.convert_type(
        (*ity.ty).clone(),
        common.name.get_namespace(),
        &TypeConversionContext::CxxInnerType,
    );
    match type_conversion_results {
        Err(err) => Err(ConvertErrorWithContext(
            err,
            Some(ErrorContext::Item(common.name.get_final_ident())),
        )),
        Ok(Annotated {
            ty: syn::Type::Path(ref typ),
            ..
        }) if QualifiedName::from_type_path(typ) == common.name => Err(ConvertErrorWithContext(
            ConvertError::InfinitelyRecursiveTypedef(common.name.clone()),
            Some(ErrorContext::Item(common.name.get_final_ident())),
        )),
        Ok(mut final_type) => {
            converted_type.ty = Box::new(final_type.ty.clone());
            extra_apis.append(&mut final_type.extra_apis);
            common.deps.extend(final_type.types_encountered);
            Ok(Api::Typedef {
                common,
                item: TypedefKind::Type(ity),
                analysis: TypedefKind::Type(converted_type),
            })
        }
    }
}
