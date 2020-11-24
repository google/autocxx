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

use crate::{
    bridge_converter::ArgumentAnalysis,
    overload_tracker::OverloadTracker,
    types::{make_ident, Namespace},
};
use std::collections::{hash_map::Drain, HashMap};
use syn::{parse_quote, punctuated::Punctuated, FnArg, Ident, ImplItem, ItemImpl, ReturnType};

pub(crate) struct ForeignModConverter {
    ns: Namespace,
    overload_tracker: OverloadTracker,
    method_impl_blocks: HashMap<String, ItemImpl>,
}

impl ForeignModConverter {
    pub(crate) fn new(ns: Namespace) -> Self {
        Self {
            ns,
            overload_tracker: OverloadTracker::new(),
            method_impl_blocks: HashMap::new(),
        }
    }

    fn add_method_to_impl_block(&mut self, impl_block_type_name: &Ident, extra_method: ImplItem) {
        let e = self
            .method_impl_blocks
            .entry(impl_block_type_name.to_string())
            .or_insert_with(|| {
                parse_quote! {
                    impl #impl_block_type_name {
                    }
                }
            });
        e.items.push(extra_method);
    }

    pub(crate) fn get_impl_blocks(&mut self) -> Drain<String, ItemImpl> {
        self.method_impl_blocks.drain()
    }

    pub(crate) fn get_ns(&self) -> Namespace {
        self.ns.clone()
    }

    pub(crate) fn get_overload_tracker(&mut self) -> &mut OverloadTracker {
        &mut self.overload_tracker
    }

    pub(crate) fn generate_wrapper_fn(
        &mut self,
        param_details: &Vec<ArgumentAnalysis>,
        is_constructor: bool,
        impl_block_type_name: &Ident,
        cxxbridge_name: &Ident,
        rust_name: &str,
        ret_type: &ReturnType,
    ) {
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
        let extra_method = ImplItem::Method(parse_quote! {
            pub fn #rust_name ( #wrapper_params ) #ret_type {
                cxxbridge::#cxxbridge_name ( #(#arg_list),* )
            }
        });
        self.add_method_to_impl_block(impl_block_type_name, extra_method);
    }
}
