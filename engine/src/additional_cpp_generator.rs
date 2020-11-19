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

use crate::types::{make_ident, type_to_cpp, Namespace, TypeName};
use itertools::Itertools;
use std::collections::HashSet;
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
        type_to_cpp(&self.unwrapped_type, TypeName::from_cxx_type_path)
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

    fn make_unique_ptr_type(&self) -> Type {
        let innerty = match &self.unwrapped_type {
            Type::Path(typ) => {
                // Until cxx supports a hierarchic set of inner mods
                // for namespace purposes, we just take the final segment.
                let final_seg = typ.path.segments.last().unwrap();
                parse_quote! {
                   #final_seg
                }
            }
            _ => self.unwrapped_type.clone(),
        };
        parse_quote! {
            UniquePtr < #innerty >
        }
    }
}

pub(crate) struct ByValueWrapper {
    pub(crate) original_function_name: Ident,
    pub(crate) original_function_ns: Namespace,
    pub(crate) wrapper_function_name: Ident,
    pub(crate) return_conversion: Option<ArgumentConversion>,
    pub(crate) argument_conversion: Vec<ArgumentConversion>,
    pub(crate) is_a_method: bool,
}

/// Instructions for new C++ which we need to generate.
pub(crate) enum AdditionalNeed {
    MakeStringConstructor,
    MakeUnique(TypeName, Vec<TypeName>),
    ByValueWrapper(Box<ByValueWrapper>),
}

#[derive(Ord, PartialOrd, Eq, PartialEq, Clone, Hash)]
struct Header {
    name: &'static str,
    system: bool,
}

impl Header {
    fn system(name: &'static str) -> Self {
        Header { name, system: true }
    }

    fn user(name: &'static str) -> Self {
        Header {
            name,
            system: false,
        }
    }

    fn include_stmt(&self) -> String {
        if self.system {
            format!("#include <{}>", self.name)
        } else {
            format!("#include \"{}\"", self.name)
        }
    }
}

struct AdditionalFunction {
    declaration: String,
    definition: String,
    headers: Vec<Header>,
}

/// Details of additional generated C++.
pub(crate) struct AdditionalCpp {
    pub(crate) declarations: String,
    pub(crate) definitions: String,
}

/// Generates additional C++ glue functions needed by autocxx.
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
                AdditionalNeed::MakeStringConstructor => self.generate_string_constructor(),
                AdditionalNeed::MakeUnique(ty, args) => self.generate_make_unique(&ty, &args),
                AdditionalNeed::ByValueWrapper(by_value_wrapper) => {
                    self.generate_by_value_wrapper(*by_value_wrapper)
                }
            }
        }
    }

    pub(crate) fn generate(&self) -> Option<AdditionalCpp> {
        if self.additional_functions.is_empty() {
            None
        } else {
            let headers: HashSet<Header> = self
                .additional_functions
                .iter()
                .map(|x| x.headers.iter().cloned())
                .flatten()
                .collect();
            let headers = headers.iter().map(|x| x.include_stmt()).join("\n");
            let declarations = self.concat_additional_items(|x| &x.declaration);
            let declarations = format!("{}\n{}\n{}", headers, self.inclusions, declarations);
            let definitions = self.concat_additional_items(|x| &x.definition);
            let definitions = format!("#include \"autocxxgen.h\"\n{}", definitions);
            Some(AdditionalCpp {
                declarations,
                definitions,
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
            .join("\n");
        s.push('\n');
        s
    }

    fn generate_string_constructor(&mut self) {
        let declaration = "std::unique_ptr<std::string> make_string(::rust::Str str)";
        let definition = format!(
            "{} {{ return std::make_unique<std::string>(std::string(str)); }}",
            declaration
        );
        let declaration = format!("{};", declaration);
        self.additional_functions.push(AdditionalFunction {
            declaration,
            definition,
            headers: vec![
                Header::system("memory"),
                Header::system("string"),
                Header::user("cxx.h"),
            ],
        })
    }

    fn generate_make_unique(&mut self, ty: &TypeName, constructor_arg_types: &[TypeName]) {
        let name = format!("{}_make_unique", ty.get_final_ident());
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
            declaration,
            definition,
            headers: vec![Header::system("memory")],
        })
    }

    fn generate_by_value_wrapper(&mut self, details: ByValueWrapper) {
        // Even if the original function call is in a namespace,
        // we generate this wrapper in the global namespace.
        // We could easily do this the other way round, and when
        // cxx::bridge comes to support nested namespace mods then
        // we wil wish to do that to avoid name conflicts. However,
        // at the moment this is simpler because it avoids us having
        // to generate namespace blocks in the generated C++.
        let original_func_call = details
            .original_function_ns
            .into_iter()
            .map(|s| make_ident(s))
            .chain(std::iter::once(details.original_function_name))
            .join("::");
        let is_a_method = details.is_a_method;
        let name = details.wrapper_function_name;
        let get_arg_name = |counter: usize| -> String {
            if is_a_method && counter == 0 {
                // For method calls that we generate, the first
                // argument name needs to be such that we recognize
                // it as a method in the second invocation of
                // bridge_converter after it's flowed again through
                // bindgen.
                "autocxx_gen_this".to_string()
            } else {
                format!("arg{}", counter)
            }
        };
        let args = details
            .argument_conversion
            .iter()
            .enumerate()
            .map(|(counter, ty)| format!("{} {}", ty.unconverted_type(), get_arg_name(counter)))
            .join(", ");
        let ret_type = details
            .return_conversion
            .as_ref()
            .map_or("void".to_string(), |x| x.converted_type());
        let declaration = format!("{} {}({})", ret_type, name, args);
        let mut arg_list = details
            .argument_conversion
            .iter()
            .enumerate()
            .map(|(counter, conv)| conv.conversion(&get_arg_name(counter)));
        let receiver = if is_a_method { arg_list.next() } else { None };
        let arg_list = arg_list.join(", ");
        let mut underlying_function_call = format!("{}({})", original_func_call, arg_list);
        if let Some(receiver) = receiver {
            underlying_function_call = format!("{}.{}", receiver, underlying_function_call);
        }
        if let Some(ret) = details.return_conversion {
            underlying_function_call =
                format!("return {}", ret.conversion(&underlying_function_call));
        };
        let definition = format!("{} {{ {}; }}", declaration, underlying_function_call,);
        let declaration = format!("{};", declaration);
        self.additional_functions.push(AdditionalFunction {
            declaration,
            definition,
            headers: vec![Header::system("memory")],
        })
    }
}
