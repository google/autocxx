// Copyright 2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use syn::{parse_quote};
use proc_macro2::Span;

pub(crate) fn make_doc_attrs(label: String) -> Vec<crate::minisyn::Attribute> {
    let hexathorpe = syn::token::Pound(Span::call_site());
    vec![crate::minisyn::Attribute(parse_quote! {
        #hexathorpe [doc = #label]
    })]
}
