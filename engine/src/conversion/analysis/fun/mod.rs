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
pub(crate) mod function_wrapper;
mod overload_tracker;
mod rust_name_tracker;

use crate::{
    conversion::{
        analysis::{
            has_attr,
            type_converter::{add_analysis, TypeConversionContext, TypeConverter},
        },
        api::TypedefKind,
        convert_error::ConvertErrorWithContext,
        convert_error::ErrorContext,
        error_reporter::convert_apis,
    },
    known_types::known_types,
    types::validate_ident_ok_for_rust,
};
use std::collections::{HashMap, HashSet};

use autocxx_parser::{IncludeCppConfig, UnsafePolicy};
use function_wrapper::{FunctionWrapper, FunctionWrapperPayload, TypeConversionPolicy};
use proc_macro2::Span;
use syn::{
    parse_quote, punctuated::Punctuated, FnArg, ForeignItemFn, Ident, LitStr, Pat, ReturnType,
    Type, TypePtr, Visibility,
};

use crate::{
    conversion::{
        api::{AnalysisPhase, Api, ApiDetail, FuncToConvert, TypeKind, UnanalyzedApi},
        codegen_cpp::AdditionalNeed,
        ConvertError,
    },
    types::{make_ident, validate_ident_ok_for_cxx, Namespace, QualifiedName},
};

use self::{
    bridge_name_tracker::BridgeNameTracker, overload_tracker::OverloadTracker,
    rust_name_tracker::RustNameTracker,
};

use super::pod::{PodAnalysis, PodStructAnalysisBody};

pub(crate) enum MethodKind {
    Normal,
    Constructor,
    Static,
    Virtual,
    PureVirtual,
}

pub(crate) enum FnKind {
    Function,
    Method(QualifiedName, MethodKind),
}

/// Strategy for ensuring that the final, callable, Rust name
/// is what the user originally expected.
pub(crate) enum RustRenameStrategy {
    /// cxx::bridge name matches user expectations
    None,
    /// We can rename using the #[rust_name] attribute in the cxx::bridge
    RenameUsingRustAttr,
    /// Even the #[rust_name] attribute would cause conflicts, and we need
    /// to use a 'use XYZ as ABC'
    RenameInOutputMod(Ident),
}

pub(crate) struct FnAnalysisBody {
    pub(crate) cxxbridge_name: Ident,
    pub(crate) rust_name: String,
    pub(crate) rust_rename_strategy: RustRenameStrategy,
    pub(crate) params: Punctuated<FnArg, syn::Token![,]>,
    pub(crate) kind: FnKind,
    pub(crate) ret_type: ReturnType,
    pub(crate) param_details: Vec<ArgumentAnalysis>,
    pub(crate) cpp_call_name: String,
    pub(crate) requires_unsafe: bool,
    pub(crate) vis: Visibility,
    pub(crate) cpp_wrapper: Option<AdditionalNeed>,
}

pub(crate) struct ArgumentAnalysis {
    pub(crate) conversion: TypeConversionPolicy,
    pub(crate) name: Pat,
    pub(crate) self_type: Option<QualifiedName>,
    was_reference: bool,
    deps: HashSet<QualifiedName>,
    is_virtual: bool,
    requires_unsafe: bool,
}

struct ReturnTypeAnalysis {
    rt: ReturnType,
    conversion: Option<TypeConversionPolicy>,
    was_reference: bool,
    deps: HashSet<QualifiedName>,
}

pub(crate) struct FnAnalysis;

impl AnalysisPhase for FnAnalysis {
    type TypedefAnalysis = TypedefKind;
    type StructAnalysis = PodStructAnalysisBody;
    type EnumAnalysis = TypeKind;
    type FunAnalysis = FnAnalysisBody;
}

pub(crate) struct FnAnalyzer<'a> {
    unsafe_policy: UnsafePolicy,
    rust_name_tracker: RustNameTracker,
    extra_apis: Vec<UnanalyzedApi>,
    type_converter: TypeConverter<'a>,
    bridge_name_tracker: BridgeNameTracker,
    pod_safe_types: HashSet<QualifiedName>,
    config: &'a IncludeCppConfig,
    overload_trackers_by_mod: HashMap<Namespace, OverloadTracker>,
    generate_utilities: bool,
}

