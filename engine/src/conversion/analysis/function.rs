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
use syn::{FnArg, ForeignItemFn, Ident, LitStr, Pat, ReturnType, Type, TypePtr, Visibility, parse_quote, punctuated::Punctuated};
use crate::{conversion::{ConvertError, api::{Api, ApiAnalysis, ApiDetail, TypeKind, UnanalyzedApi}, codegen_cpp::function_wrapper::ArgumentConversion, parse::type_converter::{TypeConverter}}, types::{Namespace, TypeName}};
use super::{bridge_name_tracker::BridgeNameTracker, overload_tracker::OverloadTracker, pod::PodAnalysis, rust_name_tracker::RustNameTracker};
use crate::types::make_ident;

pub(crate) struct FnMaterialization {
    pub(crate) cxxbridge_name: Ident,
    pub(crate) rust_name: String,
    pub(crate) is_a_method: bool,
    pub(crate) ret_type: ReturnType,
    pub(crate) is_constructor: bool,
    pub(crate) wrapper_function_needed: bool,
    pub(crate) requires_unsafe: bool,
    pub(crate) vis: Visibility,
    pub(crate) cpp_call_name: String,
    pub(crate) return_analysis: ReturnTypeAnalysis,
    pub(crate) param_details: Vec<ArgumentAnalysis>,
    pub(crate) is_static_method: bool,
    pub(crate) ret_type_conversion: Option<ArgumentConversion>,
    pub(crate) params: Punctuated<FnArg, syn::Token![,]>,
}
pub(crate) struct ArgumentAnalysis {
    pub(crate) conversion: ArgumentConversion,
    pub(crate) name: Pat,
    pub(crate) self_type: Option<TypeName>,
    pub(crate) was_reference: bool,
    pub(crate) deps: HashSet<TypeName>,
    pub(crate) virtual_this_encountered: bool,
    pub(crate) requires_unsafe: bool,
}

struct ReturnTypeAnalysis {
    pub(crate) rt: ReturnType,
    pub(crate) conversion: Option<ArgumentConversion>,
    pub(crate) was_reference: bool,
    pub(crate) deps: HashSet<TypeName>,
}

pub(crate) struct FnAnalysis;

impl ApiAnalysis for FnAnalysis {
    type TypeAnalysis = TypeKind;
    type FnAnalysis = FnMaterialization;
}

struct PodTracker; // TODO complete

struct Ctx<'a> {
    bridge_name_tracker: BridgeNameTracker,
    rust_name_tracker: RustNameTracker,
    pod_tracker: PodTracker,
    forward_declarations: HashSet<TypeName>,
    type_converter: &'a mut TypeConverter,
    extra_apis: Vec<UnanalyzedApi>, // TODO actually handle somehow
}

impl<'a> Ctx<'a> {
    fn avoid_generating_type(&self, tn: &TypeName) -> bool {
        self.forward_declarations.contains(tn)
            // TODO || is_on_blocklist
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
    fn is_pod(&self, tn: &TypeName) -> bool {
        false // TODO
    }
    fn is_on_allowlist(&self, tn: &TypeName) -> bool {
        false // TODO
    }
    fn should_be_unsafe(&self) -> bool {
        false // TODO
    }
}

struct ModCtx<'a,'b> {
    ctx: &'a mut Ctx<'b>,
    overload_tracker: OverloadTracker,
}

pub(crate) fn analyze_functions(apis: Vec<Api<PodAnalysis>>, type_converter: &mut TypeConverter) -> Result<Vec<Api<FnAnalysis>>,ConvertError> {
    let mut ctx = Ctx {
        bridge_name_tracker: BridgeNameTracker::new(),
        rust_name_tracker: RustNameTracker::new(),
        pod_tracker: PodTracker,
        forward_declarations: HashSet::new(), // TODO fill in
        type_converter,
        extra_apis: Vec::new(),
    };
    let mut mod_ctx = ModCtx {
        ctx: &mut ctx,
        overload_tracker: OverloadTracker::new(),
    };
    apis.into_iter().filter_map(|api| {
        match api.detail {
            ApiDetail::Function{ref item, ref virtual_this_type, ref self_ty, analysis} => {
                analyze_function(api, item, virtual_this_type, self_ty, &mut mod_ctx)
            }
            ApiDetail::ConcreteType(_) => {}
            ApiDetail::StringConstructor => {}
            ApiDetail::ImplEntry { impl_entry } => {}
            ApiDetail::Const { const_item } => {}
            ApiDetail::Typedef { type_item } => {}
            ApiDetail::Type { ty_details, for_extern_c_ts, is_forward_declaration, bindgen_mod_item, analysis } => {}
            ApiDetail::CType { id } => {}
        }
    }).collect()
    // TODO sort by namespace
    // TODO fill out the rest
}

fn analyze_function(api: Api<PodAnalysis>, fun: &ForeignItemFn, virtual_this: &Option<TypeName>, static_self_ty: &Option<TypeName>, mod_ctx: &mut ModCtx) -> Result<Option<Api<FnAnalysis>>,ConvertError> {

    let ns = api.ns.clone();
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

    let original_name = get_bindgen_original_name_annotation(&fun);
    let (reference_params, reference_return) = get_reference_parameters_and_return(&fun);
    let diagnostic_display_name = original_name.as_ref().unwrap_or(&initial_rust_name);

    // Now let's analyze all the parameters.
    let (param_details, bads): (Vec<_>, Vec<_>) = fun
        .sig
        .inputs
        .into_iter()
        .map(|i| {
            convert_fn_arg(
                i,
                &api.ns,
                mod_ctx,
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
        self_ty = static_self_ty.as_ref().cloned();
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
        if !mod_ctx.ctx.is_on_allowlist(&self_ty) {
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
        rust_name = mod_ctx.overload_tracker
            .get_method_real_name(&type_ident, ideal_rust_name);
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
        rust_name = mod_ctx.overload_tracker
            .get_function_real_name(ideal_rust_name);
    }

    // The name we use within the cxx::bridge mod may be different
    // from both the C++ name and the Rust name, because it's a flat
    // namespace so we might need to prepend some stuff to make it unique.
    let cxxbridge_name = mod_ctx.ctx.bridge_name_tracker.get_unique_cxx_bridge_name(
        self_ty.as_ref().map(|ty| ty.get_final_ident()),
        &rust_name,
        &api.ns,
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
        convert_return_type(mod_ctx, fun.sig.output, &api.ns, reference_return)?
    };
    let mut deps = params_deps;
    deps.extend(return_analysis.deps.drain());
    if deps.iter().any(|tn| mod_ctx.ctx.avoid_generating_type(tn)) {
        log::info!(
            "Skipping function {} due to return type or parameter being on blocklist or because only a forward declaration was encountered",
            rust_name
        );
        return Ok(None); // TODO think about how to inform user about this. Consider a more precise reason too.
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
            return Ok(None); // TODO think about how to inform user about this
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
    let requires_unsafe = requires_unsafe || mod_ctx.ctx.should_be_unsafe();

    FnMaterialization {
        cxxbridge_name,
        rust_name,
        is_a_method,
        ret_type,
        is_constructor,
        wrapper_function_needed,
        requires_unsafe,
        vis: fun.vis,
        return_analysis,
        param_details,
        cpp_call_name,
        is_static_method,
        ret_type_conversion,
        params,
    };

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

/// Returns additionally a Boolean indicating whether an argument was
/// 'this' and another one indicating whether we took a type by value
/// and that type was non-trivial.
fn convert_fn_arg(
    arg: FnArg,
    ns: &Namespace,
    mod_ctx: &mut ModCtx,
    fn_name: &str,
    virtual_this: Option<TypeName>,
    reference_args: &HashSet<Ident>,
) -> Result<(FnArg, ArgumentAnalysis), ConvertError> {
    Ok(match arg {
        FnArg::Typed(mut pt) => {
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
                mod_ctx.ctx.convert_boxed_type(pt.ty, ns, treat_as_reference)?;
            let was_reference = matches!(new_ty.as_ref(), Type::Reference(_));
            let conversion = argument_conversion_details(&new_ty, &mod_ctx.ctx);
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

fn conversion_details<F>(
    ty: &Type,
    ctx: &Ctx,
    conversion_direction: F,
) -> ArgumentConversion
where
    F: FnOnce(Type) -> ArgumentConversion,
{
    match ty {
        Type::Path(p) => {
            if ctx.is_pod(&TypeName::from_type_path(p)) {
                ArgumentConversion::new_unconverted(ty.clone())
            } else {
                conversion_direction(ty.clone())
            }
        }
        _ => ArgumentConversion::new_unconverted(ty.clone()),
    }
}

fn argument_conversion_details(
    ty: &Type,
    ctx: &Ctx,
) -> ArgumentConversion {
    conversion_details(ty, ctx, ArgumentConversion::new_from_unique_ptr)
}

fn return_type_conversion_details(
    ty: &Type,
    ctx: &Ctx,
) -> ArgumentConversion {
    conversion_details(ty, ctx, ArgumentConversion::new_to_unique_ptr)
}

fn convert_return_type(
    mod_ctx: &mut ModCtx,
    rt: ReturnType,
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
            let (boxed_type, deps, _) =
                mod_ctx.ctx.convert_boxed_type(boxed_type, ns, convert_ptr_to_reference)?;
            let was_reference = matches!(boxed_type.as_ref(), Type::Reference(_));
            let conversion =
                return_type_conversion_details(boxed_type.as_ref(), &mod_ctx.ctx);
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
