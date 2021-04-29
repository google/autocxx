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

use super::fun::FnAnalysis;
use crate::conversion::{convert_error::ErrorContext, ConvertError};
use crate::{
    conversion::api::{Api, ApiDetail},
    known_types,
};

/// Remove any APIs which depend on other items which have been ignored.
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
    let mut ignored_items: HashSet<_> = ignored_items
        .into_iter()
        .map(|api| api.typename())
        .collect();
    let valid_types: HashSet<_> = valid_items.into_iter().map(|api| api.typename()).collect();
    let mut iterate_again = true;
    while iterate_again {
        iterate_again = false;
        apis = apis
            .into_iter()
            .map(|api| {
                if api.deps.iter().any(|dep| ignored_items.contains(dep)) {
                    iterate_again = true;
                    ignored_items.insert(api.typename());
                    create_ignore_item(api, ConvertError::IgnoredDependent)
                } else if !api
                    .deps
                    .iter()
                    .all(|dep| valid_types.contains(dep) || known_types().is_known_type(dep))
                {
                    iterate_again = true;
                    ignored_items.insert(api.typename());
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
    Api {
        name: api.typename(),
        deps: HashSet::new(),
        detail: ApiDetail::IgnoredItem {
            err,
            ctx: ErrorContext::Item(api.typename().get_final_ident()), // TODO
        },
    }
}
