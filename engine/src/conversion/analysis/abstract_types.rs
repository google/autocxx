// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use indexmap::map::IndexMap as HashMap;
use syn::{punctuated::Punctuated, token::Comma, FnArg};

use super::{
    fun::{
        FnAnalysis, FnKind, FnPhase, FnPrePhase2, MethodKind, PodAndConstructorAnalysis,
        TraitMethodKind,
    },
    pod::PodAnalysis,
};
use crate::conversion::{
    analysis::{depth_first::fields_and_bases_first, fun::ReceiverMutability},
    api::{ApiName, TypeKind},
    error_reporter::{convert_apis, convert_item_apis},
    ConvertErrorFromCpp,
};
use crate::{
    conversion::{api::Api, apivec::ApiVec},
    types::QualifiedName,
};
use indexmap::set::IndexSet as HashSet;

#[derive(Hash, PartialEq, Eq, Clone, Debug)]
struct Signature {
    name: String,
    args: Vec<syn::Type>,
    constness: ReceiverMutability,
}

impl Signature {
    fn new(
        name: &ApiName,
        params: &Punctuated<FnArg, Comma>,
        constness: ReceiverMutability,
    ) -> Self {
        Signature {
            name: name.cpp_name(),
            args: params
                .iter()
                .skip(1) // skip `this` implicit argument
                .filter_map(|p| {
                    if let FnArg::Typed(t) = p {
                        Some((*t.ty).clone())
                    } else {
                        None
                    }
                })
                .collect(),
            constness,
        }
    }
}

/// Spot types with pure virtual functions and mark them abstract.
pub(crate) fn mark_types_abstract(apis: ApiVec<FnPrePhase2>) -> ApiVec<FnPrePhase2> {
    #[derive(Default, Debug, Clone)]
    struct ClassAbstractState {
        undefined: HashSet<Signature>,
        defined: HashSet<Signature>,
    }
    let mut class_states: HashMap<QualifiedName, ClassAbstractState> = HashMap::new();
    let mut abstract_classes = HashSet::new();

    for api in apis.iter() {
        if let Api::Function {
            name,
            analysis:
                FnAnalysis {
                    kind:
                        FnKind::Method {
                            impl_for: self_ty_name,
                            method_kind,
                            ..
                        },
                    params,
                    ..
                },
            ..
        } = api
        {
            match method_kind {
                MethodKind::PureVirtual(constness) => {
                    class_states
                        .entry(self_ty_name.clone())
                        .or_default()
                        .undefined
                        .insert(Signature::new(name, params, *constness));
                }
                MethodKind::Virtual(constness) => {
                    class_states
                        .entry(self_ty_name.clone())
                        .or_default()
                        .defined
                        .insert(Signature::new(name, params, *constness));
                }
                _ => {}
            }
        }
    }

    for api in fields_and_bases_first(apis.iter()) {
        if let Api::Struct {
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
        } = api
        {
            // resolve virtuals for a class: start with new pure virtuals in this class
            let mut self_cs = class_states.get(&name.name).cloned().unwrap_or_default();

            // then add pure virtuals of bases
            for base in bases.iter() {
                if let Some(base_cs) = class_states.get(base) {
                    self_cs.undefined.extend(base_cs.undefined.iter().cloned());
                }
            }

            // then remove virtuals defined in this class
            self_cs
                .undefined
                .retain(|und| !self_cs.defined.contains(und));

            // if there are undefined functions, mark as virtual
            if !self_cs.undefined.is_empty() {
                abstract_classes.insert(name.name.clone());
            }

            // store it back so child classes can read it properly
            *class_states.entry(name.name.clone()).or_default() = self_cs;
        }
    }

    // mark abstract types as abstract
    let mut apis: ApiVec<_> = apis
        .into_iter()
        .map(|mut api| {
            if let Api::Struct { name, analysis, .. } = &mut api {
                if abstract_classes.contains(&name.name) {
                    analysis.pod.kind = TypeKind::Abstract;
                }
            }
            api
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
            } if abstract_classes.contains(self_ty)
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
