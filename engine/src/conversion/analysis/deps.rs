// Copyright 2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::{
    conversion::api::{Api, TypeKind},
    types::QualifiedName,
};

use super::{
    fun::{FnPhase, FnPrePhase1, PodAndDepAnalysis},
    pod::PodAnalysis,
    tdef::TypedefAnalysis,
};

pub(crate) trait HasDependencies {
    fn deps(&self) -> Box<dyn Iterator<Item = &QualifiedName> + '_>;
}

impl HasDependencies for Api<FnPrePhase1> {
    fn deps(&self) -> Box<dyn Iterator<Item = &QualifiedName> + '_> {
        match self {
            Api::Typedef {
                old_tyname,
                analysis: TypedefAnalysis { deps, .. },
                ..
            } => Box::new(old_tyname.iter().chain(deps.iter())),
            Api::Struct {
                analysis: PodAnalysis {
                    bases, field_deps, ..
                },
                ..
            } => Box::new(field_deps.iter().chain(bases.iter())),
            Api::Function { analysis, .. } => Box::new(analysis.deps.iter()),
            Api::Subclass {
                name: _,
                superclass,
            } => Box::new(std::iter::once(superclass)),
            Api::RustSubclassFn { details, .. } => Box::new(details.dependencies.iter()),
            Api::RustFn { deps, .. } => Box::new(deps.iter()),
            _ => Box::new(std::iter::empty()),
        }
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
                                bases,
                                field_deps,
                                ..
                            },
                        constructor_and_allocator_deps,
                        ..
                    },
                ..
            } => Box::new(
                field_deps
                    .iter()
                    .chain(bases.iter())
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
            Api::RustFn { deps, .. } => Box::new(deps.iter()),
            _ => Box::new(std::iter::empty()),
        }
    }
}
