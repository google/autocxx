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
mod subclass;

use crate::{
    conversion::{
        analysis::{
            fun::function_wrapper::CppFunctionKind,
            type_converter::{self, add_analysis, TypeConversionContext, TypeConverter},
        },
        api::{ApiName, CppVisibility, FuncToConvert, SubclassName},
        convert_error::ConvertErrorWithContext,
        convert_error::ErrorContext,
        error_reporter::{convert_apis, report_any_error},
    },
    known_types::known_types,
    types::validate_ident_ok_for_rust,
};
use std::collections::{HashMap, HashSet};

use autocxx_parser::{IncludeCppConfig, UnsafePolicy};
use function_wrapper::{CppFunction, CppFunctionBody, TypeConversionPolicy};
use itertools::Itertools;
use proc_macro2::Span;
use syn::{
    parse_quote, punctuated::Punctuated, token::Comma, FnArg, Ident, Pat, ReturnType, Type,
    TypePtr, Visibility,
};

use crate::{
    conversion::{
        api::{AnalysisPhase, Api, TypeKind, UnanalyzedApi},
        ConvertError,
    },
    types::{make_ident, validate_ident_ok_for_cxx, Namespace, QualifiedName},
};

use self::{
    bridge_name_tracker::BridgeNameTracker,
    overload_tracker::OverloadTracker,
    rust_name_tracker::RustNameTracker,
    subclass::{
        create_subclass_constructor, create_subclass_constructor_wrapper,
        create_subclass_fn_wrapper, create_subclass_function,
    },
};

use super::{
    pod::{PodAnalysis, PodPhase},
    tdef::TypedefAnalysis,
    type_converter::Annotated,
};

#[derive(Clone, Debug)]
pub(crate) enum ReceiverMutability {
    Const,
    Mutable,
}

#[derive(Clone)]
pub(crate) enum MethodKind {
    Normal(ReceiverMutability),
    Constructor,
    Static,
    Virtual(ReceiverMutability),
    PureVirtual(ReceiverMutability),
}

#[derive(Clone)]
pub(crate) enum FnKind {
    Function,
    Method(QualifiedName, MethodKind),
}

/// Strategy for ensuring that the final, callable, Rust name
/// is what the user originally expected.
#[derive(Clone)]

pub(crate) enum RustRenameStrategy {
    /// cxx::bridge name matches user expectations
    None,
    /// We can rename using the #[rust_name] attribute in the cxx::bridge
    RenameUsingRustAttr,
    /// Even the #[rust_name] attribute would cause conflicts, and we need
    /// to use a 'use XYZ as ABC'
    RenameInOutputMod(Ident),
}

#[derive(Clone)]
pub(crate) struct FnAnalysis {
    pub(crate) cxxbridge_name: Ident,
    pub(crate) rust_name: String,
    pub(crate) rust_rename_strategy: RustRenameStrategy,
    pub(crate) params: Punctuated<FnArg, Comma>,
    pub(crate) kind: FnKind,
    pub(crate) ret_type: ReturnType,
    pub(crate) param_details: Vec<ArgumentAnalysis>,
    pub(crate) ret_conversion: Option<TypeConversionPolicy>,
    pub(crate) requires_unsafe: bool,
    pub(crate) vis: Visibility,
    pub(crate) cpp_wrapper: Option<CppFunction>,
    pub(crate) deps: HashSet<QualifiedName>,
}

#[derive(Clone)]
pub(crate) struct ArgumentAnalysis {
    pub(crate) conversion: TypeConversionPolicy,
    pub(crate) name: Pat,
    pub(crate) self_type: Option<(QualifiedName, ReceiverMutability)>,
    pub(crate) was_reference: bool,
    pub(crate) deps: HashSet<QualifiedName>,
    pub(crate) is_virtual: bool,
    pub(crate) requires_unsafe: bool,
}

