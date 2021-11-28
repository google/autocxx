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

use crate::{
    conversion::api::SubclassName,
    types::{Namespace, QualifiedName},
};
use syn::{parse_quote, Ident, Type};

#[derive(Clone, Debug)]
pub(crate) enum CppConversionType {
    None,
    FromUniquePtrToValue,
    FromValueToUniquePtr,
}

impl CppConversionType {
    fn inverse(&self) -> Self {
        match self {
            CppConversionType::None => CppConversionType::None,
            CppConversionType::FromUniquePtrToValue => CppConversionType::FromValueToUniquePtr,
            CppConversionType::FromValueToUniquePtr => CppConversionType::FromUniquePtrToValue,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum RustConversionType {
    None,
    FromStr,
    ToBoxedUpHolder(SubclassName),
}

/// A policy for converting types. Conversion may occur on both the Rust and
/// C++ side. The most complex example is a C++ function which takes
/// std::string by value, which might do this:
/// * Client Rust code: `&str`
/// * Rust wrapper function: converts `&str` to `UniquePtr<CxxString>`
/// * cxx::bridge mod: refers to `UniquePtr<CxxString>`
/// * C++ wrapper function converts `std::unique_ptr<std::string>` to just
///   `std::string`
/// * Finally, the actual C++ API receives a `std::string` by value.
/// The implementation here is distributed across this file, and
/// `function_wrapper_rs` and `function_wrapper_cpp`.
#[derive(Clone)]
pub(crate) struct TypeConversionPolicy {
    pub(crate) unwrapped_type: Type,
    pub(crate) cpp_conversion: CppConversionType,
    pub(crate) rust_conversion: RustConversionType,
}

impl TypeConversionPolicy {
    pub(crate) fn new_unconverted(ty: Type) -> Self {
        TypeConversionPolicy {
            unwrapped_type: ty,
            cpp_conversion: CppConversionType::None,
            rust_conversion: RustConversionType::None,
        }
    }

    pub(crate) fn new_to_unique_ptr(ty: Type) -> Self {
        TypeConversionPolicy {
            unwrapped_type: ty,
            cpp_conversion: CppConversionType::FromValueToUniquePtr,
            rust_conversion: RustConversionType::None,
        }
    }

    pub(crate) fn new_from_unique_ptr(ty: Type) -> Self {
        TypeConversionPolicy {
            unwrapped_type: ty,
            cpp_conversion: CppConversionType::FromUniquePtrToValue,
            rust_conversion: RustConversionType::None,
        }
    }

    pub(crate) fn new_from_str(ty: Type) -> Self {
        TypeConversionPolicy {
            unwrapped_type: ty,
            cpp_conversion: CppConversionType::FromUniquePtrToValue,
            rust_conversion: RustConversionType::FromStr,
        }
    }

    pub(crate) fn box_up_subclass_holder(ty: Type, subclass: SubclassName) -> Self {
        TypeConversionPolicy {
            unwrapped_type: ty,
            cpp_conversion: CppConversionType::None,
            rust_conversion: RustConversionType::ToBoxedUpHolder(subclass),
        }
    }

    pub(crate) fn cpp_work_needed(&self) -> bool {
        !matches!(self.cpp_conversion, CppConversionType::None)
    }

    pub(crate) fn unconverted_rust_type(&self) -> Type {
        match self.cpp_conversion {
            CppConversionType::FromValueToUniquePtr => self.make_unique_ptr_type(),
            _ => self.unwrapped_type.clone(),
        }
    }

    pub(crate) fn converted_rust_type(&self) -> Type {
        match self.cpp_conversion {
            CppConversionType::FromUniquePtrToValue => self.make_unique_ptr_type(),
            _ => self.unwrapped_type.clone(),
        }
    }

    fn make_unique_ptr_type(&self) -> Type {
        let innerty = &self.unwrapped_type;
        parse_quote! {
            cxx::UniquePtr < #innerty >
        }
    }

    pub(crate) fn rust_work_needed(&self) -> bool {
        !matches!(self.rust_conversion, RustConversionType::None)
    }

    pub(crate) fn inverse(&self) -> Self {
        Self {
            unwrapped_type: self.unwrapped_type.clone(),
            cpp_conversion: self.cpp_conversion.inverse(),
            rust_conversion: self.rust_conversion.clone(),
        }
    }
}

#[derive(Clone)]

pub(crate) enum CppFunctionBody {
    FunctionCall(Namespace, Ident),
    StaticMethodCall(Namespace, Ident, Ident),
    Constructor,
    ConstructSuperclass(String),
}

#[derive(Clone)]

pub(crate) enum CppFunctionKind {
    Function,
    Method,
    ConstMethod,
    Constructor,
}

#[derive(Clone)]
pub(crate) struct CppFunction {
    pub(crate) payload: CppFunctionBody,
    pub(crate) wrapper_function_name: Ident,
    pub(crate) original_cpp_name: String,
    pub(crate) return_conversion: Option<TypeConversionPolicy>,
    pub(crate) argument_conversion: Vec<TypeConversionPolicy>,
    pub(crate) kind: CppFunctionKind,
    pub(crate) pass_obs_field: bool,
    pub(crate) qualification: Option<QualifiedName>,
}
