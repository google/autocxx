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

use crate::types::{validate_ident_ok_for_rust, QualifiedName};

use self::fun::FnAnalysis;

use super::{
    api::{Api, ApiDetail},
    error_reporter::convert_item_apis,
    ConvertError,
};

pub(crate) mod abstract_types;
pub(crate) mod ctypes;
pub(crate) mod fun;
pub(crate) mod gc;
pub(crate) mod pod; // hey, that rhymes
pub(crate) mod remove_ignored;
pub(crate) mod tdef;
mod type_converter;

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

/// If any items have names which can't be represented by cxx,
/// abort.
pub(crate) fn check_names(apis: Vec<Api<FnAnalysis>>) -> Vec<Api<FnAnalysis>> {
    let mut results = Vec::new();
    convert_item_apis(apis, &mut results, |api| match api.detail {
        ApiDetail::Typedef { .. }
        | ApiDetail::ForwardDeclaration
        | ApiDetail::Const { .. }
        | ApiDetail::Enum { .. }
        | ApiDetail::Struct { .. } => {
            let cxx_name = api
                .original_name
                .as_ref()
                .map(|s| {
                    // Occasionally we might have outer_type::inner_type.
                    // We'll just check that inner_type is acceptable cxx spelling.
                    let parsed = QualifiedName::new_from_cpp_name(&s);
                    parsed.get_final_item().to_string()
                })
                .unwrap_or(api.name.get_final_item().to_string());
            validate_ident_ok_for_rust(&cxx_name).map(|_| Some(api))
        }
        ApiDetail::Function { .. } // we don't handle functions here because
            // the function analysis does an equivalent check. Instead of just rejecting
            // the function, it creates a wrapper function instead with a more
            // palatable name. That's preferable to rejecting the API entirely.
        | ApiDetail::ConcreteType  { .. }
        | ApiDetail::CType { .. }
        | ApiDetail::StringConstructor
        | ApiDetail::IgnoredItem { .. } => Ok(Some(api)),
    });
    results
}
