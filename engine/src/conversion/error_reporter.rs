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

use super::{
    api::{AnalysisPhase, Api, ApiName},
    convert_error::{ConvertErrorWithContext, ErrorContext},
    ConvertError,
};
use crate::types::Namespace;

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
            push_ignored_item(ns, ctx, err, apis);
            None
        }
    }
}

/// Run some code which generates an API. Add that API, or if
/// anything goes wrong, instead add a note of the problem in our
/// output API such that users will see documentation for the problem.
pub(crate) fn convert_apis<F, A, B>(in_apis: Vec<Api<A>>, out_apis: &mut Vec<Api<B>>, mut fun: F)
where
    F: FnMut(Api<A>) -> Result<Option<Api<B>>, ConvertErrorWithContext>,
    A: AnalysisPhase,
    B: AnalysisPhase,
{
    out_apis.extend(in_apis.into_iter().filter_map(|api| {
        let tn = api.name().clone();
        match fun(api) {
            Ok(opt) => opt,
            Err(ConvertErrorWithContext(err, None)) => {
                eprintln!("Ignored {}: {}", tn.to_string(), err);
                None
            }
            Err(ConvertErrorWithContext(err, Some(ctx))) => {
                eprintln!("Ignored {}: {}", tn.to_string(), err);
                Some(ignored_item(tn.get_namespace(), ctx, err))
            }
        }
    }))
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
    convert_apis(in_apis, out_apis, |api| {
        let id = api.name().get_final_ident();
        fun(api).map_err(|e| ConvertErrorWithContext(e, Some(ErrorContext::Item(id))))
    })
}

fn ignored_item<A: AnalysisPhase>(ns: &Namespace, ctx: ErrorContext, err: ConvertError) -> Api<A> {
    Api::IgnoredItem {
        common: ApiName::new(ns, ctx.get_id().clone()),
        err,
        ctx,
    }
}

fn push_ignored_item(
    ns: &Namespace,
    ctx: ErrorContext,
    err: ConvertError,
    apis: &mut Vec<Api<impl AnalysisPhase>>,
) {
    apis.push(ignored_item(ns, ctx, err));
}
