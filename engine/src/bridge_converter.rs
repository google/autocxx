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
use proc_macro2::TokenStream as TokenStream2;
use proc_macro2::TokenTree as TokenTree2;
use quote::TokenStreamExt;
use lazy_static::lazy_static;

lazy_static! {
    /// Substitutions from the names constructed by bindgen into those
    /// which cxx uses.
    static ref IDENT_REPLACEMENTS: HashMap<&'static str, String> = {
        let mut map = HashMap::new();
        map.insert("std_unique_ptr", "UniquePtr".to_string());
        map.insert("std_string", "CxxString".to_string());
        map
    };
}

/// Substitutes given idents in a `TokenStream2`.
/// It also replaces 'const *' with '&' and '*' with '& mut'
pub(crate) struct BridgeConverter {
}

impl BridgeConverter {
    pub fn new() -> Self {
        Self { }
    }

    /// Replace certain `Ident`s in a `TokenStream2`.
    pub(crate) fn convert(&self, bindings: TokenStream2) -> TokenStream2 {
        let mut new_ts = TokenStream2::new();
        for t in bindings {
            let replacement = match t {
                TokenTree2::Ident(i) if i.to_string() == "const" => None,
                TokenTree2::Ident(i) => {
                    let name = i.to_string();
                    let e = IDENT_REPLACEMENTS.get(name.as_str());
                    Some(match e {
                        Some(s) => TokenTree2::Ident(proc_macro2::Ident::new(s, i.span())),
                        None => TokenTree2::Ident(i),
                    })
                }
                TokenTree2::Punct(p) if p.as_char() == '*' => {
                    Some(TokenTree2::Punct(proc_macro2::Punct::new('&', p.spacing())))
                },
                TokenTree2::Punct(p) => Some(TokenTree2::Punct(p)),
                TokenTree2::Literal(l) => Some(TokenTree2::Literal(l)),
                TokenTree2::Group(g) => {
                    let delim = g.delimiter();
                    let replacement_tokens = self.convert(g.stream());
                    Some(TokenTree2::Group(proc_macro2::Group::new(delim, replacement_tokens)))
                }
            };
            if let Some (repl) = replacement {
                new_ts.append(repl);
            }
        }
        new_ts
    }
}