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

use std::collections::{HashMap, HashSet};
use syn::{parse_quote, punctuated::Punctuated, Type};

use super::fun::{FnAnalysis, FnAnalysisBody};
use crate::conversion::api::Api;
use crate::conversion::api::Use;
use crate::conversion::codegen_cpp::{
    function_wrapper::{ArgumentConversion, FunctionWrapper, FunctionWrapperPayload},
    AdditionalNeed,
};
use crate::{
    conversion::api::ApiDetail,
    types::{make_ident, TypeName},
};

/// Adds constructors for any types which otherwise lack constructors.
///
/// There's a reasonable amount of duplication here with the data fields
/// filled in in [crate::conversion::analysis::fun]. It might be desirable to split
/// that analysis into two; the first half identifies names and types sufficiently
/// for us to work out missing constructors - then we run this analysis or something
/// similar, but without filling in quite so many detailed fields
/// - and then the second half of the current [fun] analysis runs which then
/// acts upon all functions to fill in all the details.
pub(crate) fn add_missing_constructors(apis: &mut Vec<Api<FnAnalysis>>) {
    let mut constructors_by_type: HashMap<TypeName, usize> = HashMap::new();
    for api in apis.iter() {
        match &api.detail {
            ApiDetail::Function { fun: _, analysis } if analysis.is_constructor => {
                *constructors_by_type
                    .entry(analysis.self_ty.as_ref().unwrap().clone())
                    .or_default() += 1;
            }
            ApiDetail::Type { .. } => {
                constructors_by_type.entry(api.typename()).or_default();
            }
            _ => {}
        }
    }
    let types_needing_constructors = constructors_by_type
        .into_iter()
        .filter_map(|(ty, count)| if count == 0 { Some(ty) } else { None })
        .enumerate();
    for (count, ty) in types_needing_constructors {
        let tp = ty.to_type_path();
        let fn_name = format!("autocxx_synthesized_constructor{}", count);
        let conv = ArgumentConversion::new_to_unique_ptr(Type::Path(tp));
        let ret_type = conv.unconverted_rust_type();
        apis.push(Api {
            ns: ty.get_namespace().clone(),
            id: make_ident("make_unique"),
            deps: HashSet::new(),
            detail: ApiDetail::SynthesizedFunction {
                analysis: FnAnalysisBody {
                    rename_using_rust_attr: false,
                    cxxbridge_name: make_ident(&fn_name),
                    rust_name: "make_unique".into(),
                    params: Punctuated::new(),
                    self_ty: Some(ty),
                    ret_type: parse_quote! { -> #ret_type },
                    is_constructor: true,
                    param_details: Vec::new(),
                    cpp_call_name: fn_name.clone(),
                    wrapper_function_needed: false,
                    requires_unsafe: false,
                    vis: parse_quote! { pub },
                    id_for_allowlist: None,
                    use_stmt: Use::Unused,
                    additional_cpp: Some(AdditionalNeed::FunctionWrapper(Box::new(
                        FunctionWrapper {
                            payload: FunctionWrapperPayload::Constructor,
                            wrapper_function_name: make_ident(&fn_name),
                            return_conversion: Some(conv),
                            argument_conversion: Vec::new(),
                            is_a_method: false,
                        },
                    ))),
                },
            },
        })
    }
}
