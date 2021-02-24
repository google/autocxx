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

use std::collections::{HashMap, HashSet};

use syn::Ident;

use crate::{conversion::api::Api, known_types::KNOWN_TYPES, types::TypeName};
use crate::{
    conversion::{api::ApiDetail, codegen_cpp::AdditionalNeed},
    types::{make_ident, Namespace},
};

use super::fun::FnAnalysis;

/// Spot any variable-length C types (e.g. unsigned long)
/// used in the [Api]s and append those as extra APIs.
pub(crate) fn append_ctype_information(apis: &mut Vec<Api<FnAnalysis>>) {
    let ctypes: HashMap<Ident, TypeName> = apis
        .iter()
        .map(|api| api.deps.iter())
        .flatten()
        .filter(|ty| KNOWN_TYPES.is_ctype(ty))
        .map(|ty| (make_ident(ty.get_final_ident()), ty.clone()))
        .collect();
    for (id, tn) in ctypes {
        apis.push(Api {
            ns: Namespace::new(),
            id,
            deps: HashSet::new(),
            additional_cpp: Some(AdditionalNeed::CTypeTypedef(tn)),
            detail: ApiDetail::CType,
        });
    }
}
