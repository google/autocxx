// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use syn::{parse_quote, Ident, Item};

use crate::types::{make_ident, Namespace, QualifiedName};

pub(super) fn generate_cxx_use_stmt(name: &QualifiedName, alias: Option<&Ident>) -> Item {
    let segs = find_output_mod_root(name.get_namespace())
        .chain(std::iter::once(make_ident("cxxbridge")))
        .chain(std::iter::once(name.get_final_ident()));
    Item::Use(match alias {
        None => parse_quote! {
            pub use #(#segs)::*;
        },
        Some(alias) => parse_quote! {
            pub use #(#segs)::* as #alias;
        },
    })
}

pub(super) fn find_output_mod_root(ns: &Namespace) -> impl Iterator<Item = crate::minisyn::Ident> {
    std::iter::repeat(make_ident("super")).take(ns.depth())
}
