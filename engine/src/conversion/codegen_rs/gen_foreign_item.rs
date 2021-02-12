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

use std::collections::HashSet;

use syn::{Attribute, FnArg, ForeignItem, ForeignItemFn, Ident, ImplItem, LitStr, ReturnType, Type, TypePtr, parse::Parser, parse_quote, punctuated::Punctuated, token::Unsafe};
use crate::{conversion::{ConvertError, analysis::{function::{ArgumentAnalysis, FnAnalysis, FnMaterialization}}, api::{Api, Use}, codegen_cpp::{AdditionalNeed, function_wrapper::{FunctionWrapper, FunctionWrapperPayload}}}, types::{Namespace, TypeName, make_ident}};
use super::{RsCodeGenerator, RsCodegenResult, unqualify::{unqualify_params, unqualify_ret_type}};
use quote::quote;

pub(crate) struct FnConverter;

impl FnConverter {
    pub(crate) fn new() -> Self {
        Self
    }

        /*let r = self.convert_foreign_fn(fun, callbacks);
        if let Err(e) = r {
            if e.is_ignorable() {
                log::warn!("Skipped function because: {}", e);
            } else {
                return Err(e);
            }
        }*/


        // CUT HERE

    pub(crate) fn generate_fn(&mut self, api: Api<FnAnalysis>, item: &ForeignItemFn, virtual_this_ty: &Option<TypeName>, self_ty: &Option<TypeName>, analysis: &FnMaterialization) -> RsCodegenResult {
        let cxxbridge_name = analysis.cxxbridge_name;
        let rust_name = analysis.rust_name;
        let is_a_method = analysis.is_a_method;
        let param_details = analysis.param_details;
        let ret_type = analysis.ret_type;
        let is_constructor = analysis.is_constructor;
        let wrapper_function_needed = analysis.wrapper_function_needed;
        let requires_unsafe = analysis.requires_unsafe;
        let vis = analysis.vis;
        let cpp_call_name = analysis.cpp_call_name;
        let is_static_method = analysis.is_static_method;
        let return_analysis = analysis.return_analysis;
        let ret_type_conversion = analysis.ret_type_conversion;
        let params = analysis.params;

        let mut additional_cpp = None;

        if wrapper_function_needed {
            // Generate a new layer of C++ code to wrap/unwrap parameters
            // and return values into/out of std::unique_ptrs.
            // First give instructions to generate the additional C++.
            let cpp_construction_ident = make_ident(&cpp_call_name);
            let joiner = if cxxbridge_name.to_string().ends_with('_') {
                ""
            } else {
                "_"
            };
            cxxbridge_name = make_ident(&format!("{}{}autocxx_wrapper", cxxbridge_name, joiner));
            let payload = if is_constructor {
                FunctionWrapperPayload::Constructor
            } else if is_static_method {
                FunctionWrapperPayload::StaticMethodCall(
                    api.ns.clone(),
                    make_ident(self_ty.as_ref().unwrap().get_final_ident()),
                    cpp_construction_ident,
                )
            } else {
                FunctionWrapperPayload::FunctionCall(api.ns.clone(), cpp_construction_ident)
            };
            additional_cpp = Some(AdditionalNeed::FunctionWrapper(Box::new(FunctionWrapper {
                payload,
                wrapper_function_name: cxxbridge_name.clone(),
                return_conversion: ret_type_conversion.clone(),
                argument_conversion: param_details.iter().map(|d| d.conversion.clone()).collect(),
                is_a_method: is_a_method && !is_constructor && !is_static_method,
            })));
            // Now modify the cxx::bridge entry we're going to make.
            if let Some(conversion) = ret_type_conversion {
                let new_ret_type = conversion.unconverted_rust_type();
                ret_type = parse_quote!(
                    -> #new_ret_type
                );
            }

            // Amend parameters for the function which we're asking cxx to generate.
            params.clear();
            for pd in &param_details {
                let type_name = pd.conversion.converted_rust_type();
                let arg_name = if pd.self_type.is_some() && !is_constructor {
                    parse_quote!(autocxx_gen_this)
                } else {
                    pd.name.clone()
                };
                params.push(parse_quote!(
                    #arg_name: #type_name
                ));
            }
        }

        // When we generate the cxx::bridge fn declaration, we'll need to
        // put something different into here if we have to do argument or
        // return type conversion, so get some mutable variables ready.
        let mut rust_name_attr = Vec::new();
        let mut cpp_name_attr = Vec::new();
        let mut impl_entry = None;

        let mut use_alias_required = None;
        if cxxbridge_name == rust_name {
            if !is_a_method {
                // Mark that this name is now occupied in the output
                // namespace of cxx, so that future functions we encounter
                // with the same name instead get called something else.
                callbacks.ok_to_use_rust_name(&rust_name);
            }
        } else {
            // Now we've made a brand new function, we need to plumb it back
            // into place such that users can call it just as if it were
            // the original function.
            if let Some(type_name) = &self_ty {
                // Method, or static method.
                impl_entry = Some(self.generate_method_impl(
                    &param_details,
                    is_constructor,
                    &make_ident(type_name.get_final_ident()),
                    &cxxbridge_name,
                    &rust_name,
                    &ret_type,
                    &api.ns,
                ));
            } else {
                // Keep the original Rust name the same so callers don't
                // need to know about all of these shenanigans.
                // There is a global space of rust_names even if they're in
                // different namespaces.
                let rust_name_ok = callbacks.ok_to_use_rust_name(&rust_name);
                if rust_name_ok {
                    rust_name_attr = Attribute::parse_outer
                        .parse2(quote!(
                            #[rust_name = #rust_name]
                        ))
                        .unwrap();
                } else {
                    use_alias_required = Some(make_ident(&rust_name));
                }
            }
        }
        if cxxbridge_name != cpp_call_name && !wrapper_function_needed {
            cpp_name_attr = Attribute::parse_outer
                .parse2(quote!(
                    #[cxx_name = #cpp_call_name]
                ))
                .unwrap();
        }
        // Finally - namespace support. All the Types in everything
        // above this point are fully qualified. We need to unqualify them.
        // We need to do that _after_ the above wrapper_function_needed
        // work, because it relies upon spotting fully qualified names like
        // std::unique_ptr. However, after it's done its job, all such
        // well-known types should be unqualified already (e.g. just UniquePtr)
        // and the following code will act to unqualify only those types
        // which the user has declared.
        let params = unqualify_params(params);
        let ret_type = unqualify_ret_type(ret_type);
        // And we need to make an attribute for the namespace that the function
        // itself is in.
        let namespace_attr = if api.ns.is_empty() || wrapper_function_needed {
            Vec::new()
        } else {
            let namespace_string = api.ns.to_string();
            Attribute::parse_outer
                .parse2(quote!(
                    #[namespace = #namespace_string]
                ))
                .unwrap()
        };
        // At last, actually generate the cxx::bridge entry.
        let unsafety: Option<Unsafe> = if requires_unsafe {
            Some(parse_quote!(unsafe))
        } else {
            None
        };
        let extern_c_mod_item = ForeignItem::Fn(parse_quote!(
            #(#namespace_attr)*
            #(#rust_name_attr)*
            #(#cpp_name_attr)*
            #vis #unsafety fn #cxxbridge_name ( #params ) #ret_type;
        ));
        let (id, use_stmt, id_for_allowlist) = if is_a_method {
            (
                make_ident(&rust_name),
                Use::Unused,
                self_ty.map(|ty| make_ident(ty.get_final_ident())),
            )
        } else {
            match use_alias_required {
                None => (make_ident(&rust_name), Use::Used, None),
                Some(alias) => (cxxbridge_name, Use::UsedWithAlias(alias), None),
            }
        };

        RsCodegenResult {
            extern_c_mod_item: Some(extern_c_mod_item),
            bridge_items: Vec::new(),
            global_items: Vec::new(),
            bindgen_mod_item: None,
            impl_entry,
        }

    }

    /// Generate an 'impl Type { methods-go-here }' item
    fn generate_method_impl(
        &mut self,
        param_details: &[ArgumentAnalysis],
        is_constructor: bool,
        impl_block_type_name: &Ident,
        cxxbridge_name: &Ident,
        rust_name: &str,
        ret_type: &ReturnType,
        ns: &Namespace,
    ) -> ImplItem {
        let mut wrapper_params: Punctuated<FnArg, syn::Token![,]> = Punctuated::new();
        let mut arg_list = Vec::new();
        for pd in param_details {
            let type_name = pd.conversion.converted_rust_type();
            let wrapper_arg_name = if pd.self_type.is_some() && !is_constructor {
                parse_quote!(self)
            } else {
                pd.name.clone()
            };
            wrapper_params.push(parse_quote!(
                #wrapper_arg_name: #type_name
            ));
            arg_list.push(wrapper_arg_name);
        }

        let rust_name = make_ident(&rust_name);
        ImplItem::Method(parse_quote! {
            pub fn #rust_name ( #wrapper_params ) #ret_type {
                cxxbridge::#cxxbridge_name ( #(#arg_list),* )
            }
        })
    }

}