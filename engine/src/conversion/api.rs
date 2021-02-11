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

use super::codegen_cpp::AdditionalNeed;

#[derive(Debug)]
pub enum ConvertError {
    NoContent,
    UnsafePODType(String),
    UnexpectedForeignItem,
    UnexpectedOuterItem,
    UnexpectedItemInMod,
    ComplexTypedefTarget(String),
    UnexpectedThisType(Namespace, String),
    UnsupportedBuiltInType(TypeName),
    VirtualThisType(Namespace, String),
    ConflictingTemplatedArgsWithTypedef(TypeName),
}

impl Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConvertError::NoContent => write!(f, "The initial run of 'bindgen' did not generate any content. This might be because none of the requested items for generation could be converted.")?,
            ConvertError::UnsafePODType(err) => write!(f, "An item was requested using 'generate_pod' which was not safe to hold by value in Rust. {}", err)?,
            ConvertError::UnexpectedForeignItem => write!(f, "Bindgen generated some unexpected code in a foreign mod section. You may have specified something in a 'generate' directive which is not currently compatible with autocxx.")?,
            ConvertError::UnexpectedOuterItem => write!(f, "Bindgen generated some unexpected code in its outermost mod section. You may have specified something in a 'generate' directive which is not currently compatible with autocxx.")?,
            ConvertError::UnexpectedItemInMod => write!(f, "Bindgen generated some unexpected code in an inner namespace mod. You may have specified something in a 'generate' directive which is not currently compatible with autocxx.")?,
            ConvertError::ComplexTypedefTarget(ty) => write!(f, "autocxx was unable to produce a typdef pointing to the complex type {}.", ty)?,
            ConvertError::UnexpectedThisType(ns, fn_name) => write!(f, "Unexpected type for 'this' in the function {}{}.", fn_name, ns.to_display_suffix())?,
            ConvertError::UnsupportedBuiltInType(ty) => write!(f, "autocxx does not yet know how to support the built-in C++ type {} - please raise an issue on github", ty.to_cpp_name())?,
            ConvertError::VirtualThisType(ns, fn_name) => write!(f, "Member function encountered where the 'this' type is 'void*', but we were unable to recognize which type that corresponds to. Function {}{}.", fn_name, ns.to_display_suffix())?,
            ConvertError::ConflictingTemplatedArgsWithTypedef(tn) => write!(f, "Type {} has templatd arguments and so does the typedef to which it points", tn)?,
        }
        Ok(())
    }
}

impl ConvertError {
    /// Whether we should ignore this error and simply skip over such items.
    /// In the future we need to use this to provide diagnostics or logging to the user,
    /// which ideally we'd somehow winkle into the generated bindings
    /// in a way that causes them a compile-time problem only if they try to
    /// _use_ the affects functions. I don't know a way to do that. Otherwise,
    /// we should output these things as warnings during the codegen phase. TODO.
    pub(crate) fn is_ignorable(&self) -> bool {
        matches!(self, ConvertError::VirtualThisType(_, _) | ConvertError::UnsupportedBuiltInType(_))
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub(crate) enum TypeKind {
    POD,                // trivial. Can be moved and copied in Rust.
    NonPOD, // has destructor or non-trivial move constructors. Can only hold by UniquePtr
    ForwardDeclaration, // no full C++ declaration available - can't even generate UniquePtr
}

/// Whether and how this type should be exposed in the mods constructed
/// for actual end-user use.
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

/// Layers of analysis which may be applied to decorate each API.
pub(crate) trait ApiAnalysis {
    type TypeAnalysis;
}

/// No analysis has been applied to this API.
pub(crate) struct NullAnalysis;

impl ApiAnalysis for NullAnalysis {
    type TypeAnalysis = ();
}

/// Different types of API we might encounter.
pub(crate) enum ApiDetail<T: ApiAnalysis> {
    ConcreteType(TypeApiDetails),
    StringConstructor,
    ImplEntry {
        // TODO move this to be higher level and/or
        // combine with the FunctionCall item
        impl_entry: Box<ImplItem>,
    },
    Function {
        item: ForeignItemFn,
        virtual_this_type: Option<TypeName>,
        self_ty: Option<TypeName>,
    },
    Const {
        const_item: ItemConst,
    },
    Typedef {
        type_item: ItemType,
    },
    Type {
        ty_details: TypeApiDetails,
        for_extern_c_ts: TokenStream,
        is_forward_declaration: bool,
        bindgen_mod_item: Option<Item>,
        analysis: T::TypeAnalysis,
    },
    CType {
        id: Ident,
    },
}

/// Any API we encounter in the input bindgen rs which we might want to pass
/// onto the output Rust or C++. This is not exactly a high level representation
/// of the APIs we encounter - instead, we're mostly storing snippets of Rust
/// syntax which we encountered in the bindgen mod and want to pass onto the
/// resulting Rust mods. It may be that eventually this type turns into
/// a higher level description of the APIs we find, possibly even an enum.
/// That's the approach taken by both cxx and bindgen. This gives a cleaner
/// separation between the parser and the codegen phases. However our case is
/// a bit less normal because the code we generate actually includes most of
/// the code we parse.
pub(crate) struct Api<T: ApiAnalysis> {
    pub(crate) ns: Namespace,
    pub(crate) id: Ident,
    pub(crate) use_stmt: Use,
    pub(crate) deps: HashSet<TypeName>,
    pub(crate) id_for_allowlist: Option<Ident>,
    pub(crate) additional_cpp: Option<AdditionalNeed>,
    pub(crate) detail: ApiDetail<T>,
}

pub(crate) type UnanalyzedApi = Api<NullAnalysis>;

impl<T: ApiAnalysis> Api<T> {
    pub(crate) fn typename(&self) -> TypeName {
        TypeName::new(&self.ns, &self.id.to_string())
    }

    pub(crate) fn typename_for_allowlist(&self) -> TypeName {
        let id_for_allowlist = match &self.id_for_allowlist {
            None => match &self.use_stmt {
                Use::UsedWithAlias(alias) => alias,
                _ => &self.id,
            },
            Some(id) => &id,
        };
        TypeName::new(&self.ns, &id_for_allowlist.to_string())
    }
}

/// Results of parsing the bindgen mod. This is what is passed from
/// the parser to the code generation phase.
pub(crate) struct ParseResults {
    pub(crate) apis: Vec<Api<NullAnalysis>>,
    pub(crate) use_stmts_by_mod: HashMap<Namespace, Vec<Item>>,
}
