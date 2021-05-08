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

use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    parse::Parser, parse_quote, punctuated::Punctuated, token::Unsafe, Attribute, FnArg,
    ForeignItem, Ident, ImplItem, Item, ReturnType,
};

use super::{
    doc_attr::get_doc_attr,
    unqualify::{unqualify_params, unqualify_ret_type},
    RsCodegenResult, Use,
};
use crate::{conversion::api::FuncToConvert, types::make_ident};
use crate::{
    conversion::{
        analysis::fun::{ArgumentAnalysis, FnAnalysisBody, FnKind, MethodKind, RustRenameStrategy},
        api::ImplBlockDetails,
    },
    types::{Namespace, QualifiedName},
};

pub(super) fn gen_function(
    ns: &Namespace,
    fun: FuncToConvert,
    analysis: FnAnalysisBody,
) -> RsCodegenResult {
    let cxxbridge_name = analysis.cxxbridge_name;
    let rust_name = analysis.rust_name;
    let ret_type = analysis.ret_type;
    let param_details = analysis.param_details;
    let cpp_call_name = analysis.cpp_call_name;
    let wrapper_function_needed = analysis.cpp_wrapper.is_some();
    let params = analysis.params;
    let vis = analysis.vis;
    let kind = analysis.kind;
    let doc_attr = get_doc_attr(&fun.item.attrs);

    let mut cpp_name_attr = Vec::new();
    let mut impl_entry = None;
    let unsafety: Option<Unsafe> = if analysis.requires_unsafe {
        Some(parse_quote!(unsafe))
    } else {
        None
    };
    let rust_name_attr: Vec<_> = match &analysis.rust_rename_strategy {
        RustRenameStrategy::RenameUsingRustAttr => Attribute::parse_outer
            .parse2(quote!(
                #[rust_name = #rust_name]
            ))
            .unwrap(),
        _ => Vec::new(),
    };
    let mut materialization = match kind {
        FnKind::Method(..) => Use::Unused,
        FnKind::Function => match analysis.rust_rename_strategy {
            RustRenameStrategy::RenameInOutputMod(alias) => Use::UsedFromCxxBridgeWithAlias(alias),
            _ => Use::UsedFromCxxBridge,
        },
    };
    let any_param_needs_rust_conversion = param_details
        .iter()
        .any(|pd| pd.conversion.rust_work_needed());
    let rust_wrapper_needed = any_param_needs_rust_conversion
        || (cxxbridge_name != rust_name && matches!(kind, FnKind::Method(..)));
    if rust_wrapper_needed {
        if let FnKind::Method(ref type_name, ref method_kind) = kind {
            // Method, or static method.
            impl_entry = Some(generate_method_impl(
                &param_details,
                matches!(method_kind, MethodKind::Constructor),
                type_name,
                &cxxbridge_name,
                &rust_name,
                &ret_type,
                &unsafety,
                &doc_attr,
            ));
        } else {
            // Generate plain old function
            materialization = Use::Custom(generate_function_impl(
                &param_details,
                &rust_name,
                &ret_type,
                &unsafety,
                &doc_attr,
            ));
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
    let namespace_attr = if ns.is_empty() || wrapper_function_needed {
        Vec::new()
    } else {
        let namespace_string = ns.to_string();
        Attribute::parse_outer
            .parse2(quote!(
                #[namespace = #namespace_string]
            ))
            .unwrap()
    };
    // At last, actually generate the cxx::bridge entry.
    let extern_c_mod_item = ForeignItem::Fn(parse_quote!(
        #(#namespace_attr)*
        #(#rust_name_attr)*
        #(#cpp_name_attr)*
        #doc_attr
        #vis #unsafety fn #cxxbridge_name ( #params ) #ret_type;
    ));
    RsCodegenResult {
        extern_c_mod_item: Some(extern_c_mod_item),
        bridge_items: Vec::new(),
        global_items: Vec::new(),
        bindgen_mod_item: None,
        impl_entry,
        materialization,
    }
}

fn generate_arg_lists(
    param_details: &[ArgumentAnalysis],
    is_constructor: bool,
) -> (Punctuated<FnArg, syn::Token![,]>, Vec<TokenStream>) {
    let mut wrapper_params: Punctuated<FnArg, syn::Token![,]> = Punctuated::new();
    let mut arg_list = Vec::new();

    for pd in param_details {
        let type_name = pd.conversion.rust_wrapper_unconverted_type();
        let wrapper_arg_name = if pd.self_type.is_some() && !is_constructor {
            parse_quote!(self)
        } else {
            pd.name.clone()
        };
        wrapper_params.push(parse_quote!(
            #wrapper_arg_name: #type_name
        ));
        arg_list.push(pd.conversion.rust_conversion(wrapper_arg_name));
    }
    (wrapper_params, arg_list)
}

/// Generate an 'impl Type { methods-go-here }' item
#[allow(clippy::too_many_arguments)] // it's true, but probably best for now
fn generate_method_impl(
    param_details: &[ArgumentAnalysis],
    is_constructor: bool,
    impl_block_type_name: &QualifiedName,
    cxxbridge_name: &Ident,
    rust_name: &str,
    ret_type: &ReturnType,
    unsafety: &Option<Unsafe>,
    doc_attr: &Option<Attribute>,
) -> Box<ImplBlockDetails> {
    let (wrapper_params, arg_list) = generate_arg_lists(param_details, is_constructor);
    let rust_name = make_ident(&rust_name);
    Box::new(ImplBlockDetails {
        item: ImplItem::Method(parse_quote! {
            #doc_attr
            pub #unsafety fn #rust_name ( #wrapper_params ) #ret_type {
                cxxbridge::#cxxbridge_name ( #(#arg_list),* )
            }
        }),
        ty: impl_block_type_name.get_final_ident(),
    })
}

/// Generate a function call wrapper
fn generate_function_impl(
    param_details: &[ArgumentAnalysis],
    rust_name: &str,
    ret_type: &ReturnType,
    unsafety: &Option<Unsafe>,
    doc_attr: &Option<Attribute>,
) -> Box<Item> {
    let (wrapper_params, arg_list) = generate_arg_lists(param_details, false);
    let rust_name = make_ident(&rust_name);
    Box::new(Item::Fn(parse_quote! {
        #doc_attr
        pub #unsafety fn #rust_name ( #wrapper_params ) #ret_type {
            cxxbridge::#rust_name ( #(#arg_list),* )
        }
    }))
}
