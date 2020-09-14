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

use crate::TypeName;

const CXXBRIDGE_GENERATION: usize = 4;

pub enum EncounteredTypeKind {
    Struct,
    Enum,
}

pub struct EncounteredType(pub EncounteredTypeKind, pub TypeName);

/// Edits the generated C++ to insert #defines to disable certain C++
/// type definitions. A nasty temporary hack - see
pub(crate) fn disable_types(mut input: Vec<u8>, types: &Vec<EncounteredType>) -> Vec<u8> {
    let mut out = Vec::new();
    for t in types {
        let label = match t.0 {
            EncounteredTypeKind::Struct => "STRUCT",
            EncounteredTypeKind::Enum => "ENUM",
        };
        out.extend_from_slice(
            format!(
                "#define CXXBRIDGE{:02}_{}_{}\n",
                CXXBRIDGE_GENERATION, label, t.1
            )
            .as_bytes(),
        );
    }
    if !types.is_empty() {
        out.extend_from_slice("\n".as_bytes());
    }
    out.append(&mut input);
    out
}

#[cfg(test)]
mod tests {
    use crate::TypeName;

    #[test]
    fn test_type_disabler() {
        let types = vec![
            super::EncounteredType(super::EncounteredTypeKind::Enum, TypeName::new("foo")),
            super::EncounteredType(super::EncounteredTypeKind::Struct, TypeName::new("bar")),
        ];
        let input = "fish\n\n".as_bytes().to_vec();
        let output = super::disable_types(input, &types);
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "#define CXXBRIDGE04_ENUM_foo\n#define CXXBRIDGE04_STRUCT_bar\n\nfish\n\n"
        );
    }
}
