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

use bindgen::callbacks::{IntKind, ParseCallbacks};
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Mutex;

/// Keeps track of all known preprocessor invocations.
#[derive(Debug, Default)]
pub(crate) struct PreprocessorDefinitions {
    integral: HashMap<String, i64>,
}

impl PreprocessorDefinitions {
    pub fn new() -> Self {
        PreprocessorDefinitions {
            integral: HashMap::new(),
        }
    }

    fn insert_int_macro(&mut self, name: &str, val: i64) {
        self.integral.insert(name.to_string(), val);
    }

    pub fn to_tokenstream(&self) -> TokenStream2 {
        if self.integral.is_empty() {
            TokenStream2::new()
        } else {
            let defs = self.integral.iter().map(|(k, v)| {
                quote! {
                    const #k = #v;
                }
            });
            quote! {
                mod ffi {
                    mod cpp {
                        #(#defs)*
                    }
                }
            }
        }
    }
}

/// Callbacks for bindgen.
#[derive(Debug)]
pub(crate) struct PreprocessorParseCallbacks {
    // We use a mutex rather than a RefCell not because we need thread
    // safety, but because we need poisoning in order to avoid problems
    // with UnwindSafe.
    definitions: Rc<Mutex<PreprocessorDefinitions>>,
}

impl PreprocessorParseCallbacks {
    pub fn new(definitions: Rc<Mutex<PreprocessorDefinitions>>) -> Self {
        PreprocessorParseCallbacks { definitions }
    }
}

impl ParseCallbacks for PreprocessorParseCallbacks {
    fn int_macro(&self, name: &str, value: i64) -> Option<IntKind> {
        let mut mg = self
            .definitions
            .try_lock()
            .expect("would block whilst adding int macro");
        mg.insert_int_macro(name, value);
        None
    }
}
