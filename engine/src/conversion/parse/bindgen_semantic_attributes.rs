// Copyright 2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::fmt::Display;

use proc_macro2::{Ident, TokenStream};
use quote::quote;
use syn::{
    parenthesized,
    parse::{Parse, Parser},
    Attribute, LitStr,
};

use crate::{
    conversion::{
        api::{
            CppVisibility, DeletedOrDefaulted, Layout, References, SpecialMemberKind, Virtualness,
        },
        convert_error::{ConvertErrorWithContext, ErrorContext},
        ConvertErrorFromCpp, CppEffectiveName,
    },
    types::QualifiedName,
};

/// Newtype wrapper for a C++ "original name"; that is, an annotation
/// derived from bindgen that this is the original name of the C++ item.
///
/// At present these various newtype wrappers for kinds of names
/// (Rust, C++, cxx::bridge) have various conversions between them that
/// are probably not safe. They're marked with FIXMEs. Over time we should
/// remove them, or make them safe by doing name validation at the point
/// of conversion.
#[derive(PartialEq, PartialOrd, Eq, Hash, Clone, Debug)]
pub struct CppOriginalName(String);

impl Display for CppOriginalName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl CppOriginalName {
    pub(crate) fn is_nested(&self) -> bool {
        self.0.contains("::")
    }

    pub(crate) fn from_final_item_of_pre_existing_qualified_name(name: &QualifiedName) -> Self {
        Self(name.get_final_item().to_string())
    }

    pub(crate) fn to_qualified_name(&self) -> QualifiedName {
        QualifiedName::new_from_cpp_name(&self.0)
    }

    pub(crate) fn to_effective_name(&self) -> CppEffectiveName {
        CppEffectiveName(self.0.clone())
    }

    /// This is the main output of this type; it's fed into a mapping
    /// from <weird bindgen name format> to
    /// <sensible namespace::outer::inner format>; this contributes "inner".
    pub(crate) fn for_original_name_map(&self) -> &str {
        &self.0
    }

    /// Used to give the final part of the name which can be used
    /// to figure out the name for constructors, destructors etc.
    pub(crate) fn get_final_segment_for_special_members(&self) -> Option<&str> {
        self.0.rsplit_once("::").map(|(_, suffix)| suffix)
    }

    pub(crate) fn from_type_name_for_constructor(name: String) -> Self {
        Self(name)
    }

    /// Work out what to call a Rust-side API given a C++-side name.
    pub(crate) fn to_string_for_rust_name(&self) -> String {
        self.0.clone()
    }

    /// Return the string inside for validation purposes.
    pub(crate) fn for_validation(&self) -> &str {
        &self.0
    }

    /// Used for diagnostics early in function analysis before we establish
    /// the correct naming.
    pub(crate) fn diagnostic_display_name(&self) -> &String {
        &self.0
    }

    // FIXME - remove
    pub(crate) fn from_rust_name(string: String) -> Self {
        Self(string)
    }

    /// Determines whether we need to generate a cxxbridge::name attribute
    pub(crate) fn does_not_match_cxxbridge_name(
        &self,
        cxxbridge_name: &crate::minisyn::Ident,
    ) -> bool {
        cxxbridge_name.0.to_string() != self.0
    }

    pub(crate) fn generate_cxxbridge_name_attribute(&self) -> proc_macro2::TokenStream {
        let cpp_call_name = &self.to_string_for_rust_name();
        quote!(
            #[cxx_name = #cpp_call_name]
        )
    }
}

/// The set of all annotations that autocxx_bindgen has added
/// for our benefit.
#[derive(Debug)]
pub(crate) struct BindgenSemanticAttributes(Vec<BindgenSemanticAttribute>);

impl BindgenSemanticAttributes {
    // Remove `bindgen_` attributes. They don't have a corresponding macro defined anywhere,
    // so they will cause compilation errors if we leave them in.
    // We may return an error if one of the bindgen attributes shows that the
    // item can't be processed.
    pub(crate) fn new_retaining_others(attrs: &mut Vec<Attribute>) -> Self {
        let metadata = Self::new(attrs);
        attrs.retain(|a| a.path().segments.last().unwrap().ident != "cpp_semantics");
        metadata
    }

    pub(crate) fn new(attrs: &[Attribute]) -> Self {
        Self(
            attrs
                .iter()
                .filter_map(|attr| {
                    if attr.path().segments.last().unwrap().ident == "cpp_semantics" {
                        let r: Result<BindgenSemanticAttribute, syn::Error> = attr.parse_args();
                        r.ok()
                    } else {
                        None
                    }
                })
                .collect(),
        )
    }

