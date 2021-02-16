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

mod bridge_name_tracker;
mod overload_tracker;
mod rust_name_tracker;

use std::collections::{HashMap, HashSet};

use autocxx_parser::{TypeConfig, UnsafePolicy};
use syn::{
    parse_quote, punctuated::Punctuated, FnArg, ForeignItemFn, Ident, LitStr, Pat, ReturnType,
    Type, TypePtr, Visibility,
};

use crate::{
    conversion::{
        api::{Api, ApiAnalysis, ApiDetail, FuncToConvert, TypeKind, UnanalyzedApi, Use},
        codegen_cpp::{
            function_wrapper::{ArgumentConversion, FunctionWrapper, FunctionWrapperPayload},
            AdditionalNeed,
        },
        parse::type_converter::TypeConverter,
        ConvertError,
    },
    types::{make_ident, Namespace, TypeName},
};

use self::{
    bridge_name_tracker::BridgeNameTracker, overload_tracker::OverloadTracker,
    rust_name_tracker::RustNameTracker,
};

use super::pod::{ByValueChecker, PodAnalysis};

pub(crate) struct FnAnalysisBody {
    pub(crate) rename_using_rust_attr: bool,
    pub(crate) cxxbridge_name: Ident,
    pub(crate) rust_name: String,
    pub(crate) params: Punctuated<FnArg, syn::Token![,]>,
    pub(crate) self_ty: Option<TypeName>,
    pub(crate) ret_type: ReturnType,
    pub(crate) is_constructor: bool,
    pub(crate) param_details: Vec<ArgumentAnalysis>,
    pub(crate) cpp_call_name: String,
    pub(crate) wrapper_function_needed: bool,
    pub(crate) requires_unsafe: bool,
    pub(crate) vis: Visibility,
}

pub(crate) struct ArgumentAnalysis {
    pub(crate) conversion: ArgumentConversion,
    pub(crate) name: Pat,
    pub(crate) self_type: Option<TypeName>,
    was_reference: bool,
    deps: HashSet<TypeName>,
    virtual_this_encountered: bool,
    requires_unsafe: bool,
}

pub(crate) struct ReturnTypeAnalysis {
    rt: ReturnType,
    conversion: Option<ArgumentConversion>,
    was_reference: bool,
    deps: HashSet<TypeName>,
}

pub(crate) struct FnAnalysis;

impl ApiAnalysis for FnAnalysis {
    type TypeAnalysis = TypeKind;
    type FunAnalysis = FnAnalysisBody;
}

pub(crate) struct FnAnalyzer<'a> {
    unsafe_policy: UnsafePolicy,
    rust_name_tracker: RustNameTracker,
    extra_apis: Vec<UnanalyzedApi>,
    type_converter: &'a mut TypeConverter,
    bridge_name_tracker: BridgeNameTracker,
    byvalue_checker: &'a ByValueChecker,
    type_config: &'a TypeConfig,
    incomplete_types: HashSet<TypeName>,
    overload_trackers_by_mod: HashMap<Namespace, OverloadTracker>,
}

struct FnAnalysisResult(
    FnAnalysisBody,
    Ident,
    Use,
    HashSet<TypeName>,
    Option<Ident>,
    Option<AdditionalNeed>,
);

impl<'a> FnAnalyzer<'a> {
    pub(crate) fn analyze_functions(
        apis: Vec<Api<PodAnalysis>>,
        unsafe_policy: UnsafePolicy,
        type_converter: &'a mut TypeConverter,
        byvalue_checker: &'a ByValueChecker,
        type_database: &'a TypeConfig,
    ) -> Result<Vec<Api<FnAnalysis>>, ConvertError> {
        let incomplete_types = apis
            .iter()
            .filter_map(|api| match api.detail {
                ApiDetail::Type {
                    ty_details: _,
                    for_extern_c_ts: _,
                    is_forward_declaration,
                    bindgen_mod_item: _,
                    analysis: _,
                } if is_forward_declaration => Some(TypeName::new(&api.ns, &api.id.to_string())),
                _ => None,
            })
            .collect();
        let mut me = Self {
            unsafe_policy,
            rust_name_tracker: RustNameTracker::new(),
            extra_apis: Vec::new(),
            type_converter,
            bridge_name_tracker: BridgeNameTracker::new(),
            byvalue_checker,
            type_config: type_database,
            incomplete_types,
            overload_trackers_by_mod: HashMap::new(),
        };
        let mut results = Vec::new();
        for api in apis {
            let r = me.analyze_fn_api(api);
            match r {
                Err(e) if e.is_ignorable() => eprintln!("Skipped function because: {}", e),
                Err(e) => return Err(e),
                Ok(Some(api)) => results.push(api),
                Ok(None) => {}
            }
        }
        results.extend(me.extra_apis.into_iter().map(Self::make_extra_api_nonpod));
        Ok(results)
    }

