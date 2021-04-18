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

mod config;
pub mod file_locations;
mod type_config;

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

pub use config::{IncludeCppConfig, UnsafePolicy};
use file_locations::FileLocationStrategy;
use proc_macro2::TokenStream as TokenStream2;
use syn::Result as ParseResult;
use syn::{
    parse::{Parse, ParseStream},
    Macro,
};

pub use type_config::TypeConfig;

/// Core of the autocxx engine. See `generate` for most details
/// on how this works.
pub struct IncludeCpp {
    config: IncludeCppConfig,
}

impl Parse for IncludeCpp {
    fn parse(input: ParseStream) -> ParseResult<Self> {
        let config = input.parse::<IncludeCppConfig>()?;
        Ok(Self { config })
    }
}

impl IncludeCpp {
    pub fn new_from_syn(mac: Macro) -> ParseResult<Self> {
        mac.parse_body::<IncludeCpp>()
    }

    pub fn get_rs_filename(&self) -> String {
        let mut hasher = DefaultHasher::new();
        self.config.hash(&mut hasher);
        let id = hasher.finish();
        format!("{}.rs", id)
    }

    /// Generate the Rust bindings.
    pub fn generate_rs(&self) -> TokenStream2 {
        if self.config.parse_only {
            return TokenStream2::new();
        }
        let fname = self.get_rs_filename();
        FileLocationStrategy::new().make_include(fname)
    }

    pub fn get_config(&self) -> &IncludeCppConfig {
        &self.config
    }
}

#[cfg(test)]
mod parse_tests {
    use crate::IncludeCpp;
    use syn::parse_quote;

    #[test]
    fn test_basic() {
        let _i: IncludeCpp = parse_quote! {};
    }
}
