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

use syn::Ident;

use super::{
    api::{Api, ApiAnalysis, ApiDetail},
    convert_error::ConvertErrorWithIdent,
    ConvertError,
};
use crate::types::{make_ident, Namespace, TypeName};
use std::collections::HashSet;

/// Run some code which may generate a ConvertError.
/// If it does, try to note the problem in our output APIs
/// such that users will see documentation of the error.
pub(crate) fn report_error<F, T>(
    ns: &Namespace,
    apis: &mut Vec<Api<impl ApiAnalysis>>,
    fun: F,
) -> Result<Option<T>, ConvertError>
where
    F: FnOnce() -> Result<T, ConvertErrorWithIdent>,
{
    match fun() {
        Ok(r) => Ok(Some(r)),
        Err(ConvertErrorWithIdent(err, None)) if err.is_ignorable() => {
            eprintln!("Ignored item: {}", err);
            Ok(None)
        }
        Err(ConvertErrorWithIdent(err, Some(id))) if err.is_ignorable() => {
            eprintln!("Ignored item {}: {}", id.to_string(), err);
            push_ignored_item(ns, id, err, apis);
            Ok(None)
        }
        Err(ConvertErrorWithIdent(err, _)) => Err(err),
    }
}

/// Run some code which generates an API. Add that API, or if
/// anything goes wrong, instead add a note of the problem in our
/// output API such that users will see documentation for the problem.
pub(crate) fn add_api_or_report_error<F, A>(
    tn: TypeName,
    apis: &mut Vec<Api<A>>,
    fun: F,
) -> Result<(), ConvertError>
where
    F: FnOnce() -> Result<Option<Api<A>>, ConvertError>,
    A: ApiAnalysis,
{
    match fun() {
        Ok(Some(api)) => {
            apis.push(api);
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(err) if err.is_ignorable() => {
            eprintln!("Ignored {}: {}", tn.to_string(), err);
            push_ignored_item(
                tn.get_namespace(),
                make_ident(tn.get_final_ident()),
                err,
                apis,
            );
            Ok(())
        }
        Err(err) => Err(err),
    }
}

fn push_ignored_item(
    ns: &Namespace,
    id: Ident,
    err: ConvertError,
    apis: &mut Vec<Api<impl ApiAnalysis>>,
) {
    apis.push(Api {
        ns: ns.clone(),
        id,
        deps: HashSet::new(),
        detail: ApiDetail::IgnoredItem { err },
    });
}