    /// Some attributes indicate we can never handle a given item. Check for those.
    pub(crate) fn check_for_fatal_attrs(
        &self,
        id_for_context: &Ident,
    ) -> Result<(), ConvertErrorWithContext> {
        if self.has_attr("unused_template_param") {
            Err(ConvertErrorWithContext(
                ConvertErrorFromCpp::UnusedTemplateParam,
                Some(ErrorContext::new_for_item(id_for_context.clone().into())),
            ))
        } else if self.get_cpp_visibility() != CppVisibility::Public {
            Err(ConvertErrorWithContext(
                ConvertErrorFromCpp::NonPublicNestedType,
                Some(ErrorContext::new_for_item(id_for_context.clone().into())),
            ))
        } else {
            Ok(())
        }
    }

    /// Whether the given attribute is present.
    pub(super) fn has_attr(&self, attr_name: &str) -> bool {
        self.0.iter().any(|a| a.is_ident(attr_name))
    }

    /// The C++ visibility of the item.
    pub(super) fn get_cpp_visibility(&self) -> CppVisibility {
        if self.has_attr("visibility_private") {
            CppVisibility::Private
        } else if self.has_attr("visibility_protected") {
            CppVisibility::Protected
        } else {
            CppVisibility::Public
        }
    }

    /// Whether the item is virtual.
    pub(super) fn get_virtualness(&self) -> Virtualness {
        if self.has_attr("pure_virtual") {
            Virtualness::PureVirtual
        } else if self.has_attr("bindgen_virtual") {
            Virtualness::Virtual
        } else {
            Virtualness::None
        }
    }

    pub(super) fn get_deleted_or_defaulted(&self) -> DeletedOrDefaulted {
        if self.has_attr("deleted") {
            DeletedOrDefaulted::Deleted
        } else if self.has_attr("defaulted") {
            DeletedOrDefaulted::Defaulted
        } else {
            DeletedOrDefaulted::Neither
        }
    }

    fn parse_if_present<T: Parse>(&self, annotation: &str) -> Option<T> {
        self.0
            .iter()
            .find(|a| a.is_ident(annotation))
            .map(|a| a.parse_args().unwrap())
    }

    fn string_if_present(&self, annotation: &str) -> Option<String> {
        let ls: Option<LitStr> = self.parse_if_present(annotation);
        ls.map(|ls| ls.value())
    }

    /// The in-memory layout of the item.
    pub(super) fn get_layout(&self) -> Option<Layout> {
        self.parse_if_present("layout")
    }

    /// The original C++ name, which bindgen may have changed.
    pub(super) fn get_original_name(&self) -> Option<CppOriginalName> {
        self.string_if_present("original_name").map(CppOriginalName)
    }

    /// Whether this is a move constructor or other special member.
    pub(super) fn special_member_kind(&self) -> Option<SpecialMemberKind> {
        self.string_if_present("special_member")
            .map(|kind| match kind.as_str() {
                "default_ctor" => SpecialMemberKind::DefaultConstructor,
                "copy_ctor" => SpecialMemberKind::CopyConstructor,
                "move_ctor" => SpecialMemberKind::MoveConstructor,
                "dtor" => SpecialMemberKind::Destructor,
                "assignment_operator" => SpecialMemberKind::AssignmentOperator,
                _ => panic!("unexpected special_member_kind"),
            })
    }

    /// Any reference parameters or return values.
    pub(super) fn get_reference_parameters_and_return(&self) -> References {
        let mut results = References::default();
        for a in &self.0 {
            if a.is_ident("ret_type_reference") {
                results.ref_return = true;
            } else if a.is_ident("ret_type_rvalue_reference") {
                results.rvalue_ref_return = true;
            } else if a.is_ident("arg_type_reference") {
                let r: Result<Ident, syn::Error> = a.parse_args();
                if let Ok(ls) = r {
                    results.ref_params.insert(ls.into());
                }
            } else if a.is_ident("arg_type_rvalue_reference") {
                let r: Result<Ident, syn::Error> = a.parse_args();
                if let Ok(ls) = r {
                    results.rvalue_ref_params.insert(ls.into());
                }
            }
        }
        results
    }
}

#[derive(Debug)]
struct BindgenSemanticAttribute {
    annotation_name: Ident,
    body: Option<TokenStream>,
}

impl BindgenSemanticAttribute {
    fn is_ident(&self, name: &str) -> bool {
        self.annotation_name == name
    }

    fn parse_args<T: Parse>(&self) -> Result<T, syn::Error> {
        T::parse.parse2(self.body.as_ref().unwrap().clone())
    }
}

impl Parse for BindgenSemanticAttribute {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let annotation_name: Ident = input.parse()?;
        if input.peek(syn::token::Paren) {
            let body_contents;
            parenthesized!(body_contents in input);
            Ok(Self {
                annotation_name,
                body: Some(body_contents.parse()?),
            })
        } else if !input.is_empty() {
            Err(input.error("expected nothing"))
        } else {
            Ok(Self {
                annotation_name,
                body: None,
            })
        }
    }
}
