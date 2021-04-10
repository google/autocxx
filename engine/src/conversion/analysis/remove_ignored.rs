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
use crate::conversion::api::{Api, ApiDetail};

/// Remove any APIs which depend on other items which have been ignored.
pub(crate) fn filter_apis_by_ignored_dependents(
    mut apis: Vec<Api<FnAnalysis>>,
) -> Vec<Api<FnAnalysis>> {
    let mut ignored_items: HashSet<_> = apis
        .iter()
        .filter_map(|api| {
            if matches!(api.detail, ApiDetail::IgnoredItem { .. }) {
                Some(api.typename())
            } else {
                None
            }
        })
        .collect();
    let mut iterate_again = true;
    while iterate_again {
        iterate_again = false;
        apis = apis
            .into_iter()
            .filter(|api| {
                if api.deps.iter().any(|dep| ignored_items.contains(dep)) {
                    iterate_again = true;
                    ignored_items.insert(api.typename());
                    eprintln!(
                        "Skipping item {} because it depends on another item we skipped.",
                        api.typename().to_string()
                    );
                    false
                } else {
                    true
                }
            })
            .collect();
    }
    apis
}