    /// Processing functions sometimes results in new types being materialized.
    /// In future, if we wanted to make these POD, we'd probably want to create
    /// a new analysis phase prior to the POD analysis which materializes these types.
    fn make_extra_api_nonpod(api: UnanalyzedApi) -> Api<FnAnalysis> {
        let new_detail = match api.detail {
            ApiDetail::ConcreteType(stuff) => ApiDetail::ConcreteType(stuff),
            _ => panic!("Function analysis created an extra API which wasn't a concrete type"),
        };
        Api {
            ns: api.ns,
            id: api.id,
            use_stmt: api.use_stmt,
            deps: api.deps,
            id_for_allowlist: api.id_for_allowlist,
            additional_cpp: api.additional_cpp,
            detail: new_detail,
        }
    }

    fn analyze_fn_api(
        &mut self,
        api: Api<PodAnalysis>,
    ) -> Result<Option<Api<FnAnalysis>>, ConvertError> {
        let mut new_deps = api.deps.clone();
        let mut new_use_stmt = api.use_stmt.clone();
        let mut new_id_for_allowlist = api.id_for_allowlist.clone();
        let mut new_id = api.id;
        let mut new_additional_cpp = api.additional_cpp.clone();
        let api_detail = match api.detail {
            // No changes to any of these...
            ApiDetail::ConcreteType(details) => ApiDetail::ConcreteType(details),
            ApiDetail::StringConstructor => ApiDetail::StringConstructor,
            ApiDetail::Function { fun, analysis: _ } => {
                let analysis = self.analyze_foreign_fn(&api.ns, &fun)?;
                match analysis {
                    None => return Ok(None),
                    Some(FnAnalysisResult(
                        analysis,
                        id,
                        fn_uses,
                        fn_deps,
                        fn_id_for_allowlist,
                        fn_additional_cpp,
                    )) => {
                        new_deps = fn_deps;
                        new_use_stmt = fn_uses;
                        new_id_for_allowlist = fn_id_for_allowlist;
                        new_additional_cpp = fn_additional_cpp;
                        new_id = id;
                        ApiDetail::Function { fun, analysis }
                    }
                }
            }
            ApiDetail::Const { const_item } => ApiDetail::Const { const_item },
            ApiDetail::Typedef { type_item } => ApiDetail::Typedef { type_item },
            ApiDetail::CType { id } => ApiDetail::CType { id },
            // Just changes to this one...
            ApiDetail::Type {
                ty_details,
                for_extern_c_ts,
                is_forward_declaration,
                bindgen_mod_item,
                analysis,
            } => ApiDetail::Type {
                ty_details,
                for_extern_c_ts,
                is_forward_declaration,
                bindgen_mod_item,
                analysis,
            },
        };
        Ok(Some(Api {
            ns: api.ns,
            id: new_id,
            use_stmt: new_use_stmt,
            deps: new_deps,
            id_for_allowlist: new_id_for_allowlist,
            additional_cpp: new_additional_cpp,
            detail: api_detail,
        }))
    }

    fn convert_boxed_type(
        &mut self,
        ty: Box<Type>,
        ns: &Namespace,
        convert_ptrs_to_reference: bool,
    ) -> Result<(Box<Type>, HashSet<TypeName>, bool), ConvertError> {
        let annotated =
            self.type_converter
                .convert_boxed_type(ty, ns, convert_ptrs_to_reference)?;
        self.extra_apis.extend(annotated.extra_apis);
        Ok((
            annotated.ty,
            annotated.types_encountered,
            annotated.requires_unsafe,
        ))
    }

    fn is_pod(&self, ty: &TypeName) -> bool {
        self.byvalue_checker.is_pod(ty)
    }

