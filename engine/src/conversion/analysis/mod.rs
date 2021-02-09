use syn::Item;

use super::api::{Api, ApiAnalysis, ApiDetail};

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

pub(crate) mod ctypes;
pub(crate) mod gc;
pub(crate) mod pod; // hey, that rhymes

fn apply_type_analysis<T: ApiAnalysis,U: ApiAnalysis>(api: Api<T>, new_type_analysis: U::TypeAnalysis, new_item_analysis: U::ItemAnalysis) -> Api<U> {
    let api_detail = match api.detail {
        ApiDetail::ConcreteType(details) => ApiDetail::ConcreteType(details),
        ApiDetail::StringConstructor => ApiDetail::StringConstructor,
        ApiDetail::ImplEntry { impl_entry } => ApiDetail::ImplEntry { impl_entry },
        ApiDetail::Function { extern_c_mod_item } => ApiDetail::Function { extern_c_mod_item },
        ApiDetail::Const { const_item } => ApiDetail::Const { const_item },
        ApiDetail::Typedef { type_item } => ApiDetail::Typedef { type_item },
        ApiDetail::CType { id } => ApiDetail::CType { id },
        ApiDetail::Type { ty_details, for_extern_c_ts, type_kind, bindgen_mod_item, analysis } => ApiDetail::Type {
            ty_details, for_extern_c_ts, type_kind, bindgen_mod_item, analysis: new_type_analysis,
        }
    };
    Api {
        ns: api.ns,
        id: api.id,
        use_stmt: api.use_stmt,
        deps: api.deps,
        id_for_allowlist: api.id_for_allowlist,
        additional_cpp: api.additional_cpp,
        detail: api_detail,
        analysis: new_item_analysis,
    }
}
