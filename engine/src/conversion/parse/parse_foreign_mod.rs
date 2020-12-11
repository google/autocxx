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
    additional_cpp_generator::AdditionalNeed,
    conversion::ConvertError,
    function_wrapper::{ArgumentConversion, FunctionWrapper, FunctionWrapperPayload},
    types::{make_ident, Namespace, TypeName},
};
use quote::quote;
use std::collections::{HashMap, HashSet};
use syn::{
    parse::Parser, parse_quote, punctuated::Punctuated, Attribute, FnArg, ForeignItem,
    ForeignItemFn, Ident, ImplItem, Item, ItemImpl, Pat, ReturnType, Type, TypePtr,
};

use super::super::{
    api::{Api, Use},
    overload_tracker::OverloadTracker,
    unqualify::{unqualify_params, unqualify_ret_type},
};

struct ArgumentAnalysis {
    conversion: ArgumentConversion,
    name: Pat,
    self_type: Option<TypeName>,
    was_reference: bool,
    deps: HashSet<TypeName>,
}

struct ReturnTypeAnalysis {
    rt: ReturnType,
    conversion: Option<ArgumentConversion>,
    was_reference: bool,
    deps: HashSet<TypeName>,
}

/// Ways in which the conversion of a given extern "C" mod can
/// have more global effects or require more global knowledge outside
/// of its immediate conversion.
pub(crate) trait ForeignModParseCallbacks {
    fn convert_boxed_type(
        &self,
        ty: Box<Type>,
        ns: &Namespace,
    ) -> Result<(Box<Type>, HashSet<TypeName>), ConvertError>;
    fn is_pod(&self, ty: &TypeName) -> bool;
    fn add_api(&mut self, api: Api);
    fn get_cxx_bridge_name(
        &mut self,
        type_name: Option<&str>,
        found_name: &str,
        ns: &Namespace,
    ) -> String;
    fn ok_to_use_rust_name(&mut self, rust_name: &str) -> bool;
    fn is_on_allowlist(&self, type_name: &TypeName) -> bool;
    fn avoid_generating_type(&self, type_name: &TypeName) -> bool;
}

/// Converts a given bindgen-generated 'mod' into suitable
/// cxx::bridge runes. In bindgen output, a given mod concerns
/// a specific C++ namespace.
pub(crate) struct ParseForeignMod {
    ns: Namespace,
    overload_tracker: OverloadTracker,
    // We mostly act upon the functions we see within the 'extern "C"'
    // block of bindgen output, but we can't actually do this until
    // we've seen the (possibly subsequent) 'impl' blocks so we can
    // deduce which functions are actually static methods. Hence
    // store them.
    funcs_to_convert: Vec<ForeignItemFn>,
    // Evidence from 'impl' blocks about which of these items
    // may actually be methods (static or otherwise). Mapping from
    // function name to type name.
    method_receivers: HashMap<Ident, TypeName>,
}

impl ParseForeignMod {
    pub(crate) fn new(ns: Namespace) -> Self {
        Self {
            ns,
            overload_tracker: OverloadTracker::new(),
            funcs_to_convert: Vec::new(),
            method_receivers: HashMap::new(),
        }
    }

    /// Record information from foreign mod items encountered
    /// in bindgen output.
    pub(crate) fn convert_foreign_mod_items(
        &mut self,
        foreign_mod_items: Vec<ForeignItem>,
    ) -> Result<(), ConvertError> {
        for i in foreign_mod_items {
            match i {
                ForeignItem::Fn(f) => {
                    self.funcs_to_convert.push(f);
                }
                _ => return Err(ConvertError::UnexpectedForeignItem),
            }
        }
        Ok(())
    }

    /// Record information from impl blocks encountered in bindgen
    /// output.
    pub(crate) fn convert_impl_items(&mut self, imp: ItemImpl) {
        let ty_id = match *imp.self_ty {
            Type::Path(typ) => typ.path.segments.last().unwrap().ident.clone(),
            _ => return,
        };
        for i in imp.items {
            if let ImplItem::Method(itm) = i {
                let effective_fun_name = if itm.sig.ident == "new" {
                    ty_id.clone()
                } else {
                    itm.sig.ident
                };
                self.method_receivers.insert(
                    effective_fun_name,
                    TypeName::new(&self.ns, &ty_id.to_string()),
                );
            }
        }
    }

    /// Indicate that all foreign mods and all impl blocks have been
    /// fed into us, and we should process that information to generate
    /// the resulting APIs.
    pub(crate) fn finished(
        &mut self,
        callbacks: &mut impl ForeignModParseCallbacks,
    ) -> Result<(), ConvertError> {
        while !self.funcs_to_convert.is_empty() {
            let fun = self.funcs_to_convert.remove(0);
            self.convert_foreign_fn(fun, callbacks)?;
        }
        Ok(())
    }