struct ReturnTypeAnalysis {
    rt: ReturnType,
    conversion: Option<TypeConversionPolicy>,
    was_reference: bool,
    deps: HashSet<QualifiedName>,
}

pub(crate) struct FnPhase;

impl AnalysisPhase for FnPhase {
    type TypedefAnalysis = TypedefAnalysis;
    type StructAnalysis = PodAnalysis;
    type FunAnalysis = FnAnalysis;
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
    subclasses_by_superclass: HashMap<QualifiedName, Vec<SubclassName>>,
    has_unrepresentable_constructors: HashSet<QualifiedName>,
    nested_type_name_map: HashMap<QualifiedName, String>,
}

impl<'a> FnAnalyzer<'a> {
    pub(crate) fn analyze_functions(
        apis: Vec<Api<PodPhase>>,
        unsafe_policy: UnsafePolicy,
        config: &'a IncludeCppConfig,
    ) -> Vec<Api<FnPhase>> {
        let mut me = Self {
            unsafe_policy,
            rust_name_tracker: RustNameTracker::new(),
            extra_apis: Vec::new(),
            type_converter: TypeConverter::new(config, &apis),
            bridge_name_tracker: BridgeNameTracker::new(),
            config,
            overload_trackers_by_mod: HashMap::new(),
            pod_safe_types: Self::build_pod_safe_type_set(&apis),
            subclasses_by_superclass: subclass::subclasses_by_superclass(&apis),
            has_unrepresentable_constructors: HashSet::new(),
            nested_type_name_map: Self::build_nested_type_map(&apis),
        };
        let mut results = Vec::new();
        convert_apis(
            apis,
            &mut results,
            |name, fun, _, _| me.analyze_foreign_fn_and_subclasses(name, fun),
            Api::struct_unchanged,
            Api::enum_unchanged,
            Api::typedef_unchanged,
        );
        me.add_missing_make_uniques(&mut results);
        results.extend(me.extra_apis.into_iter().map(add_analysis));
        results
    }

    fn build_pod_safe_type_set(apis: &[Api<PodPhase>]) -> HashSet<QualifiedName> {
        apis.iter()
            .filter_map(|api| match api {
                Api::Struct {
                    analysis:
                        PodAnalysis {
                            kind: TypeKind::Pod,
                            ..
                        },
                    ..
                } => Some(api.name().clone()),
                Api::Enum { .. } => Some(api.name().clone()),
                _ => None,
            })
            .chain(
                known_types()
                    .get_pod_safe_types()
                    .filter_map(
                        |(tn, is_pod_safe)| {
                            if is_pod_safe {
                                Some(tn)
                            } else {
                                None
                            }
                        },
                    ),
            )
            .collect()
    }

    /// Builds a mapping from a qualified type name to the last 'nest'
    /// of its name, if it has multiple elements.
    fn build_nested_type_map(apis: &[Api<PodPhase>]) -> HashMap<QualifiedName, String> {
        apis.iter()
            .filter_map(|api| match api {
                Api::Struct { name, .. } | Api::Enum { name, .. } => {
                    let cpp_name = name
                        .cpp_name
                        .as_deref()
                        .unwrap_or_else(|| name.name.get_final_item());
                    cpp_name
                        .rsplit_once("::")
                        .map(|(_, suffix)| (name.name.clone(), suffix.to_string()))
                }
                _ => None,
            })
            .collect()
    }

