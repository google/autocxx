// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Phase which annotates constructors to say a `make_unique` is necessary.

use std::collections::HashSet;

use crate::{
    conversion::{
        api::{AnalysisPhase, Api, CppVisibility, FuncToConvert, SpecialMemberKind},
        apivec::ApiVec,
        error_reporter::convert_apis,
    },
    types::QualifiedName,
};

use super::{
    fun::{FnAnalysis, FnKind, FnPhase, MethodKind, PodAndDepAnalysis},
    tdef::TypedefAnalysis,
};

pub(crate) struct AnnotatedFnAnalysis {
    pub(crate) fun: FnAnalysis,
    pub(crate) requires_make_unique: bool,
}

pub struct AnnotatedFnPhase;

impl AnalysisPhase for AnnotatedFnPhase {
    type TypedefAnalysis = TypedefAnalysis;
    type StructAnalysis = PodAndDepAnalysis;
    type FunAnalysis = AnnotatedFnAnalysis;
}

/// Notes the functions to which our Rust codegen should add certain annotations.
pub(crate) fn add_make_uniques(apis: ApiVec<FnPhase>) -> ApiVec<AnnotatedFnPhase> {
    // Pre-assemble a list of types with known destructors, to avoid having to
    // do a O(n^2) nested loop.
    let types_with_destructors = find_types_with_destructors(&apis);
    let mut results = ApiVec::new();
    convert_apis(
        apis,
        &mut results,
        |name, fun, analysis| {
            let requires_make_unique = match analysis.kind {
                FnKind::Method {
                    method_kind: MethodKind::Constructor { .. },
                    ref impl_for,
                    ..
                } if types_with_destructors.contains(impl_for) => true,
                _ => false,
            };
            Ok(Box::new(std::iter::once(Api::Function {
                name,
                fun,
                analysis: AnnotatedFnAnalysis {
                    fun: analysis,
                    requires_make_unique,
                },
            })))
        },
        Api::struct_unchanged,
        Api::enum_unchanged,
        Api::typedef_unchanged,
    );
    results
}

fn find_types_with_destructors(apis: &ApiVec<FnPhase>) -> HashSet<QualifiedName> {
    apis.iter()
        .filter_map(|api| match api {
            Api::Function {
                fun,
                analysis:
                    FnAnalysis {
                        kind: FnKind::TraitMethod { impl_for, .. },
                        ..
                    },
                ..
            } if matches!(
                **fun,
                FuncToConvert {
                    special_member: Some(SpecialMemberKind::Destructor),
                    is_deleted: false,
                    cpp_vis: CppVisibility::Public,
                    ..
                }
            ) =>
            {
                Some(impl_for)
            }
            _ => None,
        })
        .cloned()
        .collect()
}
