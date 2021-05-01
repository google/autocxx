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

use proc_macro2::Span;
use syn::{Ident, LitStr, Result as ParseResult};

/// Allowlist configuration.
#[derive(Hash, Debug)]
pub enum Allowlist {
    Unspecified,
    All,
    Specific(Vec<String>),
}

impl Allowlist {
    pub(crate) fn push(&mut self, item: LitStr) -> ParseResult<()> {
        match self {
            Allowlist::Unspecified => {
                *self = Allowlist::Specific(vec![item.value()]);
            }
            Allowlist::All => {
                return Err(syn::Error::new(
                    item.span(),
                    "use either generate!/generate_pod! or generate_all!, not both.",
                ))
            }
            Allowlist::Specific(list) => list.push(item.value()),
        };
        Ok(())
    }

    pub(crate) fn set_all(&mut self, ident: &Ident) -> ParseResult<()> {
        if matches!(self, Allowlist::Specific(..)) {
            return Err(syn::Error::new(
                ident.span(),
                "use either generate!/generate_pod! or generate_all!, not both.",
            ));
        }
        *self = Allowlist::All;
        Ok(())
    }
}

impl Default for Allowlist {
    fn default() -> Self {
        Allowlist::Unspecified
    }
}

#[derive(Default, Hash, Debug)]
pub(crate) struct TypeConfigInput {
    pub(crate) pod_requests: Vec<String>,
    pub(crate) allowlist: Allowlist,
    pub(crate) blocklist: Vec<String>,
    pub(crate) exclude_utilities: bool,
}

/// Configuration about types.
/// At present this is very minimal; in future we should roll
/// known_types.rs into this and possibly other things as well.
/// This type can only be created once we know that the allowlist
/// is specified.
#[derive(Hash, Debug)]
pub struct TypeConfig(TypeConfigInput);

const UTILITIES: &[&str] = &["make_string"];
const NO_UTILITIES: &[&str] = &[];

impl TypeConfigInput {
    pub(crate) fn into_type_config(self) -> ParseResult<TypeConfig> {
        if matches!(self.allowlist, Allowlist::Unspecified) {
            Err(syn::Error::new(
                Span::call_site(),
                "expected generate! or generate_all!",
            ))
        } else {
            Ok(TypeConfig(self))
        }
    }
}

impl TypeConfig {
    pub fn get_pod_requests(&self) -> &[String] {
        &self.0.pod_requests
    }

    /// Whether to avoid generating the standard helpful utility
    /// functions which we normally include in every mod.
    pub fn exclude_utilities(&self) -> bool {
        self.0.exclude_utilities
    }

    /// Items which the user has explicitly asked us to generate;
    /// we should raise an error if we weren't able to do so.
    pub fn must_generate_list(&self) -> Box<dyn Iterator<Item = String> + '_> {
        if let Allowlist::Specific(items) = &self.0.allowlist {
            Box::new(items.iter().chain(self.0.pod_requests.iter()).cloned())
        } else {
            Box::new(self.0.pod_requests.iter().cloned())
        }
    }

    /// The allowlist of items to be passed into bindgen, if any.
    pub fn bindgen_allowlist(&self) -> Option<Box<dyn Iterator<Item = String> + '_>> {
        match &self.0.allowlist {
            Allowlist::All => None,
            Allowlist::Specific(items) => Some(Box::new(
                items
                    .iter()
                    .chain(self.0.pod_requests.iter())
                    .cloned()
                    .chain(self.active_utilities().iter().map(|s| s.to_string())),
            )),
            Allowlist::Unspecified => unreachable!(),
        }
    }

    fn active_utilities(&self) -> &'static [&'static str] {
        if self.0.exclude_utilities {
            NO_UTILITIES
        } else {
            UTILITIES
        }
    }

    /// Whether this type is on the allowlist specified by the user.
    ///
    /// A note on the allowlist handling in general. It's used in two places:
    /// 1) As directives to bindgen
    /// 2) After bindgen has generated code, to filter the APIs which
    ///    we pass to cxx.
    /// This second pass may seem redundant. But sometimes bindgen generates
    /// unnecessary stuff.
    pub fn is_on_allowlist(&self, cpp_name: &str) -> bool {
        match self.bindgen_allowlist() {
            None => true,
            Some(mut items) => {
                items.any(|item| item == cpp_name)
                    || self.active_utilities().iter().any(|item| *item == cpp_name)
            }
        }
    }

    pub fn is_on_blocklist(&self, cpp_name: &str) -> bool {
        self.0.blocklist.contains(&cpp_name.to_string())
    }

    pub fn get_blocklist(&self) -> impl Iterator<Item = &String> {
        self.0.blocklist.iter()
    }

    pub fn new_for_test() -> Self {
        let mut input = TypeConfigInput::default();
        input.allowlist = Allowlist::All;
        input.into_type_config().unwrap()
    }
}
