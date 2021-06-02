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

use std::collections::HashMap;

use syn::Ident;

use crate::{
    conversion::{
        api::{Api, ApiDetail},
        error_reporter::convert_item_apis,
        ConvertError,
    },
    types::{validate_ident_ok_for_rust, QualifiedName},
};

use super::fun::FnAnalysis;

/// If any items have names which can't be represented by cxx,
/// abort.
pub(crate) fn check_names(apis: Vec<Api<FnAnalysis>>) -> Vec<Api<FnAnalysis>> {
    let mut intermediate = Vec::new();
    convert_item_apis(apis, &mut intermediate, |api| match api.detail {
        ApiDetail::Typedef { .. }
        | ApiDetail::ForwardDeclaration
        | ApiDetail::Const { .. }
        | ApiDetail::Enum { .. }
        | ApiDetail::Struct { .. } => {
            let cxx_name = api
                .cpp_name
                .as_ref()
                .map(|s|
                    QualifiedName::new_from_cpp_name(&s)
                )
                .unwrap_or(api.name.clone());
            for seg in cxx_name.segment_iter() {
                validate_ident_ok_for_rust(&seg)?;
            }
            Ok(Some(api))
        }
        ApiDetail::Function { .. } // we don't handle functions here because
            // the function analysis does an equivalent check. Instead of just rejecting
            // the function, it creates a wrapper function instead with a more
            // palatable name. That's preferable to rejecting the API entirely.
        | ApiDetail::ConcreteType  { .. }
        | ApiDetail::CType { .. }
        | ApiDetail::StringConstructor
        | ApiDetail::IgnoredItem { .. } => Ok(Some(api)),
    });

    // Reject any names which are duplicates within the cxx bridge mod,
    // that has a flat namespace.
    let mut names_found: HashMap<Ident, usize> = HashMap::new();
    for api in &intermediate {
        let my_name = cxxbridge_name(api);
        if let Some(name) = my_name {
            let e = names_found.entry(name).or_default();
            *e += 1usize;
        }
    }
    let mut results = Vec::new();
    convert_item_apis(intermediate, &mut results, |api| {
        let my_name = cxxbridge_name(&api);
        if let Some(name) = my_name {
            if *names_found.entry(name).or_default() > 1usize {
                Err(ConvertError::DuplicateCxxBridgeName)
            } else {
                Ok(Some(api))
            }
        } else {
            Ok(Some(api))
        }
    });
    results
}

fn cxxbridge_name(api: &Api<FnAnalysis>) -> Option<Ident> {
    match api.detail {
        ApiDetail::Function { ref analysis, .. } => Some(analysis.cxxbridge_name.clone()),
        ApiDetail::Enum { .. }
        | ApiDetail::ForwardDeclaration
        | ApiDetail::ConcreteType { .. }
        | ApiDetail::Typedef { .. }
        | ApiDetail::Struct { .. }
        | ApiDetail::CType { .. } => Some(api.name().get_final_ident()),
        ApiDetail::StringConstructor | ApiDetail::Const { .. } | ApiDetail::IgnoredItem { .. } => {
            None
        }
    }
}
