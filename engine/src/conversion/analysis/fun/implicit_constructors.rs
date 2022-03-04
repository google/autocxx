// Copyright 2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::collections::{HashMap, HashSet};

use crate::{
    conversion::{
        analysis::{depth_first::depth_first, pod::PodAnalysis, type_converter::find_types},
        api::{Api, CppVisibility, FuncToConvert, SpecialMemberKind},
        apivec::ApiVec,
    },
    known_types::known_types,
    types::QualifiedName,
};

use super::{
    implicit_constructor_rules::{
        determine_implicit_constructors, ExplicitItemsFound, ImplicitConstructorsNeeded,
    },
    FnAnalysis, FnKind, FnPrePhase, MethodKind, ReceiverMutability, TraitMethodKind,
};

#[derive(Hash, Eq, PartialEq)]
enum ExplicitKind {
    MoveConstructor,
    ConstCopyConstructor,
    NonConstCopyConstructor,
    OtherConstructor,
    Destructor,
    CopyAssignmentOperator,
    MoveAssignmentOperator,
    DeletedOrInaccessibleCopyConstructor,
    DeletedOrInaccessibleDestructor,
}

#[derive(Hash, Eq, PartialEq)]
struct ExplicitFound {
    ty: QualifiedName,
    kind: ExplicitKind,
}

/// If a type has explicit constructors, bindgen will generate corresponding
/// constructor functions, which we'll have already converted to make_unique methods.
/// For types with implicit constructors, we synthesize them here.
/// It is tempting to make this a separate analysis phase, to be run later than
/// the function analysis; but that would make the code much more complex as it
/// would need to output a `FnAnalysisBody`. By running it as part of this phase
/// we can simply generate the sort of thing bindgen generates, then ask
/// the existing code in this phase to figure out what to do with it.
pub(super) fn find_missing_constructors(
    apis: &ApiVec<FnPrePhase>,
) -> HashMap<QualifiedName, ImplicitConstructorsNeeded> {
    let mut all_known_types = find_types(apis);
    all_known_types.extend(known_types().all_names().cloned());
    let explicits = find_explicit_items(apis);
    let mut implicit_constructors_needed = HashMap::new();
    // Important only to ask for a depth-first analysis of structs, because
    // when all APIs are considered there may be reference loops and that would
    // panic.
    for api in depth_first(apis.iter().filter(|api| matches!(api, Api::Struct { .. }))) {
        if let Api::Struct {
            name,
            analysis: PodAnalysis {
                bases, field_types, ..
            },
            details,
            ..
        } = api
        {
            if name.cpp_name_if_present().is_some() {
                // For now we do not generate implicit constructors for nested structs - see
                // https://github.com/google/autocxx/issues/884
                continue;
            }
            let name = &name.name;
            let find = |kind: ExplicitKind| -> bool {
                explicits.contains(&ExplicitFound {
                    ty: name.clone(),
                    kind,
                })
            };
            let any_bases_or_fields_lack_const_copy_constructors =
                bases.iter().chain(field_types.iter()).any(|qn| {
                    let has_explicit = explicits.contains(&ExplicitFound {
                        ty: qn.clone(),
                        kind: ExplicitKind::ConstCopyConstructor,
                    });
                    let has_implicit = implicit_constructors_needed
                        .get(qn)
                        .map(|imp: &ImplicitConstructorsNeeded| imp.copy_constructor_taking_const_t)
                        .unwrap_or_default();
                    !has_explicit && !has_implicit
                });
            let any_bases_or_fields_have_deleted_or_inaccessible_copy_constructors =
                bases.iter().chain(field_types.iter()).any(|qn| {
                    explicits.contains(&ExplicitFound {
                        ty: qn.clone(),
                        kind: ExplicitKind::DeletedOrInaccessibleCopyConstructor,
                    })
                });
            let any_bases_have_deleted_or_inaccessible_destructors = bases.iter().any(|qn| {
                explicits.contains(&ExplicitFound {
                    ty: qn.clone(),
                    kind: ExplicitKind::DeletedOrInaccessibleDestructor,
                })
            });
            // Conservatively, we will not generate implicit constructors for any struct/class
            // where we don't fully understand all field types. We need to extend our knowledge
            // to understand the constructor behavior of things in known_types.rs, then we'll
            // be able to cope with types which contain strings, unique_ptrs etc.
            let any_field_or_base_not_understood = bases
                .iter()
                .chain(field_types.iter())
                .any(|qn| !all_known_types.contains(qn));
            let explicit_items_found = ExplicitItemsFound {
                move_constructor: find(ExplicitKind::MoveConstructor),
                copy_constructor: find(ExplicitKind::ConstCopyConstructor)
                    || find(ExplicitKind::NonConstCopyConstructor)
                    || find(ExplicitKind::DeletedOrInaccessibleCopyConstructor),
                any_other_constructor: find(ExplicitKind::OtherConstructor),
                any_bases_or_fields_lack_const_copy_constructors,
                any_bases_or_fields_have_deleted_or_inaccessible_copy_constructors,
                any_bases_have_deleted_or_inaccessible_destructors,
                destructor: find(ExplicitKind::Destructor)
                    || find(ExplicitKind::DeletedOrInaccessibleDestructor),
                copy_assignment_operator: find(ExplicitKind::CopyAssignmentOperator),
                move_assignment_operator: find(ExplicitKind::MoveAssignmentOperator),
                has_rvalue_reference_fields: details.has_rvalue_reference_fields,
                any_field_or_base_not_understood,
            };
            log::info!(
                "Explicit items found for {:?}: {:?}",
                name,
                explicit_items_found
            );
            let implicits = determine_implicit_constructors(explicit_items_found);
            implicit_constructors_needed.insert(name.clone(), implicits);
        }
    }
    log::info!(
        "Implicit constructors needed: {:?}",
        implicit_constructors_needed
    );
    implicit_constructors_needed
}

