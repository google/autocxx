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
use syn::{ItemStruct, Type};

pub struct ByValueChecker {
    // Mapping from type name to whether it is POD
    results: HashMap<String, bool>,
}

impl ByValueChecker {
    pub fn new() -> Self {
        let mut results = HashMap::new();
        results.insert("CxxString".to_owned(), false);
        results.insert("UniquePtr".to_owned(), true);
        results.insert("i32".to_owned(), true);
        results.insert("i64".to_owned(), true);
        // TODO expand with all primitives, or find a better way.
        ByValueChecker {
            results
        }
    }

    /// Can this C++ type be passed safely by value to/from Rust?
    pub fn type_is_safe_for_pass_by_value(&mut self, def: ItemStruct) -> bool {
        let id = def.ident.to_string();
        let mut type_representable_as_pod = true;
        for f in def.fields {
            let fty = f.ty;
            let field_representable_as_pod = match fty {
                Type::Path(p) => {
                    // TODO better handle generics
                    let ty_id = p.path.segments.last().unwrap().ident.to_string();
                    *self.results.get(&ty_id).expect("Not yet encountered type")
                },
                // TODO handle anything else which bindgen might spit out, e.g. arrays?
                _ => false,
            };
            if !field_representable_as_pod {
                type_representable_as_pod = false;
                break;
            }
        };
        self.results.insert(id, type_representable_as_pod);
        type_representable_as_pod
    }
}

#[cfg(test)]
mod tests {
    use super::ByValueChecker;
    use syn::{parse_quote, ItemStruct};

    #[test]
    fn test_primitives() {
        let mut bvc = ByValueChecker::new();
        let t: ItemStruct = parse_quote! {
            struct Foo {
                a: i32,
                b: i64,
            }
        };
        assert!(bvc.type_is_safe_for_pass_by_value(t));
    }

    #[test]
    fn test_nested_primitives() {
        let mut bvc = ByValueChecker::new();
        let t: ItemStruct = parse_quote! {
            struct Foo {
                a: i32,
                b: i64,
            }
        };
        bvc.type_is_safe_for_pass_by_value(t);
        let t: ItemStruct = parse_quote! {
            struct Bar {
                a: Foo,
                b: i64,
            }
        };
        assert!(bvc.type_is_safe_for_pass_by_value(t));
    }

    #[test]
    fn test_with_up() {
        let mut bvc = ByValueChecker::new();
        let t: ItemStruct = parse_quote! {
            struct Bar {
                a: UniquePtr<CxxString>,
                b: i64,
            }
        };
        assert!(bvc.type_is_safe_for_pass_by_value(t));
    }

    #[test]
    fn test_with_cxxstring() {
        let mut bvc = ByValueChecker::new();
        let t: ItemStruct = parse_quote! {
            struct Bar {
                a: CxxString,
                b: i64,
            }
        };
        assert!(!bvc.type_is_safe_for_pass_by_value(t));
    }
}