struct FnAnalysisResult(FnAnalysisBody, Ident, HashSet<QualifiedName>);

impl<'a> FnAnalyzer<'a> {
    pub(crate) fn analyze_functions(
        apis: Vec<Api<PodAnalysis>>,
        unsafe_policy: UnsafePolicy,
        config: &'a IncludeCppConfig,
    ) -> Vec<Api<FnAnalysis>> {
        let mut me = Self {
            unsafe_policy,
            rust_name_tracker: RustNameTracker::new(),
            extra_apis: Vec::new(),
            type_converter: TypeConverter::new(config, &apis),
            bridge_name_tracker: BridgeNameTracker::new(),
            config,
            overload_trackers_by_mod: HashMap::new(),
            pod_safe_types: Self::build_pod_safe_type_set(&apis),
            generate_utilities: Self::should_generate_utilities(&apis),
        };
        let mut results = Vec::new();
        convert_apis(apis, &mut results, |api| me.analyze_fn_api(api));
        results.extend(me.extra_apis.into_iter().map(add_analysis));
        results
    }

    fn should_generate_utilities(apis: &[Api<PodAnalysis>]) -> bool {
        apis.iter()
            .any(|api| matches!(api.detail, ApiDetail::StringConstructor))
    }

    fn build_pod_safe_type_set(apis: &[Api<PodAnalysis>]) -> HashSet<QualifiedName> {
        apis.iter()
            .filter_map(|api| match api.detail {
                ApiDetail::Struct {
                    item: _,
                    analysis:
                        PodStructAnalysisBody {
                            kind: TypeKind::Pod,
                            ..
                        },
                } => Some(api.name()),
                ApiDetail::Enum {
                    item: _,
                    analysis: TypeKind::Pod,
                } => Some(api.name()),
                _ => None,
            })
            .chain(
                known_types()
                    .get_pod_safe_types()
                    .filter_map(
                        |(tn, is_pod_safe)| {
                            if is_pod_safe {
                                Some(tn.clone())
                            } else {
                                None
                            }
                        },
                    ),
            )
            .collect()
    }

    fn analyze_fn_api(
        &mut self,
        api: Api<PodAnalysis>,
    ) -> Result<Option<Api<FnAnalysis>>, ConvertErrorWithContext> {
        let mut new_deps = api.deps.clone();
        let mut new_id = api.name.get_final_ident();
        let api_detail = match api.detail {
            // No changes to any of these...
            ApiDetail::ConcreteType {
                rs_definition,
                cpp_definition,
            } => ApiDetail::ConcreteType {
                rs_definition,
                cpp_definition,
            },
            ApiDetail::StringConstructor => ApiDetail::StringConstructor,
            ApiDetail::Function { fun, analysis: _ } => {
                let analysis = self.analyze_foreign_fn(
                    &api.name.get_namespace(),
                    &fun,
                    api.original_name.clone(),
                )?;
                match analysis {
                    None => return Ok(None),
                    Some(FnAnalysisResult(analysis, id, fn_deps)) => {
                        new_deps = fn_deps;
                        new_id = id;
                        ApiDetail::Function { fun, analysis }
                    }
                }
            }
            ApiDetail::Const { const_item } => ApiDetail::Const { const_item },
            ApiDetail::Typedef { item, analysis } => ApiDetail::Typedef { item, analysis },
            ApiDetail::CType { typename } => ApiDetail::CType { typename },
            ApiDetail::Enum { item, analysis } => ApiDetail::Enum { item, analysis },
            ApiDetail::Struct { item, analysis } => ApiDetail::Struct { item, analysis },
            ApiDetail::ForwardDeclaration => ApiDetail::ForwardDeclaration,
            ApiDetail::IgnoredItem { err, ctx } => ApiDetail::IgnoredItem { err, ctx },
        };
        Ok(Some(Api {
            name: QualifiedName::new(api.name.get_namespace(), new_id),
            original_name: api.original_name,
            deps: new_deps,
            detail: api_detail,
        }))
    }

