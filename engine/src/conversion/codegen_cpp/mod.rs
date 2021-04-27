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

mod function_wrapper_cpp;
pub(crate) mod type_to_cpp;

use crate::{types::QualifiedName, CppFilePair};
use indoc::indoc;
use itertools::Itertools;
use std::collections::HashSet;
use syn::Type;
use type_to_cpp::type_to_cpp;

use super::{
    analysis::fun::{
        function_wrapper::{FunctionWrapper, FunctionWrapperPayload},
        FnAnalysis,
    },
    api::Api,
    ConvertError,
};

/// Instructions for new C++ which we need to generate.
#[derive(Clone)]
pub(crate) enum AdditionalNeed {
    MakeStringConstructor,
    FunctionWrapper(Box<FunctionWrapper>),
    CTypeTypedef(QualifiedName),
    ConcreteTemplatedTypeTypedef(QualifiedName, Box<Type>),
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
    type_definition: Option<String>, // are output before main declarations
    declaration: Option<String>,
    headers: Vec<Header>,
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
    pub(crate) fn generate_cpp_code(
        inclusions: String,
        apis: &[Api<FnAnalysis>],
    ) -> Result<Option<CppFilePair>, ConvertError> {
        let mut gen = CppCodeGenerator::new(inclusions);
        gen.add_needs(apis.iter().filter_map(|api| api.additional_cpp()))?;
        Ok(gen.generate())
    }

    fn new(inclusions: String) -> Self {
        CppCodeGenerator {
            additional_functions: Vec::new(),
            inclusions,
        }
    }

    fn add_needs(
        &mut self,
        additions: impl Iterator<Item = AdditionalNeed>,
    ) -> Result<(), ConvertError> {
        for need in additions {
            match need {
                AdditionalNeed::MakeStringConstructor => self.generate_string_constructor(),
                AdditionalNeed::FunctionWrapper(by_value_wrapper) => {
                    self.generate_by_value_wrapper(&by_value_wrapper)?
                }
                AdditionalNeed::CTypeTypedef(tn) => self.generate_ctype_typedef(&tn),
                AdditionalNeed::ConcreteTemplatedTypeTypedef(tn, def) => {
                    self.generate_typedef(&tn, type_to_cpp(&def)?)
                }
            }
        }
        Ok(())
    }

    fn generate(&self) -> Option<CppFilePair> {
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
            let type_definitions = self.concat_additional_items(|x| x.type_definition.as_ref());
            let declarations = self.concat_additional_items(|x| x.declaration.as_ref());
            let declarations = format!(
                "#ifndef __AUTOCXXGEN_H__\n#define __AUTOCXXGEN_H__\n\n{}\n{}\n{}\n{}#endif // __AUTOCXXGEN_H__\n",
                headers, self.inclusions, type_definitions, declarations
            );
            log::info!("Additional C++ decls:\n{}", declarations);
            Some(CppFilePair {
                header: declarations.into_bytes(),
                implementation: None,
                header_name: "autocxxgen.h".into(),
            })
        }
    }

    fn concat_additional_items<F>(&self, field_access: F) -> String
    where
        F: FnMut(&AdditionalFunction) -> Option<&String>,
    {
        let mut s = self
            .additional_functions
            .iter()
            .map(field_access)
            .flatten()
            .join("\n");
        s.push('\n');
        s
    }

    fn generate_string_constructor(&mut self) {
        let declaration = indoc! {"
        inline std::unique_ptr<std::string> make_string(::rust::Str str)
        { return std::make_unique<std::string>(std::string(str)); }
        "};
        let declaration = Some(declaration.into());
        self.additional_functions.push(AdditionalFunction {
            type_definition: None,
            declaration,
            headers: vec![
                Header::system("memory"),
                Header::system("string"),
                Header::user("cxx.h"),
            ],
        })
    }

    fn generate_by_value_wrapper(&mut self, details: &FunctionWrapper) -> Result<(), ConvertError> {
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
        let args: Result<Vec<_>, _> = details
            .argument_conversion
            .iter()
            .enumerate()
            .map(|(counter, ty)| {
                Ok(format!(
                    "{} {}",
                    ty.unconverted_type()?,
                    get_arg_name(counter)
                ))
            })
            .collect();
        let args = args?.join(", ");
        let ret_type = details
            .return_conversion
            .as_ref()
            .map_or(Ok("void".to_string()), |x| x.converted_type())?;
        let declaration = format!("{} {}({})", ret_type, name, args);
        let arg_list: Result<Vec<_>, _> = details
            .argument_conversion
            .iter()
            .enumerate()
            .map(|(counter, conv)| conv.cpp_conversion(&get_arg_name(counter)))
            .collect();
        let mut arg_list = arg_list?.into_iter();
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
                format!("return {}", ret.cpp_conversion(&underlying_function_call)?);
        };
        let declaration = Some(format!(
            "{} {{ {}; }}",
            declaration, underlying_function_call,
        ));
        self.additional_functions.push(AdditionalFunction {
            type_definition: None,
            declaration,
            headers: vec![Header::system("memory")],
        });
        Ok(())
    }

    fn generate_ctype_typedef(&mut self, tn: &QualifiedName) {
        let cpp_name = tn.to_cpp_name();
        self.generate_typedef(tn, cpp_name)
    }

    fn generate_typedef(&mut self, tn: &QualifiedName, definition: String) {
        let our_name = tn.get_final_item();
        self.additional_functions.push(AdditionalFunction {
            type_definition: Some(format!("typedef {} {};", definition, our_name)),
            declaration: None,
            headers: Vec::new(),
        })
    }
}
