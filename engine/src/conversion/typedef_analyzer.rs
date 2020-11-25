use syn::Type;

use crate::types::TypeName;

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

/// Analysis of a typedef.
#[derive(Debug)]
pub(crate) enum TypedefTarget {
    NoArguments(TypeName),
    HasArguments,
    SomethingComplex,
}

pub(crate) fn analyze_typedef_target(ty: &Type) -> TypedefTarget {
    match ty {
        Type::Path(typ) => {
            let seg = typ.path.segments.last().unwrap();
            if seg.arguments.is_empty() {
                TypedefTarget::NoArguments(TypeName::from_type_path(typ))
            } else {
                TypedefTarget::HasArguments
            }
        }
        _ => TypedefTarget::SomethingComplex,
    }
}
