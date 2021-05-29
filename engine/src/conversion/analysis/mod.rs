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

use syn::Attribute;

use super::ConvertError;

pub(crate) mod abstract_types;
mod bridge_name_tracker;
pub(crate) mod ctypes;
pub(crate) mod fun;
pub(crate) mod gc;
mod name_check;
pub(crate) mod pod; // hey, that rhymes
pub(crate) mod remove_ignored;
pub(crate) mod tdef;
mod type_converter;

pub(crate) use name_check::check_names;

// Remove `bindgen_` attributes. They don't have a corresponding macro defined anywhere,
// so they will cause compilation errors if we leave them in.
// We may return an error if one of the bindgen attributes shows that the
// item can't be processed.
fn remove_bindgen_attrs(attrs: &mut Vec<Attribute>) -> Result<(), ConvertError> {
    if has_attr(&attrs, "bindgen_unused_template_param") {
        return Err(ConvertError::UnusedTemplateParam);
    }

    fn is_bindgen_attr(attr: &Attribute) -> bool {
        let segments = &attr.path.segments;
        segments.len() == 1
            && segments
                .first()
                .unwrap()
                .ident
                .to_string()
                .starts_with("bindgen_")
    }

    attrs.retain(|a| !is_bindgen_attr(a));
    Ok(())
}

fn has_attr(attrs: &[Attribute], attr_name: &str) -> bool {
    attrs.iter().any(|at| at.path.is_ident(attr_name))
}
