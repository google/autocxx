// Copyright 2022 Google LLC
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

use std::collections::HashMap;

use crate::{
    conversion::{
        api::{Api, ApiName, StructDetails, TypeKind},
        convert_error::ConvertErrorWithContext,
        error_reporter::convert_apis,
    },
    types::QualifiedName,
};

use super::{
    fun::{FnAnalysis, FnKind, FnPhase, FnPrePhase, PodAndDepAnalysis, TraitMethodKind},
    pod::PodAnalysis,
};

/// We've now analyzed all functions (including both implicit and explicit
/// constructors). Decorate each struct with a note of its constructors,
/// which will later be used as edges in the garbage collection, because
/// typically any use of a type will require us to call its copy or move
/// constructor. The same applies to its alloc/free functions.
pub(crate) fn decorate_types_with_constructor_deps(
    apis: Vec<Api<FnPrePhase>>,
) -> Vec<Api<FnPhase>> {
    let mut constructors_and_allocators_by_type = find_important_constructors(&apis);
    let mut results = Vec::new();
    convert_apis(
        apis,
        &mut results,
        Api::fun_unchanged,
        |name, details, pod| {
            decorate_struct(name, details, pod, &mut constructors_and_allocators_by_type)
        },
        Api::enum_unchanged,
        Api::typedef_unchanged,
    );
    results
}

fn decorate_struct(
    name: ApiName,
    details: Box<StructDetails>,
    pod: PodAnalysis,
    constructors_and_allocators_by_type: &mut HashMap<QualifiedName, Vec<QualifiedName>>,
) -> Result<Box<dyn Iterator<Item = Api<FnPhase>>>, ConvertErrorWithContext> {
    let is_abstract = matches!(pod.kind, TypeKind::Abstract);
    let constructor_and_allocator_deps = if is_abstract || pod.is_generic {
        Vec::new()
    } else {
        constructors_and_allocators_by_type
            .remove(&name.name)
            .unwrap_or_default()
    };
    Ok(Box::new(std::iter::once(Api::Struct {
        name,
        details,
        analysis: PodAndDepAnalysis {
            pod,
            constructor_and_allocator_deps,
        },
    })))
}

fn find_important_constructors(
    apis: &[Api<FnPrePhase>],
) -> HashMap<QualifiedName, Vec<QualifiedName>> {
    let mut results: HashMap<QualifiedName, Vec<QualifiedName>> = HashMap::new();
    for api in apis {
        if let Api::Function {
            name,
            analysis:
                FnAnalysis {
                    kind:
                        FnKind::TraitMethod {
                            kind:
                                TraitMethodKind::Alloc
                                | TraitMethodKind::Dealloc
                                | TraitMethodKind::CopyConstructor
                                | TraitMethodKind::MoveConstructor,
                            impl_for,
                            ..
                        },
                    ignore_reason: Ok(_),
                    ..
                },
            ..
        } = api
        {
            results
                .entry(impl_for.clone())
                .or_default()
                .push(name.name.clone())
        }
    }
    results
}
