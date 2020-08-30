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

/// Substitutes given idents in a `TokenStream2`.
/// It also replaces 'const *' with '&' and '*' with '& mut'
pub(crate) struct IdentReplacer<'a, 'b> {
    replacements: &'a HashMap<&'b str, String>,
}

impl<'a, 'b> IdentReplacer<'a, 'b> {
    pub fn new(replacements: &'a HashMap<&'b str, String>) -> Self {
        Self {
            replacements
        }
    }

    /// Replace certain `Ident`s in a `TokenStream2`.
    pub(crate) fn replace_in_tokenstream(&self, bindings: TokenStream2) -> TokenStream2 {
        let mut new_ts = TokenStream2::new();
        for t in bindings {
            let replacement = match t {
                TokenTree2::Ident(i) if i.to_string() == "const" => None,
                TokenTree2::Ident(i) => {
                    let name = i.to_string();
                    let e = self.replacements.get(name.as_str());
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
                    let replacement_tokens = self.replace_in_tokenstream(g.stream());
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