    fn get_cxx_bridge_name(
        &mut self,
        type_name: Option<&str>,
        found_name: &str,
        ns: &Namespace,
    ) -> String {
        self.bridge_name_tracker
            .get_unique_cxx_bridge_name(type_name, found_name, ns)
    }

    fn ok_to_use_rust_name(&mut self, rust_name: &str) -> bool {
        self.rust_name_tracker.ok_to_use_rust_name(rust_name)
    }

    fn is_on_allowlist(&self, type_name: &TypeName) -> bool {
        self.type_config.is_on_allowlist(&type_name.to_cpp_name())
    }

    fn avoid_generating_type(&self, type_name: &TypeName) -> bool {
        self.type_config.is_on_blocklist(&type_name.to_cpp_name())
            || self.incomplete_types.contains(type_name)
    }

    fn should_be_unsafe(&self) -> bool {
        self.unsafe_policy == UnsafePolicy::AllFunctionsUnsafe
    }

    fn analyze_foreign_fn(
        &mut self,
        ns: &Namespace,
        func_information: &FuncToConvert,
    ) -> Result<Option<FnAnalysisResult>, ConvertError> {
        let fun = &func_information.item;
        let virtual_this = &func_information.virtual_this_type;
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
            return Ok(None);
        }

        let original_name = Self::get_bindgen_original_name_annotation(&fun);
        let (reference_params, reference_return) = Self::get_reference_parameters_and_return(&fun);
        let diagnostic_display_name = original_name.as_ref().unwrap_or(&initial_rust_name);

        // Now let's analyze all the parameters.
        let (param_details, bads): (Vec<_>, Vec<_>) = fun
            .sig
            .inputs
            .iter()
            .map(|i| {
                self.convert_fn_arg(
                    i,
                    &ns,
                    diagnostic_display_name,
                    virtual_this.clone(),
                    &reference_params,
                )
            })
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
        let virtual_this_encountered = param_details.iter().any(|pd| pd.virtual_this_encountered);
        let requires_unsafe = param_details.iter().any(|pd| pd.requires_unsafe);

        let is_static_method = if self_ty.is_none() {
            // Even if we can't find a 'self' parameter this could conceivably
            // be a static method.
            self_ty = func_information.self_ty.clone();
            self_ty.is_some()
        } else {
            false
        };

        let is_a_method = self_ty.is_some();
        let self_ty = self_ty; // prevent subsequent mut'ing

        // Work out naming.
        let mut rust_name;
        let mut is_constructor = false;
        // bindgen may have mangled the name either because it's invalid Rust
        // syntax (e.g. a keyword like 'async') or it's an overload.
        // If the former, we respect that mangling. If the latter, we don't,
        // because we'll add our own overload counting mangling later.
        let name_probably_invalid_in_rust =
            original_name.is_some() && initial_rust_name.ends_with('_');
        // The C++ call name will always be whatever bindgen tells us.
        let cpp_call_name = original_name.unwrap_or_else(|| initial_rust_name.clone());
        let ideal_rust_name = if name_probably_invalid_in_rust {
            initial_rust_name
        } else {
            cpp_call_name.clone()
        };
        if let Some(self_ty) = &self_ty {
            if !self.is_on_allowlist(&self_ty) {
                // Bindgen will output methods for types which have been encountered
                // virally as arguments on other allowlisted types. But we don't want
                // to generate methods unless the user has specifically asked us to.
                // It may, for instance, be a private type.
                return Ok(None);
            }
            // Method or static method.
            let type_ident = self_ty.get_final_ident().to_string();
            // bindgen generates methods with the name:
            // {class}_{method name}
            // It then generates an impl section for the Rust type
            // with the original name, but we currently discard that impl section.
            // We want to feed cxx methods with just the method name, so let's
            // strip off the class name.
            let overload_tracker = self.overload_trackers_by_mod.entry(ns.clone()).or_default();
            rust_name = overload_tracker.get_method_real_name(&type_ident, ideal_rust_name);
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
            // What shall we call this function? It may be overloaded.
            let overload_tracker = self.overload_trackers_by_mod.entry(ns.clone()).or_default();
            rust_name = overload_tracker.get_function_real_name(ideal_rust_name);
        }

