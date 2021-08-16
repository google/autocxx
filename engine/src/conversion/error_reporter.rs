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

use syn::{ItemEnum, ItemStruct};

use super::{
    api::{AnalysisPhase, Api, ApiName, FuncToConvert, TypedefKind},
    convert_error::{ConvertErrorWithContext, ErrorContext},
    ConvertError,
};
use crate::types::{Namespace, QualifiedName};

/// Run some code which may generate a ConvertError.
/// If it does, try to note the problem in our output APIs
/// such that users will see documentation of the error.
pub(crate) fn report_any_error<F, T>(
    ns: &Namespace,
    apis: &mut Vec<Api<impl AnalysisPhase>>,
    fun: F,
) -> Option<T>
where
    F: FnOnce() -> Result<T, ConvertErrorWithContext>,
{
    match fun() {
        Ok(result) => Some(result),
        Err(ConvertErrorWithContext(err, None)) => {
            eprintln!("Ignored item: {}", err);
            None
        }
        Err(ConvertErrorWithContext(err, Some(ctx))) => {
            eprintln!("Ignored item {}: {}", ctx.to_string(), err);
            apis.push(ignored_item(ns, ctx, err));
            None
        }
    }
}

/// Run some code which generates an API. Add that API, or if
/// anything goes wrong, instead add a note of the problem in our
/// output API such that users will see documentation for the problem.
pub(crate) fn convert_apis<FF, SF, EF, TF, A, B>(
    in_apis: Vec<Api<A>>,
    out_apis: &mut Vec<Api<B>>,
    mut func_conversion: FF,
    mut struct_conversion: SF,
    mut enum_conversion: EF,
    mut typedef_conversion: TF,
) where
    A: AnalysisPhase,
    B: AnalysisPhase,
    FF: FnMut(
        ApiName,
        Box<FuncToConvert>,
        A::FunAnalysis,
    ) -> Result<Option<Api<B>>, ConvertErrorWithContext>,
    SF: FnMut(
        ApiName,
        ItemStruct,
        A::StructAnalysis,
    ) -> Result<Option<Api<B>>, ConvertErrorWithContext>,
    EF: FnMut(ApiName, ItemEnum) -> Result<Option<Api<B>>, ConvertErrorWithContext>,
    TF: FnMut(
        ApiName,
        TypedefKind,
        Option<QualifiedName>,
        A::TypedefAnalysis,
    ) -> Result<Option<Api<B>>, ConvertErrorWithContext>,
{
    out_apis.extend(in_apis.into_iter().filter_map(|api| {
        let tn = api.name().clone();
        let result = match api {
            // No changes to any of these...
            Api::ConcreteType {
                name,
                rs_definition,
                cpp_definition,
            } => Ok(Some(Api::ConcreteType {
                name,
                rs_definition,
                cpp_definition,
            })),
            Api::ForwardDeclaration { name } => Ok(Some(Api::ForwardDeclaration { name })),
            Api::StringConstructor { name } => Ok(Some(Api::StringConstructor { name })),
            Api::Const { name, const_item } => Ok(Some(Api::Const { name, const_item })),
            Api::CType { name, typename } => Ok(Some(Api::CType { name, typename })),
            Api::RustType { name } => Ok(Some(Api::RustType { name })),
            Api::IgnoredItem { name, err, ctx } => Ok(Some(Api::IgnoredItem { name, err, ctx })),
            // Apply a mapping to the following
            Api::Enum { name, item } => enum_conversion(name, item),
            Api::Typedef {
                name,
                item,
                old_tyname,
                analysis,
            } => typedef_conversion(name, item, old_tyname, analysis),
            Api::Function {
                name,
                fun,
                analysis,
            } => func_conversion(name, fun, analysis),
            Api::Struct {
                name,
                item,
                analysis,
            } => struct_conversion(name, item, analysis),
        };
        api_or_error(tn, result)
    }))
}

fn api_or_error<T: AnalysisPhase>(
    name: QualifiedName,
    api_or_error: Result<Option<Api<T>>, ConvertErrorWithContext>,
) -> Option<Api<T>> {
    match api_or_error {
        Ok(opt) => opt,
        Err(ConvertErrorWithContext(err, None)) => {
            eprintln!("Ignored {}: {}", name.to_string(), err);
            None
        }
        Err(ConvertErrorWithContext(err, Some(ctx))) => {
            eprintln!("Ignored {}: {}", name.to_string(), err);
            Some(ignored_item(name.get_namespace(), ctx, err))
        }
    }
}

/// Run some code which generates an API for an item (as opposed to
/// a method). Add that API, or if
/// anything goes wrong, instead add a note of the problem in our
/// output API such that users will see documentation for the problem.
pub(crate) fn convert_item_apis<F, A, B>(
    in_apis: Vec<Api<A>>,
    out_apis: &mut Vec<Api<B>>,
    mut fun: F,
) where
    F: FnMut(Api<A>) -> Result<Option<Api<B>>, ConvertError>,
    A: AnalysisPhase,
    B: AnalysisPhase,
{
    out_apis.extend(in_apis.into_iter().filter_map(|api| {
        let tn = api.name().clone();
        let result = fun(api).map_err(|e| {
            ConvertErrorWithContext(e, Some(ErrorContext::Item(tn.get_final_ident())))
        });
        api_or_error(tn, result)
    }))
}

fn ignored_item<A: AnalysisPhase>(ns: &Namespace, ctx: ErrorContext, err: ConvertError) -> Api<A> {
    Api::IgnoredItem {
        name: ApiName::new(ns, ctx.get_id().clone()),
        err,
        ctx,
    }
}
