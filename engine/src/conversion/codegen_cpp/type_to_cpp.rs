// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::{
    conversion::{apivec::ApiVec, AnalysisPhase, ConvertErrorFromCpp},
    types::QualifiedName,
};
use indexmap::map::IndexMap as HashMap;
use itertools::Itertools;
use quote::ToTokens;
use std::iter::once;
use syn::{Token, Type};

/// Map from QualifiedName to original C++ name. Original C++ name does not
/// include the namespace; this can be assumed to be the same as the namespace
/// in the QualifiedName.
/// The "original C++ name" is mostly relevant in the case of nested types,
/// where the typename might be A::B within a namespace C::D.
pub(crate) struct CppNameMap(HashMap<QualifiedName, String>);

impl CppNameMap {
    /// Look through the APIs we've found to assemble the original name
    /// map.
    pub(crate) fn new_from_apis<T: AnalysisPhase>(apis: &ApiVec<T>) -> Self {
        Self(
            apis.iter()
                .filter_map(|api| {
                    api.cpp_name()
                        .as_ref()
                        .map(|cpp_name| (api.name().clone(), cpp_name.clone()))
                })
                .collect(),
        )
    }

    /// Imagine a nested struct in namespace::outer::inner
    /// This function converts from the bindgen name, namespace::outer_inner,
    /// to namespace::outer::inner.
    pub(crate) fn map(&self, qual_name: &QualifiedName) -> String {
        if let Some(cpp_name) = self.0.get(qual_name) {
            qual_name
                .get_namespace()
                .iter()
                .chain(once(cpp_name))
                .join("::")
        } else {
            qual_name.to_cpp_name()
        }
    }

    /// Get a stringified version of the last ident in the name.
    /// e.g. for namespace::outer_inner this will return inner.
    /// This is useful for doing things such as calling constructors
    /// such as inner() or destructors such as ~inner()
    pub(crate) fn get_final_item<'b>(&'b self, qual_name: &'b QualifiedName) -> &'b str {
        match self.get(qual_name) {
            Some(n) => match n.rsplit_once("::") {
                Some((_, suffix)) => suffix,
                None => qual_name.get_final_item(),
            },
            None => qual_name.get_final_item(),
        }
    }

    /// Convert a type to its C++ spelling.
    pub(crate) fn type_to_cpp(&self, ty: &Type) -> Result<String, ConvertErrorFromCpp> {
        match ty {
            Type::Path(typ) => {
                // If this is a std::unique_ptr we do need to pass
                // its argument through.
                let qual_name = QualifiedName::from_type_path(typ);
                let root = self.map(&qual_name);
                if root == "Pin" {
                    // Strip all Pins from type names when describing them in C++.
                    let inner_type = &typ.path.segments.last().unwrap().arguments;
                    if let syn::PathArguments::AngleBracketed(ab) = inner_type {
                        let inner_type = ab.args.iter().next().unwrap();
                        if let syn::GenericArgument::Type(gat) = inner_type {
                            return self.type_to_cpp(gat);
                        }
                    }
                    panic!("Pin<...> didn't contain the inner types we expected");
                }
                let suffix = match &typ.path.segments.last().unwrap().arguments {
                    syn::PathArguments::AngleBracketed(ab) => {
                        let results: Result<Vec<_>, _> = ab
                            .args
                            .iter()
                            .map(|x| match x {
                                syn::GenericArgument::Type(gat) => self.type_to_cpp(gat),
                                _ => Ok("".to_string()),
                            })
                            .collect();
                        Some(results?.join(", "))
                    }
                    syn::PathArguments::None | syn::PathArguments::Parenthesized(_) => None,
                };
                match suffix {
                    None => Ok(root),
                    Some(suffix) => Ok(format!("{root}<{suffix}>")),
                }
            }
            Type::Reference(typr) => match &*typr.elem {
                Type::Path(typ) if typ.path.is_ident("str") => Ok("rust::Str".into()),
                _ => Ok(format!(
                    "{}{}&",
                    get_mut_string(&typr.mutability),
                    self.type_to_cpp(typr.elem.as_ref())?
                )),
            },
            Type::Ptr(typp) => Ok(format!(
                "{}{}*",
                get_mut_string(&typp.mutability),
                self.type_to_cpp(typp.elem.as_ref())?
            )),
            Type::Array(_)
            | Type::BareFn(_)
            | Type::Group(_)
            | Type::ImplTrait(_)
            | Type::Infer(_)
            | Type::Macro(_)
            | Type::Never(_)
            | Type::Paren(_)
            | Type::Slice(_)
            | Type::TraitObject(_)
            | Type::Tuple(_)
            | Type::Verbatim(_) => Err(ConvertErrorFromCpp::UnsupportedType(
                ty.to_token_stream().to_string(),
            )),
            _ => Err(ConvertErrorFromCpp::UnknownType(
                ty.to_token_stream().to_string(),
            )),
        }
    }

    /// Check an individual item in the name map. Returns a thing if
    /// it's an inner type, otherwise returns none.
    pub(crate) fn get(&self, name: &QualifiedName) -> Option<&String> {
        self.0.get(name)
    }
}

fn get_mut_string(mutability: &Option<Token![mut]>) -> &'static str {
    match mutability {
        None => "const ",
        Some(_) => "",
    }
}
