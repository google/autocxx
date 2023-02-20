// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use syn::Attribute;

/// Returns the attribute (if any) which contains a doc comment.
pub(super) fn get_doc_attrs(attrs: &[Attribute]) -> Vec<Attribute> {
    get_doc_attrs_internal(attrs)
        .collect()
}

pub(super) fn get_doc_attrs_as_minisyn(attrs: &[Attribute]) -> Vec<crate::minisyn::Attribute> {
    get_doc_attrs_internal(attrs)
        .map(crate::minisyn::Attribute)
        .collect()
}

fn get_doc_attrs_internal(attrs: &[Attribute]) -> impl Iterator<Item=Attribute> + '_ {
    attrs
    .iter()
    .filter(|a| a.path.get_ident().iter().any(|p| *p == "doc"))
    .cloned()
}