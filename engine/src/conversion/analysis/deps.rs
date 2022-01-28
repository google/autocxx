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

use itertools::Itertools;

use crate::{
    conversion::api::{Api, TypeKind},
    types::QualifiedName,
};

use super::{
    fun::{FnPhase, FnPrePhase, PodAndDepAnalysis},
    pod::PodAnalysis,
    tdef::TypedefAnalysis,
};

pub(crate) trait HasDependencies {
    fn name(&self) -> &QualifiedName;
    fn deps(&self) -> Box<dyn Iterator<Item = &QualifiedName> + '_>;

    fn format_deps(&self) -> String {
        self.deps().join(",")
    }
}

impl HasDependencies for Api<FnPrePhase> {
    fn deps(&self) -> Box<dyn Iterator<Item = &QualifiedName> + '_> {
        match self {
            Api::Typedef {
                old_tyname,
                analysis: TypedefAnalysis { deps, .. },
                ..
            } => Box::new(old_tyname.iter().chain(deps.iter())),
            Api::Struct {
                analysis:
                    PodAnalysis {
                        kind: TypeKind::Pod,
                        field_types,
                        ..
                    },
                ..
            } => Box::new(field_types.iter()),
            Api::Function { analysis, .. } => Box::new(analysis.deps.iter()),
            Api::Subclass {
                name: _,
                superclass,
            } => Box::new(std::iter::once(superclass)),
            Api::RustSubclassFn { details, .. } => Box::new(details.dependencies.iter()),
            _ => Box::new(std::iter::empty()),
        }
    }

    fn name(&self) -> &QualifiedName {
        self.name()
    }
}

impl HasDependencies for Api<FnPhase> {
    /// Any dependencies on other APIs which this API has.
    fn deps(&self) -> Box<dyn Iterator<Item = &QualifiedName> + '_> {
        match self {
            Api::Typedef {
                old_tyname,
                analysis: TypedefAnalysis { deps, .. },
                ..
            } => Box::new(old_tyname.iter().chain(deps.iter())),
            Api::Struct {
                analysis:
                    PodAndDepAnalysis {
                        pod:
                            PodAnalysis {
                                kind: TypeKind::Pod,
                                field_types,
                                ..
                            },
                        constructor_and_allocator_deps,
                    },
                ..
            } => Box::new(
                field_types
                    .iter()
                    .chain(constructor_and_allocator_deps.iter()),
            ),
            Api::Struct {
                analysis:
                    PodAndDepAnalysis {
                        constructor_and_allocator_deps,
                        ..
                    },
                ..
            } => Box::new(constructor_and_allocator_deps.iter()),
            Api::Function { analysis, .. } => Box::new(analysis.deps.iter()),
            Api::Subclass {
                name: _,
                superclass,
            } => Box::new(std::iter::once(superclass)),
            Api::RustSubclassFn { details, .. } => Box::new(details.dependencies.iter()),
            _ => Box::new(std::iter::empty()),
        }
    }

    fn name(&self) -> &QualifiedName {
        self.name()
    }
}
