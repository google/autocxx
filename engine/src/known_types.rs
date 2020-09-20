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
use lazy_static::lazy_static;
use std::collections::HashMap;

#[derive(Debug)]
pub(crate) struct TypeDetails {
    /// Substitutions from the names constructed by bindgen into those
    /// which cxx uses.
    pub(crate) cxx_replacement: Option<TypeName>,
    /// C++ equivalent name for a Rust type.
    pub(crate) cxx_name: Option<String>,
}

impl TypeDetails {
    fn new(cxx_replacement: Option<TypeName>, cxx_name: Option<String>) -> Self {
        TypeDetails {
            cxx_replacement,
            cxx_name,
        }
    }
}

lazy_static! {
    pub(crate) static ref KNOWN_TYPES: HashMap<TypeName, TypeDetails> = {
        let mut map = HashMap::new();
        map.insert(
            TypeName::new("std_unique_ptr"),
            TypeDetails::new(Some(TypeName::new("UniquePtr")), None),
        );
        map.insert(
            TypeName::new("std_string"),
            TypeDetails::new(Some(TypeName::new("CxxString")), None),
        );
        for (cpp_type, rust_type) in (3..7)
            .map(|x| 2i32.pow(x))
            .map(|x| {
                vec![
                    (format!("uint{}_t", x), format!("u{}", x)),
                    (format!("int{}_t", x), format!("i{}", x)),
                ]
            })
            .flatten()
        {
            map.insert(
                TypeName::new(&rust_type),
                TypeDetails::new(None, Some(cpp_type)),
            );
        }
        map
    };
}

#[cfg(test)]
mod tests {
    use crate::TypeName;

    #[test]
    fn test_ints() {
        assert_eq!(
            super::KNOWN_TYPES
                .get(&TypeName::new("i8"))
                .unwrap()
                .cxx_name
                .as_ref()
                .unwrap(),
            "int8_t"
        );
        assert_eq!(
            super::KNOWN_TYPES
                .get(&TypeName::new("u64"))
                .unwrap()
                .cxx_name
                .as_ref()
                .unwrap(),
            "uint64_t"
        );
    }
}
