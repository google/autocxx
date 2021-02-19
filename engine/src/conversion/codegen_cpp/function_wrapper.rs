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

use crate::conversion::type_to_cpp;
use crate::{known_types::type_lacks_copy_constructor, types::Namespace};
use syn::{parse_quote, Ident, Type};

#[derive(Clone)]
enum ArgumentConversionType {
    None,
    FromUniquePtrToValue,
    FromValueToUniquePtr,
}

#[derive(Clone)]
pub(crate) struct ArgumentConversion {
    unwrapped_type: Type,
    conversion: ArgumentConversionType,
}

impl ArgumentConversion {
    pub(crate) fn new_unconverted(ty: Type) -> Self {
        ArgumentConversion {
            unwrapped_type: ty,
            conversion: ArgumentConversionType::None,
        }
    }

    pub(crate) fn new_to_unique_ptr(ty: Type) -> Self {
        ArgumentConversion {
            unwrapped_type: ty,
            conversion: ArgumentConversionType::FromValueToUniquePtr,
        }
    }

    pub(crate) fn new_from_unique_ptr(ty: Type) -> Self {
        ArgumentConversion {
            unwrapped_type: ty,
            conversion: ArgumentConversionType::FromUniquePtrToValue,
        }
    }

    pub(crate) fn work_needed(&self) -> bool {
        !matches!(self.conversion, ArgumentConversionType::None)
    }

    pub(crate) fn unconverted_type(&self) -> String {
        match self.conversion {
            ArgumentConversionType::FromUniquePtrToValue => self.wrapped_type(),
            _ => self.unwrapped_type_as_string(),
        }
    }

    pub(crate) fn converted_type(&self) -> String {
        match self.conversion {
            ArgumentConversionType::FromValueToUniquePtr => self.wrapped_type(),
            _ => self.unwrapped_type_as_string(),
        }
    }

    pub(crate) fn unconverted_rust_type(&self) -> Type {
        match self.conversion {
            ArgumentConversionType::FromValueToUniquePtr => self.make_unique_ptr_type(),
            _ => self.unwrapped_type.clone(),
        }
    }

    pub(crate) fn converted_rust_type(&self) -> Type {
        match self.conversion {
            ArgumentConversionType::FromUniquePtrToValue => self.make_unique_ptr_type(),
            _ => self.unwrapped_type.clone(),
        }
    }

    fn unwrapped_type_as_string(&self) -> String {
        type_to_cpp(&self.unwrapped_type)
    }

    fn wrapped_type(&self) -> String {
        format!("std::unique_ptr<{}>", self.unwrapped_type_as_string())
    }

    pub(crate) fn conversion(&self, var_name: &str) -> String {
        match self.conversion {
            ArgumentConversionType::None => {
                if type_lacks_copy_constructor(&self.unwrapped_type) {
                    format!("std::move({})", var_name)
                } else {
                    var_name.to_string()
                }
            }
            ArgumentConversionType::FromUniquePtrToValue => format!("std::move(*{})", var_name),
            ArgumentConversionType::FromValueToUniquePtr => format!(
                "std::make_unique<{}>({})",
                self.unconverted_type(),
                var_name
            ),
        }
    }

    fn make_unique_ptr_type(&self) -> Type {
        let innerty = &self.unwrapped_type;
        parse_quote! {
            UniquePtr < #innerty >
        }
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
    pub(crate) return_conversion: Option<ArgumentConversion>,
    pub(crate) argument_conversion: Vec<ArgumentConversion>,
    pub(crate) is_a_method: bool,
}