    fn convert_boxed_type(
        &mut self,
        ty: Box<Type>,
        ns: &Namespace,
        convert_ptrs_to_references: bool,
    ) -> Result<Annotated<Box<Type>>, ConvertError> {
        let ctx = TypeConversionContext::CxxOuterType {
            convert_ptrs_to_references,
        };
        let mut annotated = self.type_converter.convert_boxed_type(ty, ns, &ctx)?;
        self.extra_apis.append(&mut annotated.extra_apis);
        Ok(annotated)
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

    /// Analyze a given function, and consider if the APIs that we
    /// find also need to be replicated in subclasses.
    fn analyze_foreign_fn_and_subclasses(
        &mut self,
        name: ApiName,
        fun: Box<FuncToConvert>,
    ) -> Result<Box<dyn Iterator<Item = Api<FnPhase>>>, ConvertErrorWithContext> {
        let maybe_analysis_and_name = self.analyze_foreign_fn(name, fun.clone())?;

        let (analysis, name) = match maybe_analysis_and_name {
            None => return Ok(Box::new(std::iter::empty())),
            Some((analysis, name)) => (analysis, name),
        };

        let mut results = Vec::new();

        // Consider whether we need to synthesize subclass items.
        match &analysis.kind {
            FnKind::Method(sup, MethodKind::Constructor) => {
                for sub in self.subclasses_by_superclass(sup) {
                    // Add a constructor to the actual subclass definition in pure C++
                    results.push(create_subclass_constructor(&sub, &analysis, sup));
                    // And consider adding an API (in Rust/cxx/maybe C++) such that we
                    // can call this from Rust.
                    let (maybe_wrap, subclass_constructor_name) =
                        create_subclass_constructor_wrapper(sub, &fun);
                    let maybe_another_api =
                        self.analyze_foreign_fn(subclass_constructor_name, maybe_wrap)?;
                    if let Some((analysis, name)) = maybe_another_api {
                        results.push(Api::Function {
                            fun: fun.clone(),
                            analysis,
                            name,
                            name_for_gc: None,
                        });
                    }
                }
            }
            FnKind::Method(
                sup,
                MethodKind::Virtual(receiver_mutability)
                | MethodKind::PureVirtual(receiver_mutability),
            ) => {
                for sub in self.subclasses_by_superclass(sup) {
                    // For each subclass, we need to create a plain-C++ method to call its superclass
                    // and a Rust/C++ bridge API to call _that_.
                    // What we're generating here is entirely about the subclass, so the
                    // superclass's namespace is irrelevant. We generate
                    // all subclasses in the root namespace.
                    let is_pure_virtual = matches!(
                        &analysis.kind,
                        FnKind::Method(_, MethodKind::PureVirtual(..))
                    );
                    let super_fn_name =
                        SubclassName::get_super_fn_name(&Namespace::new(), &analysis.rust_name);

                    results.push(create_subclass_function(
                        &sub,
                        &analysis,
                        &name,
                        receiver_mutability,
                        sup,
                        if is_pure_virtual {
                            None
                        } else {
                            Some(&super_fn_name)
                        },
                    ));

                    if !is_pure_virtual {
                        let maybe_wrap = create_subclass_fn_wrapper(sub, &super_fn_name, &fun);
                        let super_fn_name = ApiName::new_from_qualified_name(super_fn_name);
                        let maybe_another_api =
                            self.analyze_foreign_fn(super_fn_name, maybe_wrap)?;
                        if let Some((analysis, name)) = maybe_another_api {
                            results.push(Api::Function {
                                fun: fun.clone(),
                                analysis,
                                name,
                                name_for_gc: None,
                            });
                        }
                    }
                }
            }
            _ => {}
        }

        results.push(Api::Function {
            fun,
            analysis,
            name,
            name_for_gc: None,
        });

        Ok(Box::new(results.into_iter()))
    }

    /// Determine how to materialize a function.
    ///
    /// The main job here is to determine whether a function can simply be noted
    /// in the [cxx::bridge] mod and passed directly to cxx, or if it needs a Rust-side
    /// wrapper function, or if it needs a C++-side wrapper function, or both.
    /// We aim for the simplest case but, for example:
    /// * We'll need a C++ wrapper for static methods
    /// * We'll need a C++ wrapper for parameters which need to be wrapped and unwrapped
    ///   to [cxx::UniquePtr]
    /// * We'll need a Rust wrapper if we've got a C++ wrapper and it's a method.
    /// * We may need wrappers if names conflict.
    /// etc.
    /// The other major thing we do here is figure out naming for the function.
    /// This depends on overloads, and what other functions are floating around.
    /// The output of this analysis phase is used by both Rust and C++ codegen.
    fn analyze_foreign_fn(
        &mut self,
        name: ApiName,
        fun: Box<FuncToConvert>,
    ) -> Result<Option<(FnAnalysis, ApiName)>, ConvertErrorWithContext> {
        let virtual_this = &fun.virtual_this_type;
        let mut cpp_name = name.cpp_name.clone();
        let ns = name.name.get_namespace();

        // Let's gather some pre-wisdom about the name of the function.
        // We're shortly going to plunge into analyzing the parameters,
        // and it would be nice to have some idea of the function name
        // for diagnostics whilst we do that.
        let initial_rust_name = fun.ident.to_string();
        if initial_rust_name.ends_with("_destructor") {
            return Ok(None);
        }
        let diagnostic_display_name = cpp_name.as_ref().unwrap_or(&initial_rust_name);

        // Now let's analyze all the parameters.
        // See if any have annotations which our fork of bindgen has craftily inserted...
        let (param_details, bads): (Vec<_>, Vec<_>) = fun
            .inputs
            .iter()
            .map(|i| {
                self.convert_fn_arg(
                    i,
                    ns,
                    diagnostic_display_name,
                    virtual_this.clone(),
                    &fun.reference_args,
                )
            })
            .partition(Result::is_ok);
        let (mut params, mut param_details): (Punctuated<_, Comma>, Vec<_>) =
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
        // bindgen may have mangled the name either because it's invalid Rust
        // syntax (e.g. a keyword like 'async') or it's an overload.
        // If the former, we respect that mangling. If the latter, we don't,
        // because we'll add our own overload counting mangling later.
        // Cases:
        //   function, IRN=foo,    CN=<none>                    output: foo    case 1
        //   function, IRN=move_,  CN=move   (keyword problem)  output: move_  case 2
        //   function, IRN=foo1,   CN=foo    (overload)         output: foo    case 3
        //   method,   IRN=A_foo,  CN=foo                       output: foo    case 4
        //   method,   IRN=A_move, CN=move   (keyword problem)  output: move_  case 5
        //   method,   IRN=A_foo1, CN=foo    (overload)         output: foo    case 6
        let ideal_rust_name = match &cpp_name {
            None => initial_rust_name, // case 1
            Some(cpp_name) => {
                if initial_rust_name.ends_with('_') {
                    initial_rust_name // case 2
                } else if validate_ident_ok_for_rust(cpp_name).is_err() {
                    format!("{}_", cpp_name) // case 5
                } else {
                    cpp_name.to_string() // cases 3, 4, 6
                }
            }
        };

        // Let's spend some time figuring out the kind of this function (i.e. method,
        // virtual function, etc.)
        let (is_static_method, self_ty, receiver_mutability) = match self_ty {
            None => {
                // Even if we can't find a 'self' parameter this could conceivably
                // be a static method.
                let self_ty = fun.self_ty.clone();
                (self_ty.is_some(), self_ty, None)
            }
            Some((self_ty, receiver_mutability)) => {
                (false, Some(self_ty), Some(receiver_mutability))
            }
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
            let mut rust_name = overload_tracker.get_method_real_name(type_ident, ideal_rust_name);
            let nested_type_ident = self
                .nested_type_name_map
                .get(&self_ty)
                .map(|s| s.as_str())
                .unwrap_or_else(|| self_ty.get_final_item());
            let method_kind =
                if let Some(constructor_suffix) = rust_name.strip_prefix(nested_type_ident) {
                    // It's a constructor. bindgen generates
                    // fn Type(this: *mut Type, ...args)
                    // We want
                    // fn make_unique(...args) -> Type
                    // which later code will convert to
                    // fn make_unique(...args) -> UniquePtr<Type>
                    // If there are multiple constructors, bindgen generates
                    // new, new1, new2 etc. and we'll keep those suffixes.
                    rust_name = format!("make_unique{}", constructor_suffix);
                    // Strip off the 'this' arg.
                    params = params.into_iter().skip(1).collect();
                    param_details.remove(0);
                    MethodKind::Constructor
                } else if is_static_method {
                    MethodKind::Static
                } else {
                    let receiver_mutability =
                        receiver_mutability.expect("Failed to find receiver details");
                    if param_details.iter().any(|pd| pd.is_virtual) {
                        if fun.is_pure_virtual {
                            MethodKind::PureVirtual(receiver_mutability)
                        } else {
                            MethodKind::Virtual(receiver_mutability)
                        }
                    } else {
                        MethodKind::Normal(receiver_mutability)
                    }
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

        // Skip private & protected methods; but if we've a private constructor, keep
        // a note of it.
        if fun.cpp_vis != CppVisibility::Public {
            if let FnKind::Method(self_ty, MethodKind::Constructor) = &kind {
                self.has_unrepresentable_constructors
                    .insert(self_ty.clone());
            }
            return Ok(None);
        }

        // The name we use within the cxx::bridge mod may be different
        // from both the C++ name and the Rust name, because it's a flat
        // namespace so we might need to prepend some stuff to make it unique.
        let cxxbridge_name = self.get_cxx_bridge_name(
            match kind {
                FnKind::Method(ref self_ty, ..) => Some(self_ty.get_final_item()),
                FnKind::Function => None,
            },
            &rust_name,
            ns,
        );
        if cxxbridge_name != rust_name && cpp_name.is_none() {
            cpp_name = Some(rust_name.clone());
        }
        let mut cxxbridge_name = make_ident(&cxxbridge_name);

        // If we encounter errors from here on, we can give some context around
        // where the error occurred such that we can put a marker in the output
        // Rust code to indicate that a problem occurred (benefiting people using
        // rust-analyzer or similar). Make a closure to make this easy.
        let contextualize_error = |err| ConvertErrorWithContext(err, Some(error_context.clone()));

        // Now we can add context to the error, check for a couple of error
        // cases. First, see if any of the parameters are trouble.
        if let Some(problem) = bads.into_iter().next() {
            match problem {
                Ok(_) => panic!("No error in the error"),
                Err(problem) => return Err(contextualize_error(problem)),
            }
        }
        // Second, reject any functions handling types which we flake out on.
        if fun.unused_template_param {
            return Err(contextualize_error(ConvertError::UnusedTemplateParam));
        }

        match kind {
            FnKind::Method(_, MethodKind::Static) => {}
            FnKind::Method(ref self_ty, _) => {
                // Reject move constructors.
                if fun.is_move_constructor {
                    self.has_unrepresentable_constructors
                        .insert(self_ty.clone());
                    return Err(contextualize_error(
                        ConvertError::MoveConstructorUnsupported,
                    ));
                }
                if !known_types().is_cxx_acceptable_receiver(self_ty) {
                    return Err(contextualize_error(ConvertError::UnsupportedReceiver));
                }
            }
            _ => {}
        };

        // Analyze the return type, just as we previously did for the
        // parameters.
        let mut return_analysis = if let FnKind::Method(ref self_ty, MethodKind::Constructor) = kind
        {
            let constructed_type = self_ty.to_type_path();
            ReturnTypeAnalysis {
                rt: parse_quote! {
                    -> #constructed_type
                },
                conversion: Some(TypeConversionPolicy::new_to_unique_ptr(parse_quote! {
                    #constructed_type
                })),
                was_reference: false,
                deps: std::iter::once(self_ty).cloned().collect(),
            }
        } else {
            self.convert_return_type(&fun.output, ns, fun.return_type_is_reference)
                .map_err(contextualize_error)?
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
        let effective_cpp_name = cpp_name.as_ref().unwrap_or(&rust_name);
        let cpp_name_incompatible_with_cxx =
            validate_ident_ok_for_rust(effective_cpp_name).is_err();
        // If possible, we'll put knowledge of the C++ API directly into the cxx::bridge
        // mod. However, there are various circumstances where cxx can't work with the existing
        // C++ API and we need to create a C++ wrapper function which is more cxx-compliant.
        // That wrapper function is included in the cxx::bridge, and calls through to the
        // original function.
        let wrapper_function_needed = match kind {
            FnKind::Method(_, MethodKind::Static)
            | FnKind::Method(_, MethodKind::Virtual(_))
            | FnKind::Method(_, MethodKind::PureVirtual(_)) => true,
            FnKind::Method(..) if cxxbridge_name != rust_name => true,
            _ if param_conversion_needed => true,
            _ if ret_type_conversion_needed => true,
            _ if cpp_name_incompatible_with_cxx => true,
            _ => false,
        };

        let cpp_wrapper = if wrapper_function_needed {
            // Generate a new layer of C++ code to wrap/unwrap parameters
            // and return values into/out of std::unique_ptrs.
            let cpp_construction_ident = make_ident(&effective_cpp_name);
            let joiner = if cxxbridge_name.to_string().ends_with('_') {
                ""
            } else {
                "_"
            };
            cxxbridge_name = make_ident(&format!("{}{}autocxx_wrapper", cxxbridge_name, joiner));
            let (payload, has_receiver) = match kind {
                FnKind::Method(_, MethodKind::Constructor) => (CppFunctionBody::Constructor, false),
                FnKind::Method(ref self_ty, MethodKind::Static) => (
                    CppFunctionBody::StaticMethodCall(
                        ns.clone(),
                        self_ty.get_final_ident(),
                        cpp_construction_ident,
                    ),
                    false,
                ),
                FnKind::Method(..) => (
                    CppFunctionBody::FunctionCall(ns.clone(), cpp_construction_ident),
                    true,
                ),
                _ => (
                    CppFunctionBody::FunctionCall(ns.clone(), cpp_construction_ident),
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

            Some(CppFunction {
                payload,
                wrapper_function_name: cxxbridge_name.clone(),
                return_conversion: ret_type_conversion.clone(),
                argument_conversion: param_details.iter().map(|d| d.conversion.clone()).collect(),
                kind: if has_receiver {
                    CppFunctionKind::Method
                } else {
                    CppFunctionKind::Function
                },
                pass_obs_field: false,
                qualification: None,
            })
        } else {
            None
        };

        let vis = fun.vis.clone();

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

        let analysis = FnAnalysis {
            cxxbridge_name,
            rust_name: rust_name.clone(),
            rust_rename_strategy,
            params,
            ret_conversion: ret_type_conversion,
            kind,
            ret_type,
            param_details,
            requires_unsafe,
            vis,
            cpp_wrapper,
            deps,
        };
        let name = ApiName {
            cpp_name,
            name: QualifiedName::new(ns, id),
        };
        Ok(Some((analysis, name)))
    }

    fn subclasses_by_superclass(&self, sup: &QualifiedName) -> impl Iterator<Item = SubclassName> {
        match self.subclasses_by_superclass.get(sup) {
            Some(subs) => subs.clone().into_iter(),
            None => Vec::new().into_iter(),
        }
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
                                    let receiver_mutability = if mutability.is_some() {
                                        ReceiverMutability::Mutable
                                    } else {
                                        ReceiverMutability::Const
                                    };
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
                                    Ok((this_type, receiver_mutability))
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
                let annotated_type = self.convert_boxed_type(pt.ty, ns, treat_as_reference)?;
                let new_ty = annotated_type.ty;
                let subclass_holder = match &annotated_type.kind {
                    type_converter::TypeKind::SubclassHolder(holder) => Some(holder),
                    _ => None,
                };
                let conversion =
                    self.argument_conversion_details(&new_ty, &subclass_holder.cloned());
                pt.pat = Box::new(new_pat.clone());
                pt.ty = new_ty;
                (
                    FnArg::Typed(pt),
                    ArgumentAnalysis {
                        self_type,
                        name: new_pat,
                        conversion,
                        was_reference: matches!(
                            annotated_type.kind,
                            type_converter::TypeKind::Reference
                                | type_converter::TypeKind::MutableReference
                        ),
                        deps: annotated_type.types_encountered,
                        is_virtual,
                        requires_unsafe: matches!(
                            annotated_type.kind,
                            type_converter::TypeKind::Pointer
                        ),
                    },
                )
            }
            _ => panic!("Did not expect FnArg::Receiver to be generated by bindgen"),
        })
    }

    fn argument_conversion_details(
        &self,
        ty: &Type,
        is_subclass_holder: &Option<Ident>,
    ) -> TypeConversionPolicy {
        if let Some(holder_id) = is_subclass_holder {
            let subclass = SubclassName::from_holder_name(holder_id);
            return TypeConversionPolicy::box_up_subclass_holder(
                parse_quote! {
                    rust::Box<#holder_id>
                },
                subclass,
            );
        }
        match ty {
            Type::Path(p) => {
                let tn = QualifiedName::from_type_path(p);
                if self.pod_safe_types.contains(&tn) {
                    TypeConversionPolicy::new_unconverted(ty.clone())
                } else if known_types().convertible_from_strs(&tn)
                    && !self.config.exclude_utilities()
                {
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
                let annotated_type =
                    self.convert_boxed_type(boxed_type.clone(), ns, convert_ptr_to_reference)?;
                let boxed_type = annotated_type.ty;
                let was_reference = matches!(boxed_type.as_ref(), Type::Reference(_));
                let conversion = self.return_type_conversion_details(boxed_type.as_ref());
                ReturnTypeAnalysis {
                    rt: ReturnType::Type(*rarrow, boxed_type),
                    conversion: Some(conversion),
                    was_reference,
                    deps: annotated_type.types_encountered,
                }
            }
        };
        Ok(result)
    }

    /// If a type has explicit constructors, bindgen will generate corresponding
    /// constructor functions, which we'll have already converted to make_unique methods.
    /// For types with no constructors, we synthesize one here.
    /// It is tempting to make this a separate analysis phase, to be run later than
    /// the function analysis; but that would make the code much more complex as it
    /// would need to output a `FnAnalysisBody`. By running it as part of this phase
    /// we can simply generate the sort of thing bindgen generates, then ask
    /// the existing code in this phase to figure out what to do with it.
    fn add_missing_make_uniques(&mut self, apis: &mut Vec<Api<FnPhase>>) {
        if self.config.exclude_impls {
            return;
        }
        let types_without_constructors = Self::find_all_types(apis);
        // For types with private constructors, we won't have generated code for them,
        // but we equally don't want to synthesize a public constructor. The same applies
        // to other types of constructor we might skip e.g. move constructors.
        let mut types_without_constructors =
            &types_without_constructors - &self.has_unrepresentable_constructors;
        // Now subtract all the actual constructors we know of.
        for api in apis.iter() {
            if let Api::Function {
                analysis:
                    FnAnalysis {
                        kind: FnKind::Method(self_ty, MethodKind::Constructor),
                        ..
                    },
                ..
            } = api
            {
                types_without_constructors.remove(self_ty);
            }
        }
        // We are left with those types where we should synthesize a constructor.
        for self_ty in types_without_constructors {
            let ident = self_ty.get_final_ident();
            let fake_api_name = ApiName::new(self_ty.get_namespace(), ident.clone());
            let ns = self_ty.get_namespace().clone();
            let path = self_ty.to_type_path();
            let items = report_any_error(&ns, apis, || {
                self.analyze_foreign_fn_and_subclasses(
                    fake_api_name,
                    Box::new(FuncToConvert {
                        virtual_this_type: Some(self_ty.clone()),
                        self_ty: Some(self_ty),
                        ident,
                        doc_attr: None,
                        inputs: parse_quote! { this: *mut #path },
                        output: ReturnType::Default,
                        vis: parse_quote! { pub },
                        is_pure_virtual: false,
                        cpp_vis: CppVisibility::Public,
                        is_move_constructor: false,
                        unused_template_param: false,
                        return_type_is_reference: false,
                        reference_args: HashSet::new(),
                        original_name: None,
                    }),
                )
            });
            apis.extend(items.into_iter().flatten());
        }
    }

    fn find_all_types(apis: &[Api<FnPhase>]) -> HashSet<QualifiedName> {
        apis.iter()
            .filter_map(|api| match api {
                Api::Struct { .. } => Some(api.name().clone()),
                _ => None,
            })
            .collect::<HashSet<_>>()
    }
}

impl Api<FnPhase> {
    pub(crate) fn typename_for_allowlist(&self) -> QualifiedName {
        match &self {
            Api::Function {
                name_for_gc: Some(name),
                ..
            } => name.clone(),
            Api::Function { analysis, .. } => match analysis.kind {
                FnKind::Method(ref self_ty, _) => self_ty.clone(),
                FnKind::Function => {
                    QualifiedName::new(self.name().get_namespace(), make_ident(&analysis.rust_name))
                }
            },
            Api::RustSubclassFn { subclass, .. }
            | Api::RustSubclassConstructor { subclass, .. } => subclass.0.name.clone(),
            _ => self.name().clone(),
        }
    }

    /// Whether this API requires generation of additional C++.
    /// This seems an odd place for this function (as opposed to in the [codegen_cpp]
    /// module) but, as it happens, even our Rust codegen phase needs to know if
    /// more C++ is needed (so it can add #includes in the cxx mod).
    /// And we can't answer the question _prior_ to this function analysis phase.
    pub(crate) fn needs_cpp_codegen(&self) -> bool {
        match &self {
            Api::Function { analysis, .. } => analysis.cpp_wrapper.is_some(),
            Api::StringConstructor { .. }
            | Api::ConcreteType { .. }
            | Api::CType { .. }
            | Api::RustSubclassConstructor { .. }
            | Api::RustSubclassFn { .. }
            | Api::Subclass { .. } => true,
            _ => false,
        }
    }

    pub(crate) fn cxxbridge_name(&self) -> Option<Ident> {
        match self {
            Api::Function { ref analysis, .. } => Some(analysis.cxxbridge_name.clone()),
            Api::StringConstructor { .. }
            | Api::Const { .. }
            | Api::IgnoredItem { .. }
            | Api::RustSubclassConstructor { .. }
            | Api::RustSubclassFn { .. } => None,
            _ => Some(self.name().get_final_ident()),
        }
    }

    /// Any dependencies on other APIs which this API has.
    pub(crate) fn deps(&self) -> Box<dyn Iterator<Item = &QualifiedName> + '_> {
        match self {
            Api::Typedef {
                old_tyname,
                analysis: TypedefAnalysis { deps, .. },
                ..
            } => Box::new(old_tyname.iter().chain(deps.iter())),
            Api::Struct { analysis, .. } => Box::new(analysis.field_deps.iter()),
            Api::Function { analysis, .. } => Box::new(analysis.deps.iter()),
            Api::Subclass {
                name: _,
                superclass,
            } => Box::new(std::iter::once(superclass)),
            Api::RustSubclassFn { details, .. } => Box::new(details.dependency.iter()),
            _ => Box::new(std::iter::empty()),
        }
    }

    pub(crate) fn format_deps(&self) -> String {
        self.deps().join(",")
    }
}
