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

use std::fmt::Display;

use syn::Ident;

use crate::types::{Namespace, TypeName};

#[derive(Debug, Clone)]
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
    UnsupportedType(String),
    UnknownType(String),
    OpaqueTypeFound,
    StaticData(String),
    InfinitelyRecursiveTypedef(TypeName),
    UnexpectedUseStatement(Option<Ident>),
    TemplatedTypeContainingNonPathArg(TypeName),
    InvalidPointee,
    DidNotGenerateAnything(String),
}

fn format_maybe_identifier(id: &Option<Ident>) -> String {
    match id {
        Some(id) => id.to_string(),
        None => "<unknown>".into(),
    }
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
            ConvertError::UnsupportedType(ty_desc) => write!(f, "Encountered type not yet supported by autocxx: {}", ty_desc)?,
            ConvertError::UnknownType(ty_desc) => write!(f, "Encountered type not yet known by autocxx: {}", ty_desc)?,
            ConvertError::OpaqueTypeFound => write!(f, "Bindgen generated an opaque type (an empty array) somewhere other than a typedef")?,
            ConvertError::StaticData(ty_desc) => write!(f, "Encountered mutable static data, not yet supported: {}", ty_desc)?,
            ConvertError::InfinitelyRecursiveTypedef(tn) => write!(f, "Encountered typedef to itself - this is a known bindgen bug: {}", tn.to_cpp_name())?,
            ConvertError::UnexpectedUseStatement(maybe_ident) => write!(f, "Unexpected 'use' statement encountered: {}", format_maybe_identifier(maybe_ident))?,
            ConvertError::TemplatedTypeContainingNonPathArg(tn) => write!(f, "Type {} was parameterized over something complex which we don't yet support", tn)?,
            ConvertError::InvalidPointee => write!(f, "Pointer pointed to something unsupported")?,
            ConvertError::DidNotGenerateAnything(directive) => write!(f, "The 'generate' or 'generate_pod' directive for '{}' did not result in any code being generated. Perhaps this was mis-spelled or you didn't qualify the name with any namespaces? Otherwise please report a bug.", directive)?,
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
    pub(super) fn is_ignorable(&self) -> bool {
        matches!(
            self,
            ConvertError::VirtualThisType(..)
                | ConvertError::UnsupportedBuiltInType(..)
                | ConvertError::UnacceptableParam(..)
                | ConvertError::NotOneInputReference(..)
                | ConvertError::UnsupportedType(..)
                | ConvertError::StaticData(..)
                | ConvertError::InfinitelyRecursiveTypedef(..)
                | ConvertError::UnexpectedUseStatement(..)
                | ConvertError::TemplatedTypeContainingNonPathArg(..)
                | ConvertError::InvalidPointee
        )
    }
}

pub(crate) struct ConvertErrorWithIdent(pub(crate) ConvertError, pub(crate) Option<Ident>);

impl std::fmt::Debug for ConvertErrorWithIdent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
