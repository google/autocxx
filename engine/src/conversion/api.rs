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

use crate::types::QualifiedName;
use itertools::Itertools;
use std::collections::HashSet;
use syn::{
    ForeignItemFn, Ident, ImplItem, ItemConst, ItemEnum, ItemStruct, ItemType, ItemUse, Type,
};

use super::{
    convert_error::{ConvertErrorWithContext, ErrorContext},
    ConvertError,
};

#[derive(Copy, Clone, Eq, PartialEq)]
pub(crate) enum TypeKind {
    Pod,    // trivial. Can be moved and copied in Rust.
    NonPod, // has destructor or non-trivial move constructors. Can only hold by UniquePtr
    Abstract, // has pure virtual members - can't even generate UniquePtr.
            // It's possible that the type itself isn't pure virtual, but it inherits from
            // some other type which is pure virtual. Alternatively, maybe we just don't
            // know if the base class is pure virtual because it wasn't on the allowlist,
            // in which case we'll err on the side of caution.
}

impl TypeKind {
    pub(crate) fn can_be_instantiated(&self) -> bool {
        match self {
            TypeKind::Pod | TypeKind::NonPod => true,
            TypeKind::Abstract => false,
        }
    }
}

/// An entry which needs to go into an `impl` block for a given type.
pub(crate) struct ImplBlockDetails {
    pub(crate) item: ImplItem,
    pub(crate) ty: Ident,
}
/// A ForeignItemFn with a little bit of context about the
/// type which is most likely to be 'this'
#[derive(Clone)]
pub(crate) struct FuncToConvert {
    pub(crate) item: ForeignItemFn,
    pub(crate) virtual_this_type: Option<QualifiedName>,
    pub(crate) self_ty: Option<QualifiedName>,
}

/// Layers of analysis which may be applied to decorate each API.
/// See description of the purpose of this trait within `Api`.
pub(crate) trait AnalysisPhase {
    type TypedefAnalysis;
    type StructAnalysis;
    type FunAnalysis;
}

/// No analysis has been applied to this API.
pub(crate) struct NullAnalysis;

impl AnalysisPhase for NullAnalysis {
    type TypedefAnalysis = ();
    type StructAnalysis = ();
    type FunAnalysis = ();
}

#[derive(Clone)]
pub(crate) enum TypedefKind {
    Use(ItemUse),
    Type(ItemType),
}

#[derive(strum_macros::Display)]
/// Different types of API we might encounter.
/// This derives from [strum_macros::Display] because we want to be
/// able to debug-print the enum discriminant without worrying about
/// the fact that their payloads may not be `Debug` or `Display`.
/// (Specifically, allowing `syn` Types to be `Debug` requires
/// enabling syn's `extra-traits` feature which increases compile time.)
pub(crate) enum ApiDetail<T: AnalysisPhase> {
    /// A forward declared type for which no definition is available.
    ForwardDeclaration,
    /// A synthetic type we've manufactured in order to
    /// concretize some templated C++ type.
    ConcreteType {
        rs_definition: Box<Type>,
        cpp_definition: String,
    },
    /// A simple note that we want to make a constructor for
    /// a `std::string` on the heap.
    StringConstructor,
    /// A function. May include some analysis.
    Function {
        fun: Box<FuncToConvert>,
        analysis: T::FunAnalysis,
    },
    /// A constant.
    Const { const_item: ItemConst },
    /// A typedef found in the bindgen output which we wish
    /// to pass on in our output
    Typedef {
        item: TypedefKind,
        analysis: T::TypedefAnalysis,
    },
    /// An enum encountered in the
    /// `bindgen` output.
    Enum { item: ItemEnum },
    /// A struct encountered in the
    /// `bindgen` output.
    Struct {
        item: ItemStruct,
        analysis: T::StructAnalysis,
    },
    /// A variable-length C integer type (e.g. int, unsigned long).
    CType { typename: QualifiedName },
    /// Some item which couldn't be processed by autocxx for some reason.
    /// We will have emitted a warning message about this, but we want
    /// to mark that it's ignored so that we don't attempt to process
    /// dependent items.
    IgnoredItem {
        err: ConvertError,
        ctx: ErrorContext,
    },
}

