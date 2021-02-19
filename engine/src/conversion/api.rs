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

use crate::types::{Namespace, TypeName};
use proc_macro2::TokenStream;
use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
};
use syn::{ForeignItemFn, Ident, ImplItem, Item, ItemConst, ItemType};

use super::{codegen_cpp::AdditionalNeed, parse::type_converter::TypeConverter};

#[derive(Debug)]
pub enum ConvertError {
    NoContent,
    UnsafePodType(String),
    UnexpectedForeignItem,
    UnexpectedOuterItem,
    UnexpectedItemInMod,
    ComplexTypedefTarget(String),
    UnexpectedThisType(Namespace, String),
    UnsupportedBuiltInType(TypeName),
    VirtualThisType(Namespace, String),
    ConflictingTemplatedArgsWithTypedef(TypeName),
    UnacceptableParam(String),
    NotOneInputReference(String),
}

impl Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConvertError::NoContent => write!(f, "The initial run of 'bindgen' did not generate any content. This might be because none of the requested items for generation could be converted.")?,
            ConvertError::UnsafePodType(err) => write!(f, "An item was requested using 'generate_pod' which was not safe to hold by value in Rust. {}", err)?,
            ConvertError::UnexpectedForeignItem => write!(f, "Bindgen generated some unexpected code in a foreign mod section. You may have specified something in a 'generate' directive which is not currently compatible with autocxx.")?,
            ConvertError::UnexpectedOuterItem => write!(f, "Bindgen generated some unexpected code in its outermost mod section. You may have specified something in a 'generate' directive which is not currently compatible with autocxx.")?,
            ConvertError::UnexpectedItemInMod => write!(f, "Bindgen generated some unexpected code in an inner namespace mod. You may have specified something in a 'generate' directive which is not currently compatible with autocxx.")?,
            ConvertError::ComplexTypedefTarget(ty) => write!(f, "autocxx was unable to produce a typdef pointing to the complex type {}.", ty)?,
            ConvertError::UnexpectedThisType(ns, fn_name) => write!(f, "Unexpected type for 'this' in the function {}{}.", fn_name, ns.to_display_suffix())?,
            ConvertError::UnsupportedBuiltInType(ty) => write!(f, "autocxx does not yet know how to support the built-in C++ type {} - please raise an issue on github", ty.to_cpp_name())?,
            ConvertError::VirtualThisType(ns, fn_name) => write!(f, "Member function encountered where the 'this' type is 'void*', but we were unable to recognize which type that corresponds to. Function {}{}.", fn_name, ns.to_display_suffix())?,
            ConvertError::ConflictingTemplatedArgsWithTypedef(tn) => write!(f, "Type {} has templated arguments and so does the typedef to which it points", tn)?,
            ConvertError::UnacceptableParam(fn_name) => write!(f, "Function {} has a parameter or return type which is either on the blocklist or a forward declaration", fn_name)?,
            ConvertError::NotOneInputReference(fn_name) => write!(f, "Function {} has a return reference parameter, but 0 or >1 input reference parameters, so the lifetime of the output reference cannot be deduced.", fn_name)?,
        }
        Ok(())
    }
}

impl ConvertError {
    /// Whether we should ignore this error and simply skip over such items.
    /// In the future we need to use this to provide diagnostics or logging to the user,
    /// which ideally we'd somehow winkle into the generated bindings
    /// in a way that causes them a compile-time problem only if they try to
    /// _use_ the affects functions. I don't know a way to do that. Meanwhile,
    /// we should output these things as warnings during the codegen phase.
    pub(crate) fn is_ignorable(&self) -> bool {
        matches!(
            self,
            ConvertError::VirtualThisType(..)
                | ConvertError::UnsupportedBuiltInType(..)
                | ConvertError::UnacceptableParam(..)
                | ConvertError::NotOneInputReference(..)
        )
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub(crate) enum TypeKind {
    Pod,                // trivial. Can be moved and copied in Rust.
    NonPod, // has destructor or non-trivial move constructors. Can only hold by UniquePtr
    ForwardDeclaration, // no full C++ declaration available - can't even generate UniquePtr
}

/// Whether and how this type should be exposed in the mods constructed
/// for actual end-user use.
#[derive(Clone)]
pub(crate) enum Use {
    Unused,
    Used,
    UsedWithAlias(Ident),
}

/// Common details for types of API which are a type and will require
/// us to generate an ExternType.
pub(crate) struct TypeApiDetails {
    pub(crate) fulltypath: Vec<Ident>,
    pub(crate) final_ident: Ident,
    pub(crate) tynamestring: String,
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
    pub(crate) virtual_this_type: Option<TypeName>,
    pub(crate) self_ty: Option<TypeName>,
}

/// Layers of analysis which may be applied to decorate each API.
/// See description of the purpose of this trait within `Api`.
pub(crate) trait ApiAnalysis {
    type TypeAnalysis;
    type FunAnalysis;
}

/// No analysis has been applied to this API.
pub(crate) struct NullAnalysis;

impl ApiAnalysis for NullAnalysis {
    type TypeAnalysis = ();
    type FunAnalysis = ();
}

/// Different types of API we might encounter.
pub(crate) enum ApiDetail<T: ApiAnalysis> {
    /// A synthetic type we've manufactured in order to
    /// concretize some templated C++ type.
    ConcreteType(TypeApiDetails),
    /// A simple note that we want to make a constructor for
    /// a `std::string` on the heap.
    StringConstructor,
    /// A function. May include some analysis.
    Function {
        fun: FuncToConvert,
        analysis: T::FunAnalysis,
    },
    /// A constant.
    Const { const_item: ItemConst },
    /// A typedef.
    Typedef { type_item: ItemType },
    /// A type (struct or enum) encountered in the
    /// `bindgen` output.
    Type {
        ty_details: TypeApiDetails,
        for_extern_c_ts: TokenStream,
        is_forward_declaration: bool,
        bindgen_mod_item: Option<Item>,
        analysis: T::TypeAnalysis,
    },
    /// A variable-length C integer type (e.g. int, unsigned long).
    CType { id: Ident },
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
pub(crate) struct Api<T: ApiAnalysis> {
    pub(crate) ns: Namespace,
    pub(crate) id: Ident,
    pub(crate) use_stmt: Use,
    pub(crate) deps: HashSet<TypeName>,
    pub(crate) additional_cpp: Option<AdditionalNeed>,
    pub(crate) detail: ApiDetail<T>,
}

pub(crate) type UnanalyzedApi = Api<NullAnalysis>;

impl<T: ApiAnalysis> Api<T> {
    pub(crate) fn typename(&self) -> TypeName {
        TypeName::new(&self.ns, &self.id.to_string())
    }
}

/// Results of parsing the bindgen mod. This is what is passed from
/// the parser to the analysis phases.
pub(crate) struct ParseResults {
    /// All APIs encountered. This is the main thing.
    pub(crate) apis: Vec<UnanalyzedApi>,
    /// A database containing known relationships between types.
    /// In particular, any typedefs detected.
    /// This should probably be replaced by extracting this information
    /// from APIs as necessary later. TODO
    pub(crate) type_converter: TypeConverter,
    /// Any `use` statements which were found in each mod within
    /// `bindgen`. TODO future, move into `apis`
    pub(crate) use_stmts_by_mod: HashMap<Namespace, Vec<Item>>,
}
