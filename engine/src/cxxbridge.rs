// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt};
use syn::ItemMod;

/// A struct to represent a cxx::bridge (i.e. some manual bindings)
/// found in a file. autocxx knows about them so that we can generate C++
/// for both manual and automatic bindings using the same tooling.
pub struct CxxBridge {
    tokens: TokenStream,
}

impl From<ItemMod> for CxxBridge {
    fn from(itm: ItemMod) -> Self {
        Self {
            tokens: itm.to_token_stream(),
        }
    }
}

impl ToTokens for CxxBridge {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_all(self.tokens.clone());
    }
}
