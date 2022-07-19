// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::collections::HashSet;

use syn::{parse_quote, Field, FnArg, Visibility};

use crate::{
    conversion::{
        api::{
            Api, ApiName, FuncToConvert, NullPhase, Provenance, SpecialMemberKind, StructDetails,
        },
        apivec::ApiVec,
        error_reporter::convert_apis,
    },
    types::{make_ident, QualifiedName},
};

use super::fun::function_wrapper::{CppFunctionBody, CppFunctionKind};

pub(crate) fn add_field_accessors(apis: ApiVec<NullPhase>) -> ApiVec<NullPhase> {
    let existing_api: HashSet<QualifiedName> = apis.iter().map(|api| api.name().clone()).collect();

    let mut results = ApiVec::new();
    convert_apis(
        apis,
        &mut results,
        Api::fun_unchanged,
        |struct_name: ApiName, details: Box<StructDetails>, analysis: ()| {
            let mut accessors = ApiVec::new();

            match &details.item.fields {
                syn::Fields::Named(named) => {
                    for field in named.named.iter() {
                        if !should_generate_accessor(field) {
                            continue;
                        }

                        let field_name = field.ident.as_ref().unwrap().to_string();
                        let accessor_name = get_accessor_name(&struct_name.name, &field_name);
                        let ident = accessor_name.get_final_ident();

                        // Don't generate accessors that would conflict with existing api (i.e., if a method with the name we would generate already exists)
                        // TOOD: it would be nicer for this to be in should_generate_accessor
                        if existing_api.contains(&accessor_name) {
                            continue;
                        }

                        let body = CppFunctionBody::ReturnFieldAccess(make_ident(field_name));

                        let struct_type = struct_name.name.to_type_path();

                        let fnarg: FnArg = parse_quote! {
                            this: *const #struct_type
                        };

                        // TODO: convert arrays to pointers?
                        let field_type = &field.ty;

                        accessors.push(Api::Function {
                            name: ApiName::new_from_qualified_name(accessor_name),
                            fun: Box::new(FuncToConvert {
                                provenance: Provenance::SynthesizedOther,
                                ident,
                                doc_attrs: Vec::new(),
                                inputs: [fnarg].into_iter().collect(),
                                variadic: false,
                                output: parse_quote! {
                                    -> #field_type
                                },
                                vis: parse_quote! { pub },
                                virtualness: crate::conversion::api::Virtualness::None,
                                cpp_vis: crate::conversion::api::CppVisibility::Public,
                                special_member: Some(SpecialMemberKind::GeneratedAccessor),
                                unused_template_param: false,
                                references: Default::default(),
                                original_name: None,
                                self_ty: None,
                                synthesized_this_type: None,
                                add_to_trait: None,
                                synthetic_cpp: Some((body, CppFunctionKind::Function)),
                                is_deleted: false,
                            }),
                            analysis: (),
                        })
                    }
                }
                syn::Fields::Unnamed(_) => {}
                syn::Fields::Unit => {}
            };

            // Generate the accessors (if any) + the struct itself
            Ok(Box::new(accessors.into_iter().chain(std::iter::once(
                Api::Struct {
                    name: struct_name,
                    details,
                    analysis,
                },
            ))))
        },
        Api::enum_unchanged,
        Api::typedef_unchanged,
    );

    results
}

fn should_generate_accessor(field: &Field) -> bool {
    // Don't generate accessors for non-public fields
    if !matches!(field.vis, Visibility::Public(_)) {
        return false;
    }

    // Don't generate accessors for cpp reference fields (this restriction may be lifted in the future)
    if field.attrs.iter().any(is_cpp_reference) {
        return false;
    }

    // Don't generate accessors for "fake" bindgen fields which wouldn't appear directly in the C++ struct
    let field_name = field.ident.as_ref().unwrap().to_string();
    if field_name == "vtable_" || field_name == "_address" || field_name == "_base" {
        return false;
    }

    return true;
}

fn is_cpp_reference(attr: &syn::Attribute) -> bool {
    return *attr == parse_quote! { #[cpp_semantics(reference)] }
        || *attr == parse_quote! { #[cpp_semantics(rvalue_reference)] };
}

fn get_accessor_name(struct_name: &QualifiedName, field_name: &str) -> QualifiedName {
    let accessor_name = format!("{}_get_{}", struct_name.get_final_item(), field_name);

    QualifiedName::new(struct_name.get_namespace(), make_ident(accessor_name))
}