    fn convert_boxed_type(
        &mut self,
        ty: Box<Type>,
        ns: &Namespace,
        convert_ptrs_to_references: bool,
    ) -> Result<(Box<Type>, HashSet<QualifiedName>, bool), ConvertError> {
        let annotated = self.type_converter.convert_boxed_type(
            ty,
            ns,
            &TypeConversionContext::CxxOuterType {
                convert_ptrs_to_references,
            },
        )?;
        self.extra_apis.extend(annotated.extra_apis);
        Ok((
            annotated.ty,
            annotated.types_encountered,
            annotated.requires_unsafe,
        ))
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

    fn is_on_allowlist(&self, type_name: &QualifiedName) -> bool {
        self.config.is_on_allowlist(&type_name.to_cpp_name())
    }

    fn should_be_unsafe(&self) -> bool {
        self.unsafe_policy == UnsafePolicy::AllFunctionsUnsafe
    }

    /// Determine how to materialize a function.
    ///
    /// The main job here is to determine whether a function can simply be noted
    /// in the [cxx::bridge] mod and passed directly to cxx, or if it needs a Rust-side
    /// wrapper function, or if it needs a C++-side wrapper function, or both.
    /// We aim for the simplest case but, for example:
    /// * We'll need a C++ wrapper for static methods
    /// * We'll need a C++ wrapper for parameters which need to be wrapped and unwrapped
    ///   to [UniquePtr]
    /// * We'll need a Rust wrapper if we've got a C++ wrapper and it's a method.
    /// * We may need wrappers if names conflict.
    /// etc.
    /// The other major thing we do here is figure out naming for the function.
    /// This depends on overloads, and what other functions are floating around.
    /// The output of this analysis phase is used by both Rust and C++ codegen.
    fn analyze_foreign_fn(
        &mut self,
        ns: &Namespace,
        func_information: &FuncToConvert,
        original_name: Option<String>,
    ) -> Result<Option<FnAnalysisResult>, ConvertErrorWithContext> {
        let fun = &func_information.item;
        let virtual_this = &func_information.virtual_this_type;

        // Let's gather some pre-wisdom about the name of the function.
        // We're shortly going to plunge into analyzing the parameters,
        // and it would be nice to have some idea of the function name
        // for diagnostics whilst we do that.
        let initial_rust_name = fun.sig.ident.to_string();
        if initial_rust_name.ends_with("_destructor") {
            return Ok(None);
        }
        let diagnostic_display_name = original_name.as_ref().unwrap_or(&initial_rust_name);

        // Now let's analyze all the parameters.
        // See if any have annotations which our fork of bindgen has craftily inserted...
        let (reference_params, reference_return) = Self::get_reference_parameters_and_return(&fun);
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
        let (mut params, mut param_details): (Punctuated<_, syn::Token![,]>, Vec<_>) =
            param_details.into_iter().map(Result::unwrap).unzip();

        let params_deps: HashSet<_> = param_details
            .iter()
            .map(|p| p.deps.iter().cloned())
            .flatten()
            .collect();
        let self_ty = param_details
            .iter()
            .filter_map(|pd| pd.self_type.as_ref())
            .next()
            .cloned();

        let requires_unsafe =
            self.should_be_unsafe() || param_details.iter().any(|pd| pd.requires_unsafe);

        // End of parameter processing.
        // Work out naming, part one.
        // The C++ call name will always be whatever bindgen tells us.
        let cpp_call_name = original_name
            .clone()
            .unwrap_or_else(|| initial_rust_name.clone());
        // The Rust name... it's more complicated.
        // bindgen may have mangled the name either because it's invalid Rust
        // syntax (e.g. a keyword like 'async') or it's an overload.
        // If the former, we respect that mangling. If the latter, we don't,
        // because we'll add our own overload counting mangling later.
        // Cases:
        //   function, IRN=foo,    ON=<none>                    output: foo    case 1
        //   function, IRN=move_,  ON=move   (keyword problem)  output: move_  case 2
        //   function, IRN=foo1,   ON=foo    (overload)         output: foo    case 3
        //   method,   IRN=A_foo,  ON=foo                       output: foo    case 4
        //   method,   IRN=A_move, ON=move   (keyword problem)  output: move_  case 5
        //   method,   IRN=A_foo1, ON=foo    (overload)         output: foo    case 6
        let ideal_rust_name = match original_name {
            None => initial_rust_name, // case 1
            Some(original_name) => {
                if initial_rust_name.ends_with('_') {
                    initial_rust_name // case 2
                } else if validate_ident_ok_for_rust(&original_name).is_err() {
                    format!("{}_", original_name) // case 5
                } else {
                    original_name // cases 3, 4, 6
                }
            }
        };

        // Let's spend some time figuring out the kind of this function (i.e. method,
        // virtual function, etc.)
        let (is_static_method, self_ty) = if self_ty.is_none() {
            // Even if we can't find a 'self' parameter this could conceivably
            // be a static method.
            let self_ty = func_information.self_ty.clone();
            (self_ty.is_some(), self_ty)
        } else {
            (false, self_ty)
        };

        let (kind, error_context, rust_name) = if let Some(self_ty) = self_ty {
            // Some kind of method.
            if !self.is_on_allowlist(&self_ty) {
                // Bindgen will output methods for types which have been encountered
                // virally as arguments on other allowlisted types. But we don't want
                // to generate methods unless the user has specifically asked us to.
                // It may, for instance, be a private type.
                return Ok(None);
            }
            // Method or static method.
            let type_ident = self_ty.get_final_item();
            // bindgen generates methods with the name:
            // {class}_{method name}
            // It then generates an impl section for the Rust type
            // with the original name, but we currently discard that impl section.
            // We want to feed cxx methods with just the method name, so let's
            // strip off the class name.
            let overload_tracker = self.overload_trackers_by_mod.entry(ns.clone()).or_default();
            let mut rust_name = overload_tracker.get_method_real_name(&type_ident, ideal_rust_name);
            let method_kind = if rust_name.starts_with(&type_ident) {
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
                MethodKind::Constructor
            } else if is_static_method {
                MethodKind::Static
            } else if param_details.iter().any(|pd| pd.is_virtual) {
                if has_attr(&fun.attrs, "bindgen_pure_virtual") {
                    MethodKind::PureVirtual
                } else {
                    MethodKind::Virtual
                }
            } else {
                MethodKind::Normal
            };
            let error_context = ErrorContext::Method {
                self_ty: self_ty.get_final_ident(),
                method: make_ident(&rust_name),
            };
            (
                FnKind::Method(self_ty, method_kind),
                error_context,
                rust_name,
            )
        } else {
            // Not a method.
            // What shall we call this function? It may be overloaded.
            let overload_tracker = self.overload_trackers_by_mod.entry(ns.clone()).or_default();
            let rust_name = overload_tracker.get_function_real_name(ideal_rust_name);
            (
                FnKind::Function,
                ErrorContext::Item(make_ident(&rust_name)),
                rust_name,
            )
        };

        // The name we use within the cxx::bridge mod may be different
        // from both the C++ name and the Rust name, because it's a flat
        // namespace so we might need to prepend some stuff to make it unique.
        let cxxbridge_name = self.get_cxx_bridge_name(
            match kind {
                FnKind::Method(ref self_ty, ..) => Some(self_ty.get_final_item()),
                FnKind::Function => None,
            },
            &rust_name,
            &ns,
        );
        let mut cxxbridge_name = make_ident(&cxxbridge_name);

        // If we encounter errors from here on, we can give some context around
        // where the error occurred such that we can put a marker in the output
        // Rust code to indicate that a problem occurred (benefiting people using
        // rust-analyzer or similar). Make a closure to make this easy.
        let contextualize_error = |err| ConvertErrorWithContext(err, Some(error_context));

        // Now we can add context to the error, check for a couple of error
        // cases. First, see if any of the parameters are trouble.
        if let Some(problem) = bads.into_iter().next() {
            match problem {
                Ok(_) => panic!("No error in the error"),
                Err(problem) => return Err(contextualize_error(problem)),
            }
        }
        // Second, reject any functions handling types which we flake out on.
        if has_attr(&fun.attrs, "bindgen_unused_template_param_in_arg_or_return") {
            return Err(contextualize_error(ConvertError::UnusedTemplateParam));
        }

        // Reject move constructors.
        if Self::is_move_constructor(&fun) {
            return Err(contextualize_error(
                ConvertError::MoveConstructorUnsupported,
            ));
        }

        // Analyze the return type, just as we previously did for the
        // parameters.
        let mut return_analysis = if let FnKind::Method(ref self_ty, MethodKind::Constructor) = kind
        {
            let constructed_type = self_ty.to_type_path();
            let mut these_deps = HashSet::new();
            these_deps.insert(self_ty.clone());
            ReturnTypeAnalysis {
                rt: parse_quote! {
                    -> #constructed_type
                },
                conversion: Some(TypeConversionPolicy::new_to_unique_ptr(parse_quote! {
                    #constructed_type
                })),
                was_reference: false,
                deps: these_deps,
            }
        } else {
            // We can't easily use map_err below because the borrow checker can't
            // prove we don't use contextualize_error more than once.
            let r = self.convert_return_type(&fun.sig.output, &ns, reference_return);
            match r {
                Err(err) => return Err(contextualize_error(err)),
                Ok(r) => r,
            }
        };
        let mut deps = params_deps;
        deps.extend(return_analysis.deps.drain());

        if return_analysis.was_reference {
            // cxx only allows functions to return a reference if they take exactly
            // one reference as a parameter. Let's see...
            let num_input_references = param_details.iter().filter(|pd| pd.was_reference).count();
            if num_input_references != 1 {
                return Err(contextualize_error(ConvertError::NotOneInputReference(
                    rust_name,
                )));
            }
        }
        let mut ret_type = return_analysis.rt;
        let ret_type_conversion = return_analysis.conversion;

        // Do we need to convert either parameters or return type?
        let param_conversion_needed = param_details.iter().any(|b| b.conversion.cpp_work_needed());
        let ret_type_conversion_needed = ret_type_conversion
            .as_ref()
            .map_or(false, |x| x.cpp_work_needed());
        // See https://github.com/dtolnay/cxx/issues/878 for the reason for this next line.
        let cpp_name_incompatible_with_cxx = validate_ident_ok_for_rust(&cpp_call_name).is_err();
        // If possible, we'll put knowledge of the C++ API directly into the cxx::bridge
        // mod. However, there are various circumstances where cxx can't work with the existing
        // C++ API and we need to create a C++ wrapper function which is more cxx-compliant.
        // That wrapper function is included in the cxx::bridge, and calls through to the
        // original function.
        let wrapper_function_needed = match kind {
            FnKind::Method(_, MethodKind::Static)
            | FnKind::Method(_, MethodKind::Virtual)
            | FnKind::Method(_, MethodKind::PureVirtual) => true,
            FnKind::Method(..) if cxxbridge_name != rust_name => true,
            _ if param_conversion_needed => true,
            _ if ret_type_conversion_needed => true,
            _ if cpp_name_incompatible_with_cxx => true,
            _ => false,
        };

        let cpp_wrapper = if wrapper_function_needed {
            // Generate a new layer of C++ code to wrap/unwrap parameters
            // and return values into/out of std::unique_ptrs.
            let cpp_construction_ident = make_ident(&cpp_call_name);
            let joiner = if cxxbridge_name.to_string().ends_with('_') {
                ""
            } else {
                "_"
            };
            cxxbridge_name = make_ident(&format!("{}{}autocxx_wrapper", cxxbridge_name, joiner));
            let (payload, has_receiver) = match kind {
                FnKind::Method(_, MethodKind::Constructor) => {
                    (FunctionWrapperPayload::Constructor, false)
                }
                FnKind::Method(ref self_ty, MethodKind::Static) => (
                    FunctionWrapperPayload::StaticMethodCall(
                        ns.clone(),
                        self_ty.get_final_ident(),
                        cpp_construction_ident,
                    ),
                    false,
                ),
                FnKind::Method(..) => (
                    FunctionWrapperPayload::FunctionCall(ns.clone(), cpp_construction_ident),
                    true,
                ),
                _ => (
                    FunctionWrapperPayload::FunctionCall(ns.clone(), cpp_construction_ident),
                    false,
                ),
            };
            // Now modify the cxx::bridge entry we're going to make.
            if let Some(ref conversion) = ret_type_conversion {
                let new_ret_type = conversion.unconverted_rust_type();
                ret_type = parse_quote!(
                    -> #new_ret_type
                );
            }

            // Amend parameters for the function which we're asking cxx to generate.
            params.clear();
            for pd in &param_details {
                let type_name = pd.conversion.converted_rust_type();
                let arg_name = if pd.self_type.is_some()
                    && !matches!(kind, FnKind::Method(_, MethodKind::Constructor))
                {
                    parse_quote!(autocxx_gen_this)
                } else {
                    pd.name.clone()
                };
                params.push(parse_quote!(
                    #arg_name: #type_name
                ));
            }

            Some(AdditionalNeed::FunctionWrapper(Box::new(FunctionWrapper {
                payload,
                wrapper_function_name: cxxbridge_name.clone(),
                return_conversion: ret_type_conversion,
                argument_conversion: param_details.iter().map(|d| d.conversion.clone()).collect(),
                is_a_method: has_receiver,
            })))
        } else {
            None
        };

        let vis = func_information.item.vis.clone();

        // Naming, part two.
        // Work out our final naming strategy.
        validate_ident_ok_for_cxx(&cxxbridge_name.to_string()).map_err(contextualize_error)?;
        let rust_name_ident = make_ident(&rust_name);
        let (id, rust_rename_strategy) = match kind {
            FnKind::Method(..) => (rust_name_ident, RustRenameStrategy::None),
            FnKind::Function => {
                // Keep the original Rust name the same so callers don't
                // need to know about all of these shenanigans.
                // There is a global space of rust_names even if they're in
                // different namespaces.
                let rust_name_ok = self.ok_to_use_rust_name(&rust_name);
                if cxxbridge_name == rust_name {
                    (rust_name_ident, RustRenameStrategy::None)
                } else if rust_name_ok {
                    (rust_name_ident, RustRenameStrategy::RenameUsingRustAttr)
                } else {
                    (
                        cxxbridge_name.clone(),
                        RustRenameStrategy::RenameInOutputMod(rust_name_ident),
                    )
                }
            }
        };

        Ok(Some(FnAnalysisResult(
            FnAnalysisBody {
                cxxbridge_name,
                rust_name,
                rust_rename_strategy,
                params,
                kind,
                ret_type,
                param_details,
                cpp_call_name,
                requires_unsafe,
                vis,
                cpp_wrapper,
            },
            id,
            deps,
        )))
    }

    fn convert_fn_arg(
        &mut self,
        arg: &FnArg,
        ns: &Namespace,
        fn_name: &str,
        virtual_this: Option<QualifiedName>,
        reference_args: &HashSet<Ident>,
    ) -> Result<(FnArg, ArgumentAnalysis), ConvertError> {
        Ok(match arg {
            FnArg::Typed(pt) => {
                let mut pt = pt.clone();
                let mut self_type = None;
                let old_pat = *pt.pat;
                let mut is_virtual = false;
                let mut treat_as_reference = false;
                let new_pat = match old_pat {
                    syn::Pat::Ident(mut pp) if pp.ident == "this" => {
                        let this_type = match pt.ty.as_ref() {
                            Type::Ptr(TypePtr {
                                elem, mutability, ..
                            }) => match elem.as_ref() {
                                Type::Path(typ) => {
                                    let mut this_type = QualifiedName::from_type_path(typ);
                                    if this_type.is_cvoid() && pp.ident == "this" {
                                        is_virtual = true;
                                        this_type = virtual_this.ok_or_else(|| {
                                            ConvertError::VirtualThisType(
                                                ns.clone(),
                                                fn_name.into(),
                                            )
                                        })?;
                                        let this_type_path = this_type.to_type_path();
                                        let const_token = if mutability.is_some() {
                                            None
                                        } else {
                                            Some(syn::Token![const](Span::call_site()))
                                        };
                                        pt.ty = Box::new(parse_quote! {
                                            * #mutability #const_token #this_type_path
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
                        validate_ident_ok_for_cxx(&pp.ident.to_string())?;
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
                        is_virtual,
                        requires_unsafe,
                    },
                )
            }
            _ => panic!("Did not expect FnArg::Receiver to be generated by bindgen"),
        })
    }

    fn argument_conversion_details(&self, ty: &Type) -> TypeConversionPolicy {
        match ty {
            Type::Path(p) => {
                let tn = QualifiedName::from_type_path(p);
                if self.pod_safe_types.contains(&tn) {
                    TypeConversionPolicy::new_unconverted(ty.clone())
                } else if known_types().convertible_from_strs(&tn) && self.generate_utilities {
                    TypeConversionPolicy::new_from_str(ty.clone())
                } else {
                    TypeConversionPolicy::new_from_unique_ptr(ty.clone())
                }
            }
            _ => TypeConversionPolicy::new_unconverted(ty.clone()),
        }
    }

    fn return_type_conversion_details(&self, ty: &Type) -> TypeConversionPolicy {
        match ty {
            Type::Path(p) => {
                let tn = QualifiedName::from_type_path(p);
                if self.pod_safe_types.contains(&tn) {
                    TypeConversionPolicy::new_unconverted(ty.clone())
                } else {
                    TypeConversionPolicy::new_to_unique_ptr(ty.clone())
                }
            }
            _ => TypeConversionPolicy::new_unconverted(ty.clone()),
        }
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

    fn get_bindgen_special_member_annotation(fun: &ForeignItemFn) -> Option<String> {
        fun.attrs
            .iter()
            .filter_map(|a| {
                if a.path.is_ident("bindgen_special_member") {
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

    fn is_move_constructor(fun: &ForeignItemFn) -> bool {
        Self::get_bindgen_special_member_annotation(fun).map_or(false, |val| val == "move_ctor")
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

impl Api<FnAnalysis> {
    pub(crate) fn typename_for_allowlist(&self) -> QualifiedName {
        match &self.detail {
            ApiDetail::Function { fun: _, analysis } => match analysis.kind {
                FnKind::Method(ref self_ty, _) => self_ty.clone(),
                FnKind::Function => {
                    QualifiedName::new(&self.name.get_namespace(), make_ident(&analysis.rust_name))
                }
            },
            _ => self.name(),
        }
    }

    /// Whether this API requires generation of additional C++, and if so,
    /// what.
    /// This seems an odd place for this function (as opposed to in the [codegen_rs]
    /// module) but, as it happens, even our Rust codegen phase needs to know if
    /// more C++ is needed (so it can add #includes in the cxx mod).
    /// And we can't answer the question _prior_ to this function analysis phase.
    pub(crate) fn additional_cpp(&self) -> Option<AdditionalNeed> {
        match &self.detail {
            ApiDetail::Function { fun: _, analysis } => analysis.cpp_wrapper.clone(),
            ApiDetail::StringConstructor => Some(AdditionalNeed::MakeStringConstructor),
            ApiDetail::ConcreteType { rs_definition, .. } => {
                Some(AdditionalNeed::ConcreteTemplatedTypeTypedef(
                    self.name.clone(),
                    rs_definition.clone(),
                ))
            }
            ApiDetail::CType { typename } => Some(AdditionalNeed::CTypeTypedef(typename.clone())),
            _ => None,
        }
    }
}
