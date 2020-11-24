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

use std::collections::HashMap;

#[derive(PartialEq, Debug)]
pub(crate) struct MethodOverload {
    pub(crate) cpp_method_name: String,
    pub(crate) rust_method_name: String,
}

impl MethodOverload {
    fn new(cpp_method_name: String, rust_method_name: String) -> Self {
        Self {
            cpp_method_name,
            rust_method_name,
        }
    }
}

/// Registry of all the overloads of a function found within a given
/// namespace (i.e. mod in bindgen's output).
/// The idea here is that bindgen will output a series of overridden
/// 'foo' functions as foo, foo1, foo2.
/// We will recognize that sequence and call the correct underlying
/// C++ function ("foo" in all cases).
/// For extra complexity, if multiple types each have 'foo' methods
/// it's part of the same global numbering series within bindgen
/// output, whereas we would like to use plain old 'foo' as the method
/// names in the impl blocks we generate. This is more important than
/// it seems, because otherwise two different types with a 'get()'
/// method would instead have a 'get()' and 'get1()' method in the
/// bindings we generate.
#[derive(Default)]
pub(crate) struct OverloadTracker {
    offset_by_type_and_name: HashMap<String, HashMap<String, usize>>,
    expected_next_by_name: HashMap<String, usize>,
}

static NULL_TYPE: &str = "<null>"; // for global functions not methods

impl OverloadTracker {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn split_name(found_name: &str) -> (&str, usize) {
        for (pos, ch) in found_name.chars().rev().enumerate() {
            if !ch.is_numeric() {
                let split = found_name.len() - pos;
                let prefix = &found_name[0..split];
                let suffix = &found_name[split..];
                let counter = if suffix.is_empty() {
                    0
                } else {
                    suffix.parse::<usize>().unwrap()
                };
                return (prefix, counter);
            }
        }
        panic!("Identifier was entirely numeric");
    }

    pub(crate) fn get_function_real_name<'a>(&mut self, found_name: &'a str) -> MethodOverload {
        self.get_method_real_name(NULL_TYPE, found_name)
    }

    pub(crate) fn get_method_real_name(
        &mut self,
        type_name: &str,
        found_name: &str,
    ) -> MethodOverload {
        let (fn_name, counter) = Self::split_name(found_name);
        let expected_next_suffix = self
            .expected_next_by_name
            .entry(fn_name.to_owned())
            .or_insert(0usize);
        if counter != *expected_next_suffix {
            // This is not some kind of overload thing.
            // Instead, this is probably some function legitimately called 'insert2' or somesuch.
            MethodOverload::new(found_name.to_owned(), found_name.to_owned())
        } else {
            // Possibly part of an overload sequence. We have no way to be sure
            // but let's assume so.
            *expected_next_suffix += 1;
            let type_e = self
                .offset_by_type_and_name
                .entry(type_name.to_string())
                .or_insert_with(HashMap::new);
            let offset = type_e.entry(fn_name.to_string()).or_insert(counter);
            let effective_count = counter - *offset;
            MethodOverload::new(
                fn_name.to_string(),
                if effective_count == 0 {
                    fn_name.to_string()
                } else {
                    format!("{}{}", fn_name, effective_count)
                },
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MethodOverload, OverloadTracker};

    #[test]
    fn test_by_function() {
        let mut ot = OverloadTracker::new();
        assert_eq!(
            ot.get_function_real_name("job"),
            MethodOverload::new("job".into(), "job".into())
        );
        assert_eq!(
            ot.get_function_real_name("job1"),
            MethodOverload::new("job".into(), "job1".into())
        );
        assert_eq!(
            ot.get_function_real_name("job2"),
            MethodOverload::new("job".into(), "job2".into())
        );
        assert_eq!(
            ot.get_function_real_name("job24"),
            MethodOverload::new("job24".into(), "job24".into())
        );
        assert_eq!(
            ot.get_function_real_name("fish1"),
            MethodOverload::new("fish1".into(), "fish1".into())
        );
        assert_eq!(
            ot.get_function_real_name("fish2"),
            MethodOverload::new("fish2".into(), "fish2".into())
        );
    }

    #[test]
    fn test_by_method() {
        let mut ot = OverloadTracker::new();
        assert_eq!(
            ot.get_method_real_name("A", "do"),
            MethodOverload::new("do".into(), "do".into())
        );
        assert_eq!(
            ot.get_method_real_name("A", "do1"),
            MethodOverload::new("do".into(), "do1".into())
        );
        assert_eq!(
            ot.get_method_real_name("A", "dog"),
            MethodOverload::new("dog".into(), "dog".into())
        );
        assert_eq!(
            ot.get_method_real_name("A", "dog1"),
            MethodOverload::new("dog".into(), "dog1".into())
        );
        assert_eq!(
            ot.get_method_real_name("B", "do2"),
            MethodOverload::new("do".into(), "do".into())
        );
        assert_eq!(
            ot.get_method_real_name("B", "do3"),
            MethodOverload::new("do".into(), "do1".into())
        );
        assert_eq!(
            ot.get_method_real_name("C", "do2"),
            MethodOverload::new("do2".into(), "do2".into())
        );
    }
}
