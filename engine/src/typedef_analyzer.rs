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

use crate::known_types::KNOWN_TYPES;
use syn::{Type, TypePath};

pub(crate) fn analyze_typedef_target(ty: &Type) -> Option<TypePath> {
    match ty {
        Type::Path(typ) => KNOWN_TYPES.known_type_substitute_path(&typ),
        _ => None,
    }
}
