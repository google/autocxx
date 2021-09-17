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

use std::collections::HashMap;

use proc_macro2::Ident;
use syn::parse_quote;

use crate::conversion::analysis::fun::ReceiverMutability;
use crate::conversion::analysis::pod::PodAnalysis;
use crate::conversion::api::{RustSubclassFnDetails, SubclassName};
use crate::{
    conversion::{
        analysis::fun::function_wrapper::{
            CppFunction, CppFunctionBody, CppFunctionKind, TypeConversionPolicy,
        },
        api::{Api, ApiName},
    },
    types::{make_ident, Namespace, QualifiedName},
};

use super::FnAnalysis;

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

pub(super) fn create_subclass_constructor(
    sub: &SubclassName,
    analysis: &super::FnAnalysisBody,
    superclass_name: Ident,
) -> Api<FnAnalysis> {
    let holder_name = sub.holder();
    let cpp = sub.cpp();
    let argument_conversion =
        std::iter::once(TypeConversionPolicy::new_unconverted(parse_quote! {
            rust::Box< #holder_name >
        }))
        .chain(analysis.param_details.iter().map(|p| p.conversion.clone()))
        .collect();
    let cpp_impl = CppFunction {
        payload: CppFunctionBody::ConstructSuperclass(superclass_name),
        wrapper_function_name: cpp.clone(),
        return_conversion: None,
        argument_conversion,
        kind: CppFunctionKind::Constructor,
        pass_obs_field: false,
        qualification: Some(cpp.clone()),
    };
    Api::RustSubclassConstructor {
        name: ApiName::new_in_root_namespace(cpp.clone()),
        subclass: sub.clone(),
        cpp_impl: Box::new(cpp_impl),
    }
}