fn find_explicit_items(apis: &ApiVec<FnPrePhase>) -> HashSet<ExplicitFound> {
    apis.iter()
        .filter_map(|api| match api {
            Api::Function {
                analysis:
                    FnAnalysis {
                        kind: FnKind::Method(self_ty, MethodKind::Constructor),
                        ..
                    },
                ..
            } => Some(ExplicitFound {
                ty: self_ty.clone(),
                kind: ExplicitKind::OtherConstructor,
            }),
            Api::Function {
                analysis:
                    FnAnalysis {
                        kind:
                            FnKind::TraitMethod {
                                kind: TraitMethodKind::MoveConstructor,
                                impl_for,
                                ..
                            },
                        ..
                    },
                ..
            } => Some(ExplicitFound {
                ty: impl_for.clone(),
                kind: ExplicitKind::MoveConstructor,
            }),
            Api::Function {
                analysis:
                    FnAnalysis {
                        kind:
                            FnKind::TraitMethod {
                                kind: TraitMethodKind::Destructor,
                                impl_for,
                                ..
                            },
                        ..
                    },
                fun,
                ..
            } if is_deleted_or_inaccessible(fun) => Some(ExplicitFound {
                ty: impl_for.clone(),
                kind: ExplicitKind::DeletedOrInaccessibleDestructor,
            }),
            Api::Function {
                analysis:
                    FnAnalysis {
                        kind:
                            FnKind::TraitMethod {
                                kind: TraitMethodKind::Destructor,
                                impl_for,
                                ..
                            },
                        ..
                    },
                ..
            } => Some(ExplicitFound {
                ty: impl_for.clone(),
                kind: ExplicitKind::Destructor,
            }),
            Api::Function {
                analysis:
                    FnAnalysis {
                        kind:
                            FnKind::TraitMethod {
                                kind: TraitMethodKind::CopyConstructor,
                                impl_for,
                                ..
                            },
                        ..
                    },
                fun,
                ..
            } if is_deleted_or_inaccessible(fun) => Some(ExplicitFound {
                ty: impl_for.clone(),
                kind: ExplicitKind::DeletedOrInaccessibleCopyConstructor,
            }),
            Api::Function {
                analysis:
                    FnAnalysis {
                        kind:
                            FnKind::TraitMethod {
                                kind: TraitMethodKind::CopyConstructor,
                                impl_for,
                                ..
                            },
                        param_details,
                        ..
                    },
                ..
            } => {
                let receiver_mutability = &param_details
                    .iter()
                    .next()
                    .unwrap()
                    .self_type
                    .as_ref()
                    .unwrap()
                    .1;
                let kind = match receiver_mutability {
                    ReceiverMutability::Const => ExplicitKind::ConstCopyConstructor,
                    ReceiverMutability::Mutable => ExplicitKind::NonConstCopyConstructor,
                };
                Some(ExplicitFound {
                    ty: impl_for.clone(),
                    kind,
                })
            }
            Api::Function {
                fun,
                analysis:
                    FnAnalysis {
                        kind: FnKind::Method(self_ty, ..),
                        ..
                    },
                ..
            } if matches!(
                fun.special_member,
                Some(SpecialMemberKind::AssignmentOperator)
            ) =>
            {
                let is_move_assignment_operator = !fun.references.rvalue_ref_params.is_empty();
                Some(ExplicitFound {
                    ty: self_ty.clone(),
                    kind: if is_move_assignment_operator {
                        ExplicitKind::MoveAssignmentOperator
                    } else {
                        ExplicitKind::CopyAssignmentOperator
                    },
                })
            }
            _ => None,
        })
        .chain(known_type_constructors())
        .collect()
}

fn known_type_constructors() -> impl Iterator<Item = ExplicitFound> {
    known_types()
        .all_types_with_move_constructors()
        .map(|ty| ExplicitFound {
            ty,
            kind: ExplicitKind::MoveConstructor,
        })
        .chain(
            known_types()
                .all_types_with_const_copy_constructors()
                .map(|ty| ExplicitFound {
                    ty,
                    kind: ExplicitKind::ConstCopyConstructor,
                }),
        )
        .chain(
            known_types()
                .all_types_without_copy_constructors()
                .map(|ty| ExplicitFound {
                    ty,
                    kind: ExplicitKind::DeletedOrInaccessibleCopyConstructor,
                }),
        )
}

fn is_deleted_or_inaccessible(fun: &FuncToConvert) -> bool {
    fun.cpp_vis == CppVisibility::Private || fun.is_deleted
}
