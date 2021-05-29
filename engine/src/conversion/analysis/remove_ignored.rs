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

use std::collections::HashSet;

use super::fun::{FnAnalysis, FnAnalysisBody, FnKind};
use crate::conversion::{convert_error::ErrorContext, ConvertError};
use crate::{
    conversion::api::{Api, ApiDetail},
    known_types,
};

/// Remove any APIs which depend on other items which have been ignored.
/// We also eliminate any APIs that depend on some type that we just don't
/// know about at all. In either case, we don't simply remove the type, but instead
/// replace it with an error marker.
pub(crate) fn filter_apis_by_ignored_dependents(
    mut apis: Vec<Api<FnAnalysis>>,
) -> Vec<Api<FnAnalysis>> {
    let (ignored_items, valid_items): (Vec<&Api<_>>, Vec<&Api<_>>) = apis.iter().partition(|api| {
        matches!(
            api.detail,
            ApiDetail::IgnoredItem {
                ctx: ErrorContext::Item(..),
                ..
            }
        )
    });
    let mut ignored_items: HashSet<_> = ignored_items.into_iter().map(|api| api.name()).collect();
    let valid_types: HashSet<_> = valid_items.into_iter().map(|api| api.name()).collect();
    let mut iterate_again = true;
    while iterate_again {
        iterate_again = false;
        apis = apis
            .into_iter()
            .map(|api| {
                if api.deps.iter().any(|dep| ignored_items.contains(dep)) {
                    iterate_again = true;
                    ignored_items.insert(api.name());
                    create_ignore_item(api, ConvertError::IgnoredDependent)
                } else if !api
                    .deps
                    .iter()
                    .all(|dep| valid_types.contains(dep) || known_types().is_known_type(dep))
                {
                    iterate_again = true;
                    ignored_items.insert(api.name());
                    create_ignore_item(api, ConvertError::UnknownDependentType)
                } else {
                    api
                }
            })
            .collect();
    }
    apis
}

fn create_ignore_item(api: Api<FnAnalysis>, err: ConvertError) -> Api<FnAnalysis> {
    let id = api.name().get_final_ident();
    Api {
        name: api.name(),
        original_name: api.original_name,
        deps: HashSet::new(),
        detail: ApiDetail::IgnoredItem {
            err,
            ctx: match api.detail {
                ApiDetail::Function {
                    analysis:
                        FnAnalysisBody {
                            kind: FnKind::Method(self_ty, _),
                            ..
                        },
                    ..
                } => ErrorContext::Method {
                    self_ty: self_ty.get_final_ident(),
                    method: id,
                },
                _ => ErrorContext::Item(id),
            },
        },
        rename_to: api.rename_to,
    }
}
