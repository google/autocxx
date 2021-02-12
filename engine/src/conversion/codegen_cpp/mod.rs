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

pub(crate) mod function_wrapper;
pub(crate) mod type_to_cpp;

use crate::types::TypeName;
use itertools::Itertools;
use std::collections::HashSet;
use syn::Type;

use function_wrapper::FunctionWrapper;
use type_to_cpp::type_to_cpp;

use self::function_wrapper::FunctionWrapperPayload;

use super::api::{Api, ApiAnalysis};

/// Instructions for new C++ which we need to generate.
#[derive(Clone)]
pub(crate) enum AdditionalNeed {
    MakeStringConstructor,
    FunctionWrapper(Box<FunctionWrapper>),
    CTypeTypedef(TypeName),
    ConcreteTemplatedTypeTypedef(TypeName, Box<Type>),
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
    type_definition: String, // are output before main declarations
    declaration: String,
    definition: String,
    headers: Vec<Header>,
}

/// Details of additional generated C++.
pub(crate) struct CppCodegenResults {
    pub(crate) declarations: String,
    pub(crate) definitions: String,
}

/// Generates additional C++ glue functions needed by autocxx.
/// In some ways it would be preferable to be able to pass snippets
/// of C++ through to `cxx` for inclusion in the C++ file which it
/// generates, and perhaps we'll explore that in future. But for now,
/// autocxx generates its own _additional_ C++ files which therefore
/// need to be built and included in linking procedures.
pub(crate) struct CppCodeGenerator {
    additional_functions: Vec<AdditionalFunction>,
    inclusions: String,
}

impl CppCodeGenerator {
    pub(crate) fn generate_cpp_code<T: ApiAnalysis>(
        inclusions: String,
        apis: &[Api<T>],
    ) -> Option<CppCodegenResults> {
        let mut gen = CppCodeGenerator::new(inclusions);
        gen.add_needs(apis.iter().filter_map(|api| api.additional_cpp.as_ref()));
        gen.generate()
    }

    fn new(inclusions: String) -> Self {
        CppCodeGenerator {
            additional_functions: Vec::new(),
            inclusions,
        }
    }

    fn add_needs<'a>(&mut self, additions: impl Iterator<Item = &'a AdditionalNeed>) {
        for need in additions {
            match need {
                AdditionalNeed::MakeStringConstructor => self.generate_string_constructor(),
                AdditionalNeed::FunctionWrapper(by_value_wrapper) => {
                    self.generate_by_value_wrapper(by_value_wrapper)
                }
                AdditionalNeed::CTypeTypedef(tn) => self.generate_ctype_typedef(tn),
                AdditionalNeed::ConcreteTemplatedTypeTypedef(tn, def) => {
                    self.generate_typedef(tn, type_to_cpp(&def))
                }
            }
        }
    }

    fn generate(&self) -> Option<CppCodegenResults> {
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
            let type_definitions = self.concat_additional_items(|x| &x.type_definition);
            let declarations = self.concat_additional_items(|x| &x.declaration);
            let declarations = format!(
                "{}\n{}\n{}\n{}",
                headers, self.inclusions, type_definitions, declarations
            );
            let definitions = self.concat_additional_items(|x| &x.definition);
            let definitions = format!("#include \"autocxxgen.h\"\n{}", definitions);
            Some(CppCodegenResults {
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
            type_definition: "".into(),
            declaration,
            definition,
            headers: vec![
                Header::system("memory"),
                Header::system("string"),
                Header::user("cxx.h"),
            ],
        })
    }

    fn generate_by_value_wrapper(&mut self, details: &FunctionWrapper) {
        // Even if the original function call is in a namespace,
        // we generate this wrapper in the global namespace.
        // We could easily do this the other way round, and when
        // cxx::bridge comes to support nested namespace mods then
        // we wil wish to do that to avoid name conflicts. However,
        // at the moment this is simpler because it avoids us having
        // to generate namespace blocks in the generated C++.
        let is_a_method = details.is_a_method;
        let name = &details.wrapper_function_name;
        let get_arg_name = |counter: usize| -> String {
            if is_a_method && counter == 0 {
                // For method calls that we generate, the first
                // argument name needs to be such that we recognize
                // it as a method in the second invocation of
                // bridge_converter after it's flowed again through
                // bindgen.
                // TODO this may not be the case any longer. We
                // may be able to remove this.
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
        let mut underlying_function_call = match &details.payload {
            FunctionWrapperPayload::Constructor => arg_list,
            FunctionWrapperPayload::FunctionCall(ns, id) => match receiver {
                Some(receiver) => format!("{}.{}({})", receiver, id.to_string(), arg_list),
                None => {
                    let underlying_function_call = ns
                        .into_iter()
                        .cloned()
                        .chain(std::iter::once(id.to_string()))
                        .join("::");
                    format!("{}({})", underlying_function_call, arg_list)
                }
            },
            FunctionWrapperPayload::StaticMethodCall(ns, ty_id, fn_id) => {
                let underlying_function_call = ns
                    .into_iter()
                    .cloned()
                    .chain([ty_id.to_string(), fn_id.to_string()].iter().cloned())
                    .join("::");
                format!("{}({})", underlying_function_call, arg_list)
            }
        };
        if let Some(ret) = &details.return_conversion {
            underlying_function_call =
                format!("return {}", ret.conversion(&underlying_function_call));
        };
        let definition = format!("{} {{ {}; }}", declaration, underlying_function_call,);
        let declaration = format!("{};", declaration);
        self.additional_functions.push(AdditionalFunction {
            type_definition: "".into(),
            declaration,
            definition,
            headers: vec![Header::system("memory")],
        })
    }

    fn generate_ctype_typedef(&mut self, tn: &TypeName) {
        let cpp_name = tn.to_cpp_name();
        self.generate_typedef(tn, cpp_name)
    }

    fn generate_typedef(&mut self, tn: &TypeName, definition: String) {
        let our_name = tn.get_final_ident();
        self.additional_functions.push(AdditionalFunction {
            type_definition: format!("typedef {} {};", definition, our_name),
            declaration: "".into(),
            definition: "".into(),
            headers: Vec::new(),
        })
    }
}
