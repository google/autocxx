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

use crate::types::{Namespace, QualifiedName};
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

/// Information common to all types of API.
/// At the moment this includes its name information
/// and its dependency information. It's intended that the deps
/// information be stored on each individual API type, as it's not
/// truly common to all APIs. Then this type will become renamed
/// ApiName.
pub(crate) struct ApiCommon {
    pub(crate) name: QualifiedName,
    pub(crate) cpp_name: Option<String>,
}

impl ApiCommon {
    pub(crate) fn new(ns: &Namespace, id: Ident) -> Self {
        Self {
            name: QualifiedName::new(ns, id),
            cpp_name: None,
        }
    }

    pub(crate) fn new_in_root_namespace(id: Ident) -> Self {
        Self::new(&Namespace::new(), id)
    }
}

#[derive(strum_macros::Display)]
/// Different types of API we might encounter.
///
/// This type is parameterized over an `ApiAnalysis`. This is any additional
/// information which we wish to apply to our knowledge of our APIs later
/// during analysis phases.
///
/// This is not as high-level as the equivalent types in `cxx` or `bindgen`,
/// because sometimes we pass on the `bindgen` output directly in the
/// Rust codegen output.
///
/// This derives from [strum_macros::Display] because we want to be
/// able to debug-print the enum discriminant without worrying about
/// the fact that their payloads may not be `Debug` or `Display`.
/// (Specifically, allowing `syn` Types to be `Debug` requires
/// enabling syn's `extra-traits` feature which increases compile time.)
pub(crate) enum Api<T: AnalysisPhase> {
    /// A forward declared type for which no definition is available.
    ForwardDeclaration { common: ApiCommon },
    /// A synthetic type we've manufactured in order to
    /// concretize some templated C++ type.
    ConcreteType {
        common: ApiCommon,
        rs_definition: Box<Type>,
        cpp_definition: String,
    },
    /// A simple note that we want to make a constructor for
    /// a `std::string` on the heap.
    StringConstructor { common: ApiCommon },
    /// A function. May include some analysis.
    Function {
        common: ApiCommon,
        fun: Box<FuncToConvert>,
        analysis: T::FunAnalysis,
    },
    /// A constant.
    Const {
        common: ApiCommon,
        const_item: ItemConst,
    },
    /// A typedef found in the bindgen output which we wish
    /// to pass on in our output
    Typedef {
        common: ApiCommon,
        item: TypedefKind,
        old_tyname: Option<QualifiedName>,
        analysis: T::TypedefAnalysis,
    },
    /// An enum encountered in the
    /// `bindgen` output.
    Enum { common: ApiCommon, item: ItemEnum },
    /// A struct encountered in the
    /// `bindgen` output.
    Struct {
        common: ApiCommon,
        item: ItemStruct,
        analysis: T::StructAnalysis,
    },
    /// A variable-length C integer type (e.g. int, unsigned long).
    CType {
        common: ApiCommon,
        typename: QualifiedName,
    },
    /// Some item which couldn't be processed by autocxx for some reason.
    /// We will have emitted a warning message about this, but we want
    /// to mark that it's ignored so that we don't attempt to process
    /// dependent items.
    IgnoredItem {
        common: ApiCommon,
        err: ConvertError,
        ctx: ErrorContext,
    },
}

impl<T: AnalysisPhase> Api<T> {
    pub(crate) fn common(&self) -> &ApiCommon {
        match self {
            Api::ForwardDeclaration { common } => common,
            Api::ConcreteType { common, .. } => common,
            Api::StringConstructor { common } => common,
            Api::Function { common, .. } => common,
            Api::Const { common, .. } => common,
            Api::Typedef { common, .. } => common,
            Api::Enum { common, .. } => common,
            Api::Struct { common, .. } => common,
            Api::CType { common, .. } => common,
            Api::IgnoredItem { common, .. } => common,
        }
    }

    /// The name of this API as used in Rust code.
    /// For types, it's important that this never changes, since
    /// functions or other types may refer to this.
    /// Yet for functions, this may not actually be the name
    /// used in the [cxx::bridge] mod -  see
    /// [Api<FnAnalysis>::cxxbridge_name]
    pub(crate) fn name(&self) -> &QualifiedName {
        &self.common().name
    }

    /// The name recorded for use in C++, if and only if
    /// it differs from Rust.
    pub(crate) fn cpp_name(&self) -> &Option<String> {
        &self.common().cpp_name
    }

    /// The name for use in C++, whether or not it differs
    /// from Rust.
    pub(crate) fn effective_cpp_name(&self) -> &str {
        self.cpp_name()
            .as_deref()
            .unwrap_or_else(|| self.name().get_final_item())
    }
}

impl<T: AnalysisPhase> std::fmt::Debug for Api<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (kind={})", self.name().to_cpp_name(), self,)
    }
}

pub(crate) type UnanalyzedApi = Api<NullAnalysis>;

macro_rules! make_unchanged {
    ($func_name:ident, $analysis:ident, $detail:ident, $fields:tt) => {
        pub(crate) fn $func_name<U>(self) -> Result<Option<Api<U>>, ConvertErrorWithContext>
        where
            U: AnalysisPhase<$analysis = T::$analysis>
        {
            Ok(Some(match self {
                Api::$detail $fields => Api::$detail $fields,
                _ => panic!("Applying identity mapping to the wrong type")
            }))
        }
    };
}

impl<T: AnalysisPhase> Api<T> {
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
        Ok(Some(match self {
            // No changes to any of these...
            Api::ConcreteType {
                common,
                rs_definition,
                cpp_definition,
            } => Api::ConcreteType {
                common,
                rs_definition,
                cpp_definition,
            },
            Api::ForwardDeclaration { common } => Api::ForwardDeclaration { common },
            Api::StringConstructor { common } => Api::StringConstructor { common },
            Api::Const { common, const_item } => Api::Const { common, const_item },
            Api::CType { common, typename } => Api::CType { common, typename },
            Api::IgnoredItem { common, err, ctx } => Api::IgnoredItem { common, err, ctx },
            Api::Enum { common, item } => Api::Enum { common, item },
            // Apply a mapping to the following
            Api::Typedef { .. } => return typedef_conversion(self),
            Api::Function { .. } => return func_conversion(self),
            Api::Struct { .. } => return struct_conversion(self),
        }))
    }

    make_unchanged!(typedef_unchanged, TypedefAnalysis, Typedef, { common, item, old_tyname, analysis });
    make_unchanged!(struct_unchanged, StructAnalysis, Struct, { common, item, analysis });
}