        // The name we use within the cxx::bridge mod may be different
        // from both the C++ name and the Rust name, because it's a flat
        // namespace so we might need to prepend some stuff to make it unique.
        let cxxbridge_name = self.get_cxx_bridge_name(
            self_ty.as_ref().map(|ty| ty.get_final_ident()),
            &rust_name,
            &ns,
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
            self.convert_return_type(&fun.sig.output, &ns, reference_return)?
        };
        let mut deps = params_deps;
        deps.extend(return_analysis.deps.drain());
        if deps.iter().any(|tn| self.avoid_generating_type(tn)) {
            return Err(ConvertError::UnacceptableParam(rust_name));
        }
        if return_analysis.was_reference {
            // cxx only allows functions to return a reference if they take exactly
            // one reference as a parameter. Let's see...
            let num_input_references = param_details.iter().filter(|pd| pd.was_reference).count();
            if num_input_references != 1 {
                return Err(ConvertError::NotOneInputReference(rust_name));
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
            || differently_named_method
            || virtual_this_encountered;

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

        // Bits copied from below
        let mut use_alias_required = None;
        let mut rename_using_rust_attr = false;
        if cxxbridge_name == rust_name {
            if !is_a_method {
                // Mark that this name is now occupied in the output
                // namespace of cxx, so that future functions we encounter
                // with the same name instead get called something else.
                self.ok_to_use_rust_name(&rust_name);
            }
        } else {
            // Now we've made a brand new function, we need to plumb it back
            // into place such that users can call it just as if it were
            // the original function.
            if self_ty.is_none() {
                // Keep the original Rust name the same so callers don't
                // need to know about all of these shenanigans.
                // There is a global space of rust_names even if they're in
                // different namespaces.
                let rust_name_ok = self.ok_to_use_rust_name(&rust_name);
                if rust_name_ok {
                    rename_using_rust_attr = true;
                } else {
                    use_alias_required = Some(make_ident(&rust_name));
                }
            }
        }

        let requires_unsafe = requires_unsafe || self.should_be_unsafe();
        let vis = func_information.item.vis.clone();

        let (id, use_stmt, id_for_allowlist) = if is_a_method {
            (
                make_ident(&rust_name),
                Use::Unused,
                self_ty.clone().map(|ty| make_ident(ty.get_final_ident())),
            )
        } else {
            match use_alias_required {
                None => (make_ident(&rust_name), Use::Used, None),
                Some(alias) => (cxxbridge_name.clone(), Use::UsedWithAlias(alias), None),
            }
        };

        // TODO work out what 'id' was used for
        Ok(Some(FnAnalysisResult(
            FnAnalysisBody {
                rename_using_rust_attr,
                cxxbridge_name,
                rust_name,
                params,
                self_ty,
                ret_type,
                is_constructor,
                param_details,
                cpp_call_name,
                wrapper_function_needed,
                requires_unsafe,
                vis,
            },
            id,
            use_stmt,
            deps,
            id_for_allowlist,
            additional_cpp,
        )))
    }

    /// Returns additionally a Boolean indicating whether an argument was
    /// 'this' and another one indicating whether we took a type by value
    /// and that type was non-trivial.
    fn convert_fn_arg(
        &mut self,
        arg: &FnArg,
        ns: &Namespace,
        fn_name: &str,
        virtual_this: Option<TypeName>,
        reference_args: &HashSet<Ident>,
    ) -> Result<(FnArg, ArgumentAnalysis), ConvertError> {
        Ok(match arg {
            FnArg::Typed(pt) => {
                let mut pt = pt.clone();
                let mut self_type = None;
                let old_pat = *pt.pat;
                let mut virtual_this_encountered = false;
                let mut treat_as_reference = false;
                let new_pat = match old_pat {
                    syn::Pat::Ident(mut pp) if pp.ident == "this" => {
                        let this_type = match pt.ty.as_ref() {
                            Type::Ptr(TypePtr {
                                elem, mutability, ..
                            }) => match elem.as_ref() {
                                Type::Path(typ) => {
                                    let mut this_type = TypeName::from_type_path(typ);
                                    if this_type.is_cvoid() {
                                        virtual_this_encountered = true;
                                        this_type = virtual_this.ok_or_else(|| {
                                            ConvertError::VirtualThisType(
                                                ns.clone(),
                                                fn_name.into(),
                                            )
                                        })?;
                                        let this_type_path = this_type.to_type_path();
                                        pt.ty = Box::new(parse_quote! {
                                            * #mutability #this_type_path
                                        });
                                    }
                                    Ok(this_type)
                                }
                                _ => Err(ConvertError::UnexpectedThisType(
                                    ns.clone(),
                                    fn_name.into(),
                                )),
                            },
                            _ => Err(ConvertError::UnexpectedThisType(ns.clone(), fn_name.into())),
                        }?;
                        self_type = Some(this_type);
                        pp.ident = Ident::new("self", pp.ident.span());
                        treat_as_reference = true;
                        syn::Pat::Ident(pp)
                    }
                    syn::Pat::Ident(pp) => {
                        treat_as_reference = reference_args.contains(&pp.ident);
                        syn::Pat::Ident(pp)
                    }
                    _ => old_pat,
                };
                let (new_ty, deps, requires_unsafe) =
                    self.convert_boxed_type(pt.ty, ns, treat_as_reference)?;
                let was_reference = matches!(new_ty.as_ref(), Type::Reference(_));
                let conversion = self.argument_conversion_details(&new_ty);
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
                        virtual_this_encountered,
                        requires_unsafe,
                    },
                )
            }
            _ => panic!("Did not expect FnArg::Receiver to be generated by bindgen"),
        })
    }

    fn conversion_details<F>(&self, ty: &Type, conversion_direction: F) -> ArgumentConversion
    where
        F: FnOnce(Type) -> ArgumentConversion,
    {
        match ty {
            Type::Path(p) => {
                if self.is_pod(&TypeName::from_type_path(p)) {
                    ArgumentConversion::new_unconverted(ty.clone())
                } else {
                    conversion_direction(ty.clone())
                }
            }
            _ => ArgumentConversion::new_unconverted(ty.clone()),
        }
    }

    fn argument_conversion_details(&self, ty: &Type) -> ArgumentConversion {
        self.conversion_details(ty, ArgumentConversion::new_from_unique_ptr)
    }

    fn return_type_conversion_details(&self, ty: &Type) -> ArgumentConversion {
        self.conversion_details(ty, ArgumentConversion::new_to_unique_ptr)
    }

    fn convert_return_type(
        &mut self,
        rt: &ReturnType,
        ns: &Namespace,
        convert_ptr_to_reference: bool,
    ) -> Result<ReturnTypeAnalysis, ConvertError> {
        let result = match rt {
            ReturnType::Default => ReturnTypeAnalysis {
                rt: ReturnType::Default,
                was_reference: false,
                conversion: None,
                deps: HashSet::new(),
            },
            ReturnType::Type(rarrow, boxed_type) => {
                // TODO remove the below clone
                let (boxed_type, deps, _) =
                    self.convert_boxed_type(boxed_type.clone(), ns, convert_ptr_to_reference)?;
                let was_reference = matches!(boxed_type.as_ref(), Type::Reference(_));
                let conversion = self.return_type_conversion_details(boxed_type.as_ref());
                ReturnTypeAnalysis {
                    rt: ReturnType::Type(*rarrow, boxed_type),
                    conversion: Some(conversion),
                    was_reference,
                    deps,
                }
            }
        };
        Ok(result)
    }

    fn get_bindgen_original_name_annotation(fun: &ForeignItemFn) -> Option<String> {
        fun.attrs
            .iter()
            .filter_map(|a| {
                if a.path.is_ident("bindgen_original_name") {
                    let r: Result<LitStr, syn::Error> = a.parse_args();
                    match r {
                        Ok(ls) => Some(ls.value()),
                        Err(_) => None,
                    }
                } else {
                    None
                }
            })
            .next()
    }

    fn get_reference_parameters_and_return(fun: &ForeignItemFn) -> (HashSet<Ident>, bool) {
        let mut ref_params = HashSet::new();
        let mut ref_return = false;
        for a in &fun.attrs {
            if a.path.is_ident("bindgen_ret_type_reference") {
                ref_return = true;
            } else if a.path.is_ident("bindgen_arg_type_reference") {
                let r: Result<Ident, syn::Error> = a.parse_args();
                if let Ok(ls) = r {
                    ref_params.insert(ls);
                }
            }
        }
        (ref_params, ref_return)
    }
}
