// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use indexmap::map::IndexMap as HashMap;
use syn::FnArg;

use super::{
    fun::{
        FnAnalysis, FnKind, FnPhase, FnPrePhase2, MethodKind, PodAndConstructorAnalysis,
        TraitMethodKind,
    },
    pod::PodAnalysis,
};
use crate::conversion::{
    analysis::fun::ReceiverMutability,
    api::TypeKind,
    error_reporter::{convert_apis, convert_item_apis},
    ConvertErrorFromCpp,
};
use crate::{
    conversion::{api::Api, apivec::ApiVec},
    types::QualifiedName,
};
use indexmap::set::IndexSet as HashSet;

/// Spot types with pure virtual functions and mark them abstract.
pub(crate) fn mark_types_abstract(mut apis: ApiVec<FnPrePhase2>) -> ApiVec<FnPrePhase2> {
    // values of set are the cppname
    #[derive(Hash, PartialEq, Eq, Clone, Debug)]
    struct Signature {
        name: String,
        args: Vec<syn::Type>,
        constness: ReceiverMutability,
    }

    #[derive(Default, Debug)]
    struct ClassAbstractState {
        undefined: HashSet<Signature>,
        defined: HashSet<Signature>,
    }
    let mut class_states: HashMap<QualifiedName, ClassAbstractState> = HashMap::new();

    for api in apis.iter() {
        match &api {
            Api::Function {
                name,
                analysis:
                    FnAnalysis {
                        kind:
                            FnKind::Method {
                                impl_for: self_ty_name,
                                method_kind: MethodKind::PureVirtual(constness),
                                ..
                            },
                        params,
                        ..
                    },
                ..
            } => {
                class_states
                    .entry(self_ty_name.clone())
                    .or_default()
                    .undefined
                    .insert(Signature {
                        name: name.cpp_name(),
                        args: params
                            .iter()
                            .skip(1)
                            .filter_map(|p| {
                                if let FnArg::Typed(t) = p {
                                    Some((*t.ty).clone())
                                } else {
                                    None
                                }
                            })
                            .collect(),
                        constness: *constness,
                    });
            }
            Api::Function {
                name,
                analysis:
                    FnAnalysis {
                        kind:
                            FnKind::Method {
                                impl_for: self_ty_name,
                                method_kind: MethodKind::Virtual(constness),
                                ..
                            },
                        params,
                        ..
                    },
                ..
            } => {
                class_states
                    .entry(self_ty_name.clone())
                    .or_default()
                    .defined
                    .insert(Signature {
                        name: name.cpp_name(),
                        args: params
                            .iter()
                            .skip(1)
                            .filter_map(|p| {
                                if let FnArg::Typed(t) = p {
                                    Some((*t.ty).clone())
                                } else {
                                    None
                                }
                            })
                            .collect(),
                        constness: *constness,
                    });
            }
            _ => (),
        }
    }

    // Propagate undefined virtuals down, but remove functions that are well-defined
    // as they propagate down
    let mut iterate = true;
    while iterate {
        iterate = false;
        for api in apis.iter() {
            match api {
                Api::Struct {
                    analysis:
                        PodAndConstructorAnalysis {
                            pod:
                                PodAnalysis {
                                    bases,
                                    kind: TypeKind::Pod | TypeKind::NonPod,
                                    ..
                                },
                            ..
                        },
                    name,
                    ..
                } if bases.iter().any(|b| {
                    class_states
                        .get(b)
                        .map(|cs| !cs.undefined.is_empty())
                        .unwrap_or(false)
                }) =>
                {
                    let mut newset: HashSet<Signature> = bases
                        .iter()
                        .flat_map(|b| class_states.get(b).map(|cs| cs.undefined.iter()))
                        .flatten()
                        .cloned()
                        .collect();

                    let state = class_states.entry(name.name.clone()).or_default();

                    newset = newset
                        .into_iter()
                        .filter(|s| !state.defined.contains(s))
                        .collect();

                    // Recurse in case there are further dependent types

                    if state.undefined != newset {
                        state.undefined = newset;
                        iterate = true;
                    }
                }

                _ => {}
            }
        }
    }

    apis = apis
        .into_iter()
        .map(|api| match api {
            Api::Struct {
                analysis:
                    PodAndConstructorAnalysis {
                        pod:
                            PodAnalysis {
                                bases,
                                kind: TypeKind::Pod | TypeKind::NonPod,
                                castable_bases,
                                field_deps,
                                field_definition_deps,
                                field_info,
                                is_generic,
                                in_anonymous_namespace,
                            },
                        constructors,
                    },
                name,
                details,
            } if class_states
                .get(&name.name)
                .map(|cs| !cs.undefined.is_empty())
                .unwrap_or(false) =>
            {
                Api::Struct {
                    analysis: PodAndConstructorAnalysis {
                        pod: PodAnalysis {
                            bases,
                            kind: TypeKind::Abstract,
                            castable_bases,
                            field_deps,
                            field_definition_deps,
                            field_info,
                            is_generic,
                            in_anonymous_namespace,
                        },
                        constructors,
                    },
                    name,
                    details,
                }
            }
            api => api,
        })
        .collect();

    // We also need to remove any constructors belonging to these
    // abstract types.
    apis.retain(|api| {
        !matches!(&api,
            Api::Function {
                analysis:
                    FnAnalysis {
                        kind: FnKind::Method{impl_for: self_ty, method_kind: MethodKind::Constructor{..}, ..}
                            | FnKind::TraitMethod{ kind: TraitMethodKind::CopyConstructor | TraitMethodKind::MoveConstructor, impl_for: self_ty, ..},
                        ..
                    },
                    ..
            } if class_states.get(self_ty).map(|cs| !cs.undefined.is_empty()).unwrap_or(false)
        )
    });

    // Finally, if there are any types which are nested inside other types,
    // they can't be abstract. This is due to two small limitations in cxx.
    // Imagine we have class Foo { class Bar }
    // 1) using "type Foo = super::bindgen::root::Foo_Bar" results
    //    in the creation of std::unique_ptr code which isn't acceptable
    //    for an abtract class
    // 2) using "type Foo;" isn't possible unless Foo is a top-level item
    //    within its namespace. Any outer names will be interpreted as namespace
    //    names and result in cxx generating "namespace Foo { class Bar }"".
    let mut results = ApiVec::new();
    convert_item_apis(apis, &mut results, |api| match api {
        Api::Struct {
            analysis:
                PodAndConstructorAnalysis {
                    pod:
                        PodAnalysis {
                            kind: TypeKind::Abstract,
                            ..
                        },
                    ..
                },
            ..
        } if api
            .cpp_name()
            .as_ref()
            .map(|n| n.contains("::"))
            .unwrap_or_default() =>
        {
            Err(ConvertErrorFromCpp::AbstractNestedType)
        }
        _ => Ok(Box::new(std::iter::once(api))),
    });

    results
}

pub(crate) fn discard_ignored_functions(apis: ApiVec<FnPhase>) -> ApiVec<FnPhase> {
    // Some APIs can't be generated, e.g. because they're protected.
    // Now we've finished analyzing abstract types and constructors, we'll
    // convert them to IgnoredItems.
    let mut apis_new = ApiVec::new();
    convert_apis(
        apis,
        &mut apis_new,
        |name, fun, analysis| {
            analysis.ignore_reason.clone()?;
            Ok(Box::new(std::iter::once(Api::Function {
                name,
                fun,
                analysis,
            })))
        },
        Api::struct_unchanged,
        Api::enum_unchanged,
        Api::typedef_unchanged,
    );
    apis_new
}