    fn convert_foreign_fn(
        &mut self,
        fun: ForeignItemFn,
        callbacks: &mut impl ForeignModParseCallbacks,
    ) -> Result<(), ConvertError> {
        let ns = &self.ns.clone();
        // This function is one of the most complex parts of our conversion.
        // It needs to consider:
        // 1. Rejecting destructors entirely.
        // 2. For methods, we need to strip off the class name.
        // 3. For constructors, we change new(this: *Type, ...) into make_unique(...) -> UniquePtr<Type>
        // 4. For anything taking or returning a non-POD type _by value_,
        //    we need to generate a wrapper function in C++ which wraps and unwraps
        //    it from a unique_ptr.
        //    3a. And alias the original name to the wrapper.
        let initial_rust_name = fun.sig.ident.to_string();
        if initial_rust_name.ends_with("_destructor") {
            return Ok(());
        }

        // Now let's analyze all the parameters.
        let (param_details, bads): (Vec<_>, Vec<_>) = fun
            .sig
            .inputs
            .into_iter()
            .map(|i| self.convert_fn_arg(i, ns, callbacks))
            .partition(Result::is_ok);
        if let Some(problem) = bads.into_iter().next() {
            match problem {
                Err(e) => return Err(e),
                _ => panic!("Err didn't contain en err"),
            }
        }
        let (mut params, mut param_details): (Punctuated<_, syn::Token![,]>, Vec<_>) =
            param_details.into_iter().map(Result::unwrap).unzip();

        let params_deps: HashSet<_> = param_details
            .iter()
            .map(|p| p.deps.iter().cloned())
            .flatten()
            .collect();
        let mut self_ty = param_details
            .iter()
            .filter_map(|pd| pd.self_type.as_ref())
            .next()
            .cloned();

        let is_static_method = if self_ty.is_none() {
            // Even if we can't find a 'self' parameter this could conceivably
            // be a static method.
            self_ty = self.method_receivers.get(&fun.sig.ident).cloned();
            self_ty.is_some()
        } else {
            false
        };

        let is_a_method = self_ty.is_some();
        let self_ty = self_ty; // prevent subsequent mut'ing

        // Work out naming.
        let mut rust_name;
        let mut is_constructor = false;
        let cpp_call_name;
        if let Some(self_ty) = &self_ty {
            if !callbacks.is_on_allowlist(&self_ty) {
                // Bindgen will output methods for types which have been encountered
                // virally as arguments on other allowlisted types. But we don't want
                // to generate methods unless the user has specifically asked us to.
                // It may, for instance, be a private type.
                return Ok(());
            }
            // Method or static method.
            let type_ident = self_ty.get_final_ident().to_string();
            // bindgen generates methods with the name:
            // {class}_{method name}
            // It then generates an impl section for the Rust type
            // with the original name, but we currently discard that impl section.
            // We want to feed cxx methods with just the method name, so let's
            // strip off the class name.
            let overload_details = self
                .overload_tracker
                .get_method_real_name(&type_ident, &initial_rust_name);
            cpp_call_name = overload_details.cpp_method_name;
            rust_name = overload_details.rust_method_name;
            if rust_name.starts_with(&type_ident) {
                // It's a constructor. bindgen generates
                // fn new(this: *Type, ...args)
                // We want
                // fn make_unique(...args) -> Type
                // which later code will convert to
                // fn make_unique(...args) -> UniquePtr<Type>
                // If there are multiple constructors, bindgen generates
                // new, new1, new2 etc. and we'll keep those suffixes.
                let constructor_suffix = &rust_name[type_ident.len()..];
                rust_name = format!("make_unique{}", constructor_suffix);
                // Strip off the 'this' arg.
                params = params.into_iter().skip(1).collect();
                param_details.remove(0);
                is_constructor = true;
            }
        } else {
            // Not a method.
            // What's the name of the underlying C++ function call?
            // If bindgen found overloaded methods, it may not be what it seems.
            let overload_details = self
                .overload_tracker
                .get_function_real_name(&initial_rust_name);
            cpp_call_name = overload_details.cpp_method_name;
            rust_name = overload_details.rust_method_name;
        }

        // The name we use within the cxx::bridge mod may be different
        // from both the C++ name and the Rust name, because it's a flat
        // namespace so we might need to prepend some stuff to make it unique.
        let cxxbridge_name = callbacks.get_cxx_bridge_name(
            self_ty.as_ref().map(|ty| ty.get_final_ident()),
            &rust_name,
            &self.ns,
        );
        let mut cxxbridge_name = make_ident(&cxxbridge_name);

        // Analyze the return type, just as we previously did for the
        // parameters.
        let mut return_analysis = if is_constructor {
            let self_ty = self_ty.as_ref().unwrap();
            let constructed_type = self_ty.to_type_path();
            let mut these_deps = HashSet::new();
            these_deps.insert(self_ty.clone());
            ReturnTypeAnalysis {
                rt: parse_quote! {
                    -> #constructed_type
                },
                conversion: Some(ArgumentConversion::new_to_unique_ptr(parse_quote! {
                    #constructed_type
                })),
                was_reference: false,
                deps: these_deps,
            }
        } else {
            self.convert_return_type(callbacks, fun.sig.output, ns)?
        };
        let mut deps = params_deps;
        deps.extend(return_analysis.deps.drain());
        if deps.iter().any(|tn| callbacks.avoid_generating_type(tn)) {
            log::info!(
                "Skipping function {} due to return type or parameter being on blocklist or because only a forward declaration was encountered",
                rust_name
            );
            return Ok(()); // TODO think about how to inform user about this. Consider a more precise reason too.
        }
        if return_analysis.was_reference {
            // cxx only allows functions to return a reference if they take exactly
            // one reference as a parameter. Let's see...
            let num_input_references = param_details.iter().filter(|pd| pd.was_reference).count();
            if num_input_references != 1 {
                log::info!(
                    "Skipping function {} due to reference return type and <> 1 input reference",
                    rust_name
                );
                return Ok(()); // TODO think about how to inform user about this
            }
        }
        let mut ret_type = return_analysis.rt;
        let ret_type_conversion = return_analysis.conversion;

        // Do we need to convert either parameters or return type?
        let param_conversion_needed = param_details.iter().any(|b| b.conversion.work_needed());
        let ret_type_conversion_needed = ret_type_conversion
            .as_ref()
            .map_or(false, |x| x.work_needed());
        let differently_named_method = self_ty.is_some() && (cxxbridge_name != rust_name);
        let wrapper_function_needed = param_conversion_needed
            || ret_type_conversion_needed
            || is_static_method
            || differently_named_method;

        // When we generate the cxx::bridge fn declaration, we'll need to
        // put something different into here if we have to do argument or
        // return type conversion, so get some mutable variables ready.
        let mut rust_name_attr = Vec::new();
        let mut cpp_name_attr = Vec::new();
        let mut additional_cpp = None;

        if wrapper_function_needed {
            // Generate a new layer of C++ code to wrap/unwrap parameters
            // and return values into/out of std::unique_ptrs.
            // First give instructions to generate the additional C++.
            let cpp_construction_ident = make_ident(&cpp_call_name);
            cxxbridge_name = make_ident(&format!("{}_autocxx_wrapper", cxxbridge_name));
            let payload = if is_constructor {
                FunctionWrapperPayload::Constructor
            } else if is_static_method {
                FunctionWrapperPayload::StaticMethodCall(
                    ns.clone(),
                    make_ident(self_ty.as_ref().unwrap().get_final_ident()),
                    cpp_construction_ident,
                )
            } else {
                FunctionWrapperPayload::FunctionCall(ns.clone(), cpp_construction_ident)
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
                self.generate_method_impl(
                    &param_details,
                    is_constructor,
                    &make_ident(type_name.get_final_ident()),
                    &cxxbridge_name,
                    &rust_name,
                    &ret_type,
                    ns,
                    callbacks,
                );
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
        let vis = &fun.vis;
        let extern_c_mod_item = Some(ForeignItem::Fn(parse_quote!(
            #(#namespace_attr)*
            #(#rust_name_attr)*
            #(#cpp_name_attr)*
            #vis fn #cxxbridge_name ( #params ) #ret_type;
        )));
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
        let api = Api {
            ns: ns.clone(),
            id,
            use_stmt,
            extern_c_mod_item,
            additional_cpp,
            global_items: Vec::new(),
            bridge_item: None,
            deps,
            id_for_allowlist,
            bindgen_mod_item: None,
        };
        callbacks.add_api(api);
        Ok(())
    }

    /// Returns additionally a Boolean indicating whether an argument was
    /// 'this' and another one indicating whether we took a type by value
    /// and that type was non-trivial.
    fn convert_fn_arg(
        &self,
        arg: FnArg,
        ns: &Namespace,
        callbacks: &impl ForeignModParseCallbacks,
    ) -> Result<(FnArg, ArgumentAnalysis), ConvertError> {
        Ok(match arg {
            FnArg::Typed(mut pt) => {
                let mut self_type = None;
                let old_pat = *pt.pat;
                let new_pat = match old_pat {
                    syn::Pat::Ident(mut pp) if pp.ident == "this" => {
                        self_type = Some(match pt.ty.as_ref() {
                            Type::Ptr(TypePtr { elem, .. }) => match elem.as_ref() {
                                Type::Path(typ) => TypeName::from_type_path(typ),
                                _ => return Err(ConvertError::UnexpectedThisType),
                            },
                            _ => return Err(ConvertError::UnexpectedThisType),
                        });
                        pp.ident = Ident::new("self", pp.ident.span());
                        syn::Pat::Ident(pp)
                    }
                    _ => old_pat,
                };
                let (new_ty, deps) = callbacks.convert_boxed_type(pt.ty, ns)?;
                let was_reference = matches!(new_ty.as_ref(), Type::Reference(_));
                let conversion = self.argument_conversion_details(&new_ty, callbacks);
                pt.pat = Box::new(new_pat.clone());
                pt.ty = new_ty;
                (
                    FnArg::Typed(pt),
                    ArgumentAnalysis {
                        self_type,
                        name: new_pat,
                        conversion,
                        was_reference,
                        deps,
                    },
                )
            }
            _ => panic!("Did not expect FnArg::Receiver to be generated by bindgen"),
        })
    }

    fn conversion_details<F>(
        &self,
        ty: &Type,
        callbacks: &impl ForeignModParseCallbacks,
        conversion_direction: F,
    ) -> ArgumentConversion
    where
        F: FnOnce(Type) -> ArgumentConversion,
    {
        match ty {
            Type::Path(p) => {
                if callbacks.is_pod(&TypeName::from_type_path(p)) {
                    ArgumentConversion::new_unconverted(ty.clone())
                } else {
                    conversion_direction(ty.clone())
                }
            }
            _ => ArgumentConversion::new_unconverted(ty.clone()),
        }
    }

    fn argument_conversion_details(
        &self,
        ty: &Type,
        callbacks: &impl ForeignModParseCallbacks,
    ) -> ArgumentConversion {
        self.conversion_details(ty, callbacks, ArgumentConversion::new_from_unique_ptr)
    }

    fn return_type_conversion_details(
        &self,
        ty: &Type,
        callbacks: &impl ForeignModParseCallbacks,
    ) -> ArgumentConversion {
        self.conversion_details(ty, callbacks, ArgumentConversion::new_to_unique_ptr)
    }

    fn convert_return_type(
        &self,
        callbacks: &impl ForeignModParseCallbacks,
        rt: ReturnType,
        ns: &Namespace,
    ) -> Result<ReturnTypeAnalysis, ConvertError> {
        let result = match rt {
            ReturnType::Default => ReturnTypeAnalysis {
                rt: ReturnType::Default,
                was_reference: false,
                conversion: None,
                deps: HashSet::new(),
            },
            ReturnType::Type(rarrow, boxed_type) => {
                let (boxed_type, deps) = callbacks.convert_boxed_type(boxed_type, ns)?;
                let was_reference = matches!(boxed_type.as_ref(), Type::Reference(_));
                let conversion =
                    self.return_type_conversion_details(boxed_type.as_ref(), callbacks);
                ReturnTypeAnalysis {
                    rt: ReturnType::Type(rarrow, boxed_type),
                    conversion: Some(conversion),
                    was_reference,
                    deps,
                }
            }
        };
        Ok(result)
    }

    /// Generate an 'impl Type { methods-go-here }' item
    #[allow(clippy::too_many_arguments)] // Clippy's right, but the alternatives
                                         // are probably less maintainable still.
    fn generate_method_impl(
        &mut self,
        param_details: &[ArgumentAnalysis],
        is_constructor: bool,
        impl_block_type_name: &Ident,
        cxxbridge_name: &Ident,
        rust_name: &str,
        ret_type: &ReturnType,
        ns: &Namespace,
        callbacks: &mut impl ForeignModParseCallbacks,
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

        callbacks.add_api(Api {
            ns: ns.clone(),
            id: impl_block_type_name.clone(),
            use_stmt: Use::Unused,
            deps: HashSet::new(),
            extern_c_mod_item: None,
            bridge_item: None,
            global_items: Vec::new(),
            additional_cpp: None,
            id_for_allowlist: None,
            bindgen_mod_item: Some(Item::Impl(parse_quote! {
                impl #impl_block_type_name {
                    #extra_method
                }
            })),
        });
    }
}
