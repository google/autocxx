// Copyright 2021 Google LLC
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

use std::collections::{HashMap, HashSet};

use syn::parse_quote;

use crate::conversion::analysis::fun::ReceiverMutability;
use crate::conversion::analysis::pod::PodAnalysis;
use crate::conversion::api::{RustSubclassFnDetails, SubclassName};
use crate::{
    conversion::{
        analysis::fun::{
            function_wrapper::{
                CppFunction, CppFunctionBody, CppFunctionKind, TypeConversionPolicy,
            },
            ArgumentAnalysis,
        },
        api::{Api, ApiName},
    },
    types::{make_ident, Namespace, QualifiedName},
};

use super::{FnAnalysis, FnKind, MethodKind};

pub(super) fn subclasses_by_superclass(
    apis: &[Api<PodAnalysis>],
) -> HashMap<QualifiedName, Vec<SubclassName>> {
    let mut subclasses_per_superclass: HashMap<QualifiedName, Vec<SubclassName>> = HashMap::new();

    for api in apis.iter() {
        if let Api::Subclass { name, superclass } = api {
            subclasses_per_superclass
                .entry(superclass.clone())
                .or_default()
                .push(name.clone());
        }
    }
    subclasses_per_superclass
}

pub(super) fn create_subclass_function(
    sub: &SubclassName,
    analysis: &super::FnAnalysisBody,
    name: &ApiName,
    receiver_mutability: &ReceiverMutability,
    superclass: &QualifiedName,
) -> Api<FnAnalysis> {
    let cpp = sub.cpp();
    let holder_name = sub.holder();
    let rust_call_name = make_ident(format!(
        "{}_{}",
        sub.0.name.get_final_item(),
        name.name.get_final_item()
    ));
    let params = std::iter::once(parse_quote! {
        me: & #holder_name
    })
    .chain(analysis.params.iter().skip(1).cloned())
    .collect();
    let kind = if matches!(receiver_mutability, ReceiverMutability::Mutable) {
        CppFunctionKind::Method
    } else {
        CppFunctionKind::ConstMethod
    };
    let super_fn_api_name = QualifiedName::new(
        &Namespace::new(),
        SubclassName::get_super_fn_name(&analysis.rust_name.to_string()),
    );
    let subclass_function: Api<FnAnalysis> = Api::RustSubclassFn {
        name: ApiName::new_in_root_namespace(rust_call_name.clone()),
        subclass: sub.clone(),
        details: Box::new(RustSubclassFnDetails {
            params,
            ret: analysis.ret_type.clone(),
            method_name: make_ident(&analysis.rust_name),
            cpp_impl: CppFunction {
                payload: CppFunctionBody::FunctionCall(Namespace::new(), rust_call_name),
                wrapper_function_name: name.name.get_final_ident(),
                return_conversion: analysis.ret_conversion.clone(),
                argument_conversion: analysis
                    .param_details
                    .iter()
                    .skip(1)
                    .map(|p| p.conversion.clone())
                    .collect(),
                kind,
                pass_obs_field: true,
                qualification: Some(cpp),
            },
            superclass: superclass.clone(),
            receiver_mutability: receiver_mutability.clone(),
            super_fn_api_name,
        }),
    };
    subclass_function
}

pub(super) fn add_subclass_constructor(
    sub: &SubclassName,
    new_apis: &mut Vec<Api<FnAnalysis>>,
    analysis: &super::FnAnalysisBody,
    fun: &crate::conversion::api::FuncToConvert,
) {
    let holder_name = sub.holder();
    let cpp = sub.cpp();
    let cpp_impl = CppFunction {
        payload: CppFunctionBody::AssignSubclassHolderField,
        wrapper_function_name: cpp.clone(),
        return_conversion: None,
        argument_conversion: vec![TypeConversionPolicy::new_unconverted(parse_quote! {
            rust::Box< #holder_name >
        })],
        kind: CppFunctionKind::Constructor,
        pass_obs_field: false,
        qualification: Some(cpp.clone()),
    };
    new_apis.push(Api::RustSubclassConstructor {
        name: ApiName::new_in_root_namespace(cpp.clone()),
        subclass: sub.clone(),
        cpp_impl: Box::new(cpp_impl),
    });
    let wrapper_name = make_ident(format!("{}_make_unique", cpp));
    let mut constructor_wrapper = analysis.clone();
    let id = sub.0.name.get_final_ident();
    constructor_wrapper.param_details.insert(
        0,
        ArgumentAnalysis {
            conversion: TypeConversionPolicy::box_up_subclass_holder(
                parse_quote! {
                    autocxx::subclass::CppSubclassRustPeerHolder<super::super::super:: #id>
                },
                holder_name.clone(),
            ),
            name: parse_quote! {rs_peer},
            self_type: None,
            was_reference: false,
            deps: HashSet::new(),
            is_virtual: false,
            requires_unsafe: false,
        },
    );
    constructor_wrapper
        .params
        .insert(0, parse_quote! { rs_peer: Box<#holder_name> });
    constructor_wrapper.cxxbridge_name = wrapper_name.clone();
    constructor_wrapper.ret_type = parse_quote! { -> cxx::UniquePtr < #cpp > };
    constructor_wrapper.kind = FnKind::Method(
        QualifiedName::new(&Namespace::new(), cpp.clone()),
        MethodKind::Static,
    );
    constructor_wrapper.cpp_wrapper = Some(CppFunction {
        payload: CppFunctionBody::Constructor,
        wrapper_function_name: wrapper_name.clone(),
        return_conversion: Some(TypeConversionPolicy::new_to_unique_ptr(
            parse_quote! { #cpp },
        )),
        argument_conversion: vec![TypeConversionPolicy::new_unconverted(parse_quote! {
            rust::Box< #holder_name >
        })],
        kind: CppFunctionKind::Function,
        pass_obs_field: false,
        qualification: None,
    });
    new_apis.push(Api::Function {
        name: ApiName::new_in_root_namespace(wrapper_name),
        name_for_gc: Some(sub.0.name.clone()),
        analysis: constructor_wrapper,
        fun: Box::new(fun.clone()),
    })
}
