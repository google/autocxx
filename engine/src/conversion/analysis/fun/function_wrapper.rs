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

use crate::types::Namespace;
use syn::{parse_quote, Ident, Type};

#[derive(Clone)]
pub(crate) enum CppConversionType {
    None,
    FromUniquePtrToValue,
    FromValueToUniquePtr,
}

#[derive(Clone)]
pub(crate) enum RustConversionType {
    None,
    FromStr,
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
}

#[derive(Clone)] // TODO wish this didn't need to be cloneable
pub(crate) enum FunctionWrapperPayload {
    FunctionCall(Namespace, Ident),
    StaticMethodCall(Namespace, Ident, Ident),
    Constructor,
}

#[derive(Clone)] // TODO wish this didn't need to be cloneable
pub(crate) struct FunctionWrapper {
    pub(crate) payload: FunctionWrapperPayload,
    pub(crate) wrapper_function_name: Ident,
    pub(crate) return_conversion: Option<TypeConversionPolicy>,
    pub(crate) argument_conversion: Vec<TypeConversionPolicy>,
    pub(crate) is_a_method: bool,
}
