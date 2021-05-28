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

use crate::{
    conversion::{
        api::{Api, ApiDetail},
        error_reporter::convert_item_apis,
    },
    types::{validate_ident_ok_for_rust, QualifiedName},
};

use super::fun::FnAnalysis;

/// If any items have names which can't be represented by cxx,
/// abort.
pub(crate) fn check_names(apis: Vec<Api<FnAnalysis>>) -> Vec<Api<FnAnalysis>> {
    let mut results = Vec::new();
    convert_item_apis(apis, &mut results, |api| match api.detail {
        ApiDetail::Typedef { .. }
        | ApiDetail::ForwardDeclaration
        | ApiDetail::Const { .. }
        | ApiDetail::Enum { .. }
        | ApiDetail::Struct { .. } => {
            let cxx_name = api
                .original_name
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
    results
}
