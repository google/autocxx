// Copyright 2022 Google LLC
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

//! Code to create functions to alloc and free while unitialized.

use syn::{parse_quote, punctuated::Punctuated, token::Comma, FnArg, ReturnType};

use crate::{
    conversion::api::{
        Api, ApiName, CppVisibility, FuncToConvert, Provenance, References, TraitSynthesis,
    },
    types::{make_ident, QualifiedName},
};

use super::{
    fun::function_wrapper::{CppFunctionBody, CppFunctionKind},
    pod::PodPhase,
};

pub(crate) fn create_alloc_and_frees(apis: Vec<Api<PodPhase>>) -> Vec<Api<PodPhase>> {
    apis.into_iter()
        .flat_map(|api| -> Box<dyn Iterator<Item = Api<PodPhase>>> {
            match &api {
                Api::Struct { name, .. } => {
                    Box::new(create_alloc_and_free(name.name.clone()).chain(std::iter::once(api)))
                }
                Api::Subclass { name, .. } => {
                    Box::new(create_alloc_and_free(name.cpp()).chain(std::iter::once(api)))
                }
                _ => Box::new(std::iter::once(api)),
            }
        })
        .collect()
}

fn create_alloc_and_free(ty_name: QualifiedName) -> impl Iterator<Item = Api<PodPhase>> {
    let typ = ty_name.to_type_path();
    let free_inputs: Punctuated<FnArg, Comma> = parse_quote! {
        arg0: *mut #typ
    };
    let alloc_return: ReturnType = parse_quote! {
        -> *mut #typ
    };
    [
        (
            TraitSynthesis::AllocUninitialized(ty_name.clone()),
            get_alloc_name(&ty_name),
            Punctuated::new(),
            alloc_return,
            CppFunctionBody::AllocUninitialized(ty_name.clone()),
        ),
        (
            TraitSynthesis::FreeUninitialized(ty_name.clone()),
            get_free_name(&ty_name),
            free_inputs,
            ReturnType::Default,
            CppFunctionBody::FreeUninitialized(ty_name.clone()),
        ),
    ]
    .into_iter()
    .map(
        move |(synthesis, name, inputs, output, cpp_function_body)| {
            let ident = name.get_final_ident();
            let api_name = ApiName::new_from_qualified_name(name);
            Api::Function {
                name: api_name,
                name_for_gc: None,
                fun: Box::new(FuncToConvert {
                    ident,
                    doc_attr: None,
                    inputs,
                    output,
                    vis: parse_quote! { pub },
                    virtualness: crate::conversion::api::Virtualness::None,
                    cpp_vis: CppVisibility::Public,
                    special_member: None,
                    unused_template_param: false,
                    references: References::default(),
                    original_name: None,
                    self_ty: None,
                    synthesized_this_type: None,
                    synthetic_cpp: Some((cpp_function_body, CppFunctionKind::Function)),
                    add_to_trait: Some(synthesis),
                    is_deleted: false,
                    provenance: Provenance::SynthesizedOther,
                }),
                analysis: (),
            }
        },
    )
}

pub(crate) fn get_alloc_name(ty_name: &QualifiedName) -> QualifiedName {
    get_name(ty_name, "alloc")
}

pub(crate) fn get_free_name(ty_name: &QualifiedName) -> QualifiedName {
    get_name(ty_name, "free")
}

fn get_name(ty_name: &QualifiedName, label: &str) -> QualifiedName {
    let name = format!("{}_{}", ty_name.get_final_item(), label);
    let name_id = make_ident(name);
    QualifiedName::new(ty_name.get_namespace(), name_id)
}
