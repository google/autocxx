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

use crate::{conversion::api::Api, known_types::KNOWN_TYPES};
use crate::{
    conversion::{
        api::{ApiDetail, Use},
        codegen_cpp::AdditionalNeed,
    },
    types::{make_ident, Namespace},
};

use super::fun::FnAnalysis;

pub(crate) fn append_ctype_information(apis: &mut Vec<Api<FnAnalysis>>) {
    let ctypes: HashSet<_> = apis
        .iter()
        .map(|api| api.deps.iter())
        .flatten()
        .filter(|ty| KNOWN_TYPES.is_ctype(ty))
        .cloned()
        .collect();
    for ctype in ctypes {
        let id = make_ident(ctype.get_final_ident());
        apis.push(Api {
            ns: Namespace::new(),
            id: id.clone(),
            use_stmt: Use::Unused,
            deps: HashSet::new(),
            id_for_allowlist: None,
            additional_cpp: Some(AdditionalNeed::CTypeTypedef(ctype.clone())),
            detail: ApiDetail::CType { id },
        });
    }
}
