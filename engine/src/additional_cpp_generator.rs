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

use crate::types::TypeName;
use itertools::Itertools;
use std::collections::HashMap;
use syn::{Ident, Type};

enum ArgumentConversionType {
    None,
    FromUniquePtrToValue,
    FromValueToUniquePtr,
}

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

    fn unconverted_type(&self) -> String {
        match self.conversion {
            ArgumentConversionType::FromUniquePtrToValue => self.wrapped_type(),
            _ => self.unwrapped_type_as_string(),
        }
    }

    fn converted_type(&self) -> String {
        match self.conversion {
            ArgumentConversionType::FromValueToUniquePtr => self.wrapped_type(),
            _ => self.unwrapped_type_as_string(),
        }
    }

    fn unwrapped_type_as_string(&self) -> String {
        TypeName::from_type(&self.unwrapped_type)
            .to_cpp_name()
            .to_string()
    }

    fn wrapped_type(&self) -> String {
        format!("std::unique_ptr<{}>", self.unwrapped_type_as_string())
    }

    fn conversion(&self, var_name: &str) -> String {
        match self.conversion {
            ArgumentConversionType::None => var_name.to_string(),
            ArgumentConversionType::FromUniquePtrToValue => format!("std::move(*{})", var_name),
            ArgumentConversionType::FromValueToUniquePtr => format!(
                "std::make_unique<{}>({})",
                self.unconverted_type(),
                var_name
            ),
        }
    }
}

/// Instructions for new C++ which we need to generate.
pub(crate) enum AdditionalNeed {
    MakeUnique(TypeName, Vec<TypeName>),
    ByValueWrapper(Ident, Option<ArgumentConversion>, Vec<ArgumentConversion>),
}

struct AdditionalFunction {
    declaration: String,
    definition: String,
    name: String,
    suppress_older: Option<String>,
    rename: Option<(String, String)>,
}

/// Details of additional generated C++.
pub(crate) struct AdditionalCpp {
    pub(crate) declarations: String,
    pub(crate) definitions: String,
    pub(crate) extra_allowlist: Vec<String>,
    pub(crate) extra_blocklist: Vec<String>,
    pub(crate) renames: HashMap<String, String>,
}

/// Generates additional C++ glue functions needed by autocxx.
/// At the moment, the only use here is for generating an ability
/// to do `make_unique` but more uses are expected in future.
/// In some ways it would be preferable to be able to pass snippets
/// of C++ through to `cxx` for inclusion in the C++ file which it
/// generates, and perhaps we'll explore that in future. But for now,
/// autocxx generates its own _additional_ C++ files which therefore
/// need to be built and included in linking procedures.
pub(crate) struct AdditionalCppGenerator {
    additional_functions: Vec<AdditionalFunction>,
    inclusions: String,
}

impl AdditionalCppGenerator {
    pub(crate) fn new(inclusions: String) -> Self {
        AdditionalCppGenerator {
            additional_functions: Vec::new(),
            inclusions,
        }
    }

    pub(crate) fn add_needs(&mut self, additions: Vec<AdditionalNeed>) {
        for need in additions {
            match need {
                AdditionalNeed::MakeUnique(ty, args) => self.generate_make_unique(&ty, &args),
                AdditionalNeed::ByValueWrapper(id, ret, tys) => {
                    self.generate_by_value_wrapper(&id, &ret, &tys)
                }
            }
        }
    }

    pub(crate) fn generate(&self) -> Option<AdditionalCpp> {
        if self.additional_functions.is_empty() {
            None
        } else {
            let declarations = self.concat_additional_items(|x| &x.declaration);
            let declarations = format!("#include <memory>\n{}\n{}", self.inclusions, declarations);
            let definitions = self.concat_additional_items(|x| &x.definition);
            let definitions = format!("#include \"autocxxgen.h\"\n{}", definitions);
            let extra_allowlist = self
                .additional_functions
                .iter()
                .map(|x| x.name.to_string())
                .collect();
            let extra_blocklist = self
                .additional_functions
                .iter()
                .filter_map(|x| x.suppress_older.clone())
                .collect();
            let renames = self
                .additional_functions
                .iter()
                .filter_map(|x| x.rename.clone())
                .collect();
            Some(AdditionalCpp {
                declarations,
                definitions,
                extra_allowlist,
                extra_blocklist,
                renames,
            })
        }
    }

    fn concat_additional_items<F>(&self, field_access: F) -> String
    where
        F: FnMut(&AdditionalFunction) -> &str,
    {
        let mut s = self
            .additional_functions
            .iter()
            .map(field_access)
            .collect::<Vec<&str>>()
            .join("\n\n");
        s.push('\n');
        s
    }

    fn generate_make_unique(&mut self, ty: &TypeName, constructor_arg_types: &[TypeName]) {
        let name = format!("{}_make_unique", ty.to_cpp_name());
        let constructor_args = constructor_arg_types
            .iter()
            .enumerate()
            .map(|(counter, ty)| format!("{} arg{}", ty.to_cpp_name(), counter))
            .join(", ");
        let declaration = format!("std::unique_ptr<{}> {}({})", ty, name, constructor_args);
        let arg_list = constructor_arg_types
            .iter()
            .enumerate()
            .map(|(counter, _)| format!("arg{}", counter))
            .join(", ");
        let definition = format!(
            "{} {{ return std::make_unique<{}>({}); }}",
            declaration, ty, arg_list
        );
        let declaration = format!("{};", declaration);
        self.additional_functions.push(AdditionalFunction {
            name,
            declaration,
            definition,
            suppress_older: None,
            rename: None,
        })
    }

    fn generate_by_value_wrapper(
        &mut self,
        ident: &Ident,
        ret: &Option<ArgumentConversion>,
        arg_types: &[ArgumentConversion],
    ) {
        let name = format!("{}_up_wrapper", ident.to_string());
        let args = arg_types
            .iter()
            .enumerate()
            .map(|(counter, ty)| format!("{} arg{}", ty.unconverted_type(), counter))
            .join(", ");
        let ret_type = ret
            .as_ref()
            .map_or("void".to_string(), |x| x.converted_type());
        let declaration = format!("{} {}({})", ret_type, name, args);
        let arg_list = arg_types
            .iter()
            .enumerate()
            .map(|(counter, conv)| conv.conversion(&format!("arg{}", counter)))
            .join(", ");
        let underlying_function_call = format!("{}({})", ident.to_string(), arg_list);
        let underlying_function_call = match ret {
            None => underlying_function_call,
            Some(ret) => format!("return {}", ret.conversion(&underlying_function_call)),
        };
        let definition = format!("{} {{ {}; }}", declaration, underlying_function_call,);
        let declaration = format!("{};", declaration);
        self.additional_functions.push(AdditionalFunction {
            name: name.clone(),
            declaration,
            definition,
            suppress_older: Some(ident.to_string()),
            rename: Some((name, ident.to_string())),
        })
    }
}
