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

use syn::{parse_quote, FnArg, PatType, Type, TypePtr};

use crate::conversion::analysis::fun::{FnKind, MethodKind, ReceiverMutability};
use crate::conversion::analysis::pod::PodPhase;
use crate::conversion::api::{CppVisibility, FuncToConvert, RustSubclassFnDetails, SubclassName};
use crate::{
    conversion::{
        analysis::fun::function_wrapper::{
            CppFunction, CppFunctionBody, CppFunctionKind, TypeConversionPolicy,
        },
        api::{Api, ApiName},
    },
    types::{make_ident, Namespace, QualifiedName},
};

use super::FnPhase;

pub(super) fn subclasses_by_superclass(
    apis: &[Api<PodPhase>],
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

pub(super) fn create_subclass_fn_wrapper(
    sub: SubclassName,
    super_fn_name: &QualifiedName,
    fun: &FuncToConvert,
) -> Box<FuncToConvert> {
    let self_ty = Some(sub.cpp());
    Box::new(FuncToConvert {
        virtual_this_type: self_ty.clone(),
        self_ty,
        ident: super_fn_name.get_final_ident(),
        doc_attr: fun.doc_attr.clone(),
        inputs: fun.inputs.clone(),
        output: fun.output.clone(),
        vis: fun.vis.clone(),
        is_pure_virtual: false,
        cpp_vis: CppVisibility::Public,
        is_move_constructor: false,
        unused_template_param: fun.unused_template_param,
        original_name: None,
        return_type_is_reference: fun.return_type_is_reference,
        reference_args: fun.reference_args.clone(),
        synthesize_make_unique: fun.synthesize_make_unique,
    })
}

pub(super) fn create_subclass_function(
    sub: &SubclassName,
    analysis: &super::FnAnalysis,
    name: &ApiName,
    receiver_mutability: &ReceiverMutability,
    superclass: &QualifiedName,
    dependency: Option<&QualifiedName>,
) -> Api<FnPhase> {
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
    let subclass_function: Api<FnPhase> = Api::RustSubclassFn {
        name: ApiName::new_in_root_namespace(rust_call_name.clone()),
        subclass: sub.clone(),
        details: Box::new(RustSubclassFnDetails {
            params,
            ret: analysis.ret_type.clone(),
            method_name: make_ident(&analysis.rust_name),
            cpp_impl: CppFunction {
                payload: CppFunctionBody::FunctionCall(Namespace::new(), rust_call_name),
                wrapper_function_name: name.name.get_final_ident(),
                original_cpp_name: name.cpp_name(),
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
            dependency: dependency.cloned(),
            requires_unsafe: analysis.param_details.iter().any(|pd| pd.requires_unsafe),
            is_pure_virtual: matches!(
                analysis.kind,
                FnKind::Method(_, MethodKind::PureVirtual(..))
            ),
        }),
    };
    subclass_function
}

pub(super) fn create_subclass_constructor_wrapper(
    sub: SubclassName,
    fun: &FuncToConvert,
) -> (Box<FuncToConvert>, ApiName) {
    let subclass_constructor_name = make_ident(format!(
        "{}_{}",
        sub.cpp().get_final_item(),
        sub.cpp().get_final_item()
    ));
    let mut existing_params = fun.inputs.clone();
    if let Some(FnArg::Typed(PatType { ty, .. })) = existing_params.first_mut() {
        if let Type::Ptr(TypePtr { elem, .. }) = &mut **ty {
            *elem = Box::new(Type::Path(sub.cpp().to_type_path()));
        } else {
            panic!("Unexpected self type parameter when creating subclass constructor");
        }
    } else {
        panic!("Unexpected self type parameter when creating subclass constructor");
    }
    let mut existing_params = existing_params.into_iter();
    let self_param = existing_params.next();
    let holder = sub.holder();
    let boxed_holder_param: FnArg = parse_quote! {
        peer: rust::Box<#holder>
    };
    let inputs = self_param
        .into_iter()
        .chain(std::iter::once(boxed_holder_param))
        .chain(existing_params)
        .collect();
    let self_ty = Some(sub.cpp());
    let maybe_wrap = Box::new(FuncToConvert {
        virtual_this_type: self_ty.clone(),
        self_ty,
        ident: subclass_constructor_name.clone(),
        doc_attr: fun.doc_attr.clone(),
        inputs,
        output: fun.output.clone(),
        vis: fun.vis.clone(),
        is_pure_virtual: false,
        cpp_vis: CppVisibility::Public,
        is_move_constructor: false,
        original_name: None,
        unused_template_param: fun.unused_template_param,
        return_type_is_reference: fun.return_type_is_reference,
        reference_args: fun.reference_args.clone(),
        synthesize_make_unique: fun.synthesize_make_unique,
    });
    let subclass_constructor_name = ApiName::new_with_cpp_name(
        &Namespace::new(),
        subclass_constructor_name,
        Some(sub.cpp().get_final_item().to_string()),
    );
    (maybe_wrap, subclass_constructor_name)
}

pub(super) fn create_subclass_constructor(
    sub: &SubclassName,
    analysis: &super::FnAnalysis,
    superclass: &QualifiedName,
) -> Api<FnPhase> {
    let holder_name = sub.holder();
    let cpp = sub.cpp();
    let wrapper_function_name = cpp.get_final_ident();
    let initial_arg = TypeConversionPolicy::new_unconverted(parse_quote! {
        rust::Box< #holder_name >
    });
    let args = std::iter::once(initial_arg).chain(
        analysis
            .param_details
            .iter()
            .skip(1) // skip placement new destination
            .map(|aa| aa.conversion.clone()),
    );
    let cpp_impl = CppFunction {
        payload: CppFunctionBody::ConstructSuperclass(superclass.to_cpp_name()),
        wrapper_function_name,
        return_conversion: None,
        argument_conversion: args.collect(),
        kind: CppFunctionKind::SynthesizedConstructor,
        pass_obs_field: false,
        qualification: Some(cpp.clone()),
        original_cpp_name: cpp.to_cpp_name(),
    };
    Api::RustSubclassConstructor {
        name: ApiName::new_from_qualified_name(cpp),
        subclass: sub.clone(),
        cpp_impl: Box::new(cpp_impl),
        is_trivial: analysis.param_details.len() == 1, // just placement new
                                                       // destination, no other parameters
    }
}