/// Any API we encounter in the input bindgen rs which we might want to pass
/// onto the output Rust or C++.
///
/// This type is parameterized over an `ApiAnalysis`. This is any additional
/// information which we wish to apply to our knowledge of our APIs later
/// during analysis phases. It might be a excessively traity to parameterize
/// this type; we might be better off relying on an `Option<SomeKindOfAnalysis>`
/// but for now it's working.
///
/// This is not as high-level as the equivalent types in `cxx` or `bindgen`,
/// because sometimes we pass on the `bindgen` output directly in the
/// Rust codegen output.
pub(crate) struct Api<T: AnalysisPhase> {
    /// The name by which we refer to this within Rust.
    pub(crate) name: QualifiedName,
    /// The C++ name, if it's different.
    pub(crate) cpp_name: Option<String>,
    /// Any dependencies of this API, such that during garbage collection
    /// we can ensure to keep them.
    pub(crate) deps: HashSet<QualifiedName>,
    /// Details of this specific API kind.
    pub(crate) detail: ApiDetail<T>,
}

impl<T: AnalysisPhase> std::fmt::Debug for Api<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (kind={}, deps={})",
            self.name.to_cpp_name(),
            self.detail,
            self.deps.iter().map(|d| d.to_cpp_name()).join(", ")
        )
    }
}

pub(crate) type UnanalyzedApi = Api<NullAnalysis>;

macro_rules! make_unchanged {
    ($func_name:ident, $analysis:ident, $detail:ident, $fields:tt) => {
        pub(crate) fn $func_name<U>(self) -> Result<Option<Api<U>>, ConvertErrorWithContext>
        where
            U: AnalysisPhase<$analysis = T::$analysis>
        {
            let detail = match self.detail {
                ApiDetail::$detail $fields => ApiDetail::$detail $fields,
                _ => panic!("Applying identity mapping to the wrong type")
            };
            Ok(Some(Api {
                detail,
                name: self.name,
                cpp_name: self.cpp_name,
                deps: self.deps,
            }))
        }
    };
}

impl<T: AnalysisPhase> Api<T> {
    pub(crate) fn name(&self) -> QualifiedName {
        self.name.clone()
    }

    pub(crate) fn cxx_name(&self) -> &str {
        self.cpp_name
            .as_deref()
            .unwrap_or_else(|| self.name.get_final_item())
    }

    pub(crate) fn map<U, FF, SF, TF>(
        self,
        func_conversion: FF,
        struct_conversion: SF,
        typedef_conversion: TF,
    ) -> Result<Option<Api<U>>, ConvertErrorWithContext>
    where
        U: AnalysisPhase,
        FF: FnOnce(Api<T>) -> Result<Option<Api<U>>, ConvertErrorWithContext>,
        SF: FnOnce(Api<T>) -> Result<Option<Api<U>>, ConvertErrorWithContext>,
        TF: FnOnce(Api<T>) -> Result<Option<Api<U>>, ConvertErrorWithContext>,
    {
        let detail = match self.detail {
            // No changes to any of these...
            ApiDetail::ConcreteType {
                rs_definition,
                cpp_definition,
            } => ApiDetail::ConcreteType {
                rs_definition,
                cpp_definition,
            },
            ApiDetail::ForwardDeclaration => ApiDetail::ForwardDeclaration,
            ApiDetail::StringConstructor => ApiDetail::StringConstructor,
            ApiDetail::Const { const_item } => ApiDetail::Const { const_item },
            ApiDetail::CType { typename } => ApiDetail::CType { typename },
            ApiDetail::IgnoredItem { err, ctx } => ApiDetail::IgnoredItem { err, ctx },
            ApiDetail::Enum { item } => ApiDetail::Enum { item },
            // Apply a mapping to the following
            ApiDetail::Typedef { .. } => return typedef_conversion(self),
            ApiDetail::Function { .. } => return func_conversion(self),
            ApiDetail::Struct { .. } => return struct_conversion(self),
        };
        Ok(Some(Api {
            detail,
            name: self.name,
            cpp_name: self.cpp_name,
            deps: self.deps,
        }))
    }

    make_unchanged!(typedef_unchanged, TypedefAnalysis, Typedef, { item, analysis });
    make_unchanged!(struct_unchanged, StructAnalysis, Struct, { item, analysis });
}
