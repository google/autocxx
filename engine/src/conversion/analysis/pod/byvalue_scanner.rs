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

use autocxx_parser::TypeConfig;
use syn::{Item, Type, TypePath};

use crate::{
    conversion::{
        api::{ApiDetail, UnanalyzedApi},
        ConvertError,
    },
    known_types::KNOWN_TYPES,
    types::TypeName,
};

use super::ByValueChecker;

/// Scan APIs to work out which are by-value safe. Constructs a [ByValueChecker]
/// that others can use to query the results.
pub(crate) fn identify_byvalue_safe_types(
    apis: &[UnanalyzedApi],
    type_config: &TypeConfig,
) -> Result<ByValueChecker, ConvertError> {
    let mut byvalue_checker = ByValueChecker::new();
    byvalue_checker.ingest_blocklist(type_config.get_blocklist());
    for api in apis {
        match &api.detail {
            ApiDetail::Typedef { type_item } => {
                let name = api.typename();
                let typedef_type = analyze_typedef_target(type_item.ty.as_ref());
                match &typedef_type {
                    Some(typ) => {
                        byvalue_checker.ingest_simple_typedef(name, TypeName::from_type_path(&typ))
                    }
                    None => byvalue_checker.ingest_nonpod_type(name),
                }
            }
            ApiDetail::Type {
                ty_details: _,
                for_extern_c_ts: _,
                is_forward_declaration: _,
                bindgen_mod_item,
                analysis: _,
            } => match bindgen_mod_item {
                None => {}
                Some(Item::Struct(s)) => byvalue_checker.ingest_struct(&s, &api.ns),
                Some(Item::Enum(_)) => byvalue_checker.ingest_pod_type(api.typename()),
                _ => {}
            },
            ApiDetail::OpaqueTypedef => byvalue_checker.ingest_nonpod_type(api.typename()),
            _ => {}
        }
    }
    let pod_requests = type_config
        .get_pod_requests()
        .iter()
        .map(|ty| TypeName::new_from_user_input(ty))
        .collect();
    byvalue_checker
        .satisfy_requests(pod_requests)
        .map_err(ConvertError::UnsafePodType)?;
    Ok(byvalue_checker)
}

fn analyze_typedef_target(ty: &Type) -> Option<TypePath> {
    match ty {
        Type::Path(typ) => KNOWN_TYPES.known_type_substitute_path(&typ),
        _ => None,
    }
}
