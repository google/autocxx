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
    api::{AnalysisPhase, Api, ApiDetail},
    convert_error::{ConvertErrorWithContext, ErrorContext},
    ConvertError,
};
use crate::types::{Namespace, QualifiedName};
use std::collections::HashSet;

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
pub(crate) fn add_api_or_report_error<F, A>(tn: QualifiedName, apis: &mut Vec<Api<A>>, fun: F)
where
    F: FnOnce() -> Result<Option<Api<A>>, ConvertErrorWithContext>,
    A: AnalysisPhase,
{
    match fun() {
        Ok(Some(api)) => {
            apis.push(api);
        }
        Ok(None) => {}
        Err(ConvertErrorWithContext(err, None)) => {
            eprintln!("Ignored {}: {}", tn.to_string(), err);
        }
        Err(ConvertErrorWithContext(err, Some(ctx))) => {
            eprintln!("Ignored {}: {}", tn.to_string(), err);
            push_ignored_item(tn.get_namespace(), ctx, err, apis);
        }
    }
}

fn push_ignored_item(
    ns: &Namespace,
    ctx: ErrorContext,
    err: ConvertError,
    apis: &mut Vec<Api<impl AnalysisPhase>>,
) {
    apis.push(Api {
        name: QualifiedName::new(ns, ctx.get_id().clone()),
        original_name: None,
        deps: HashSet::new(),
        detail: ApiDetail::IgnoredItem { err, ctx },
    });
}
