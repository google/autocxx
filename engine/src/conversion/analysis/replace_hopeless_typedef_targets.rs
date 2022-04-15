// Copyright 2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::collections::HashSet;

use crate::{
    conversion::{analysis::tdef::TypedefAnalysis, api::Api, apivec::ApiVec},
    types::QualifiedName,
};

use super::pod::PodPhase;
/// Where we find a typedef pointing at something we can't represent,
/// e.g. because it uses too many template parameters, break the link.
/// Use the typedef as a first-class type.
pub(crate) fn replace_hopeless_typedef_targets(apis: ApiVec<PodPhase>) -> ApiVec<PodPhase> {
    let ignored_types: HashSet<QualifiedName> = apis
        .iter()
        .filter_map(|api| match api {
            Api::IgnoredItem { .. } => Some(api.name()),
            _ => None,
        })
        .cloned()
        .collect();
    apis.into_iter()
        .map(|api| match api {
            Api::Typedef {
                analysis: TypedefAnalysis { ref deps, .. },
                ..
            } if !ignored_types.is_disjoint(deps) => Api::OpaqueType {
                name: api.name_info().clone(),
            },
            _ => api,
        })
        .collect()
}
