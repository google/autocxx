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

mod byvalue_checker;
mod byvalue_scanner;

pub(crate) use byvalue_checker::ByValueChecker;
pub(crate) use byvalue_scanner::identify_byvalue_safe_types;

use crate::conversion::api::{Api, ApiAnalysis, ApiDetail, TypeKind, UnanalyzedApi};

use super::apply_type_analysis;

pub(crate) struct PodAnalysis;

impl ApiAnalysis for PodAnalysis {
    type ItemAnalysis = ();
    type TypeAnalysis = TypeKind;
}

pub(crate) fn analyze_pod_apis(apis: Vec<UnanalyzedApi>, byvalue_checker: &ByValueChecker) -> Vec<Api<PodAnalysis>> {
    apis.into_iter().map(|api| analyze_pod_api(api, byvalue_checker)).collect()
}

fn analyze_pod_api(api: UnanalyzedApi, byvalue_checker: &ByValueChecker) -> Api<PodAnalysis> {
    let new_type_kind = TypeKind::POD;
    apply_type_analysis(api, new_type_kind, ())
}
