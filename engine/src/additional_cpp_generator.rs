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

use crate::TypeName;
use itertools::Itertools;

/// Instructions for new C++ which we need to generate.
pub(crate) enum AdditionalNeed {
    MakeUnique(TypeName, Vec<TypeName>),
}

struct AdditionalFunction {
    declaration: String,
    definition: String,
    name: String,
}

/// Details of additional generated C++.
pub(crate) struct AdditionalCpp {
    pub(crate) declarations: String,
    pub(crate) definitions: String,
    pub(crate) extra_allowlist: Vec<String>,
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
            }
        }
    }

    pub(crate) fn generate(&self) -> Option<AdditionalCpp> {
        if self.additional_functions.is_empty() {
            None
        } else {
            // TODO should probably replace pragma once below with traditional include guards.
            let declarations = self.concat_additional_items(|x| &x.declaration);
            let declarations = format!(
                "#pragma once\n#include <memory>\n{}\n{}",
                self.inclusions, declarations
            );
            let definitions = self.concat_additional_items(|x| &x.definition);
            let definitions = format!("#include \"autocxxgen.h\"\n{}", definitions);
            println!("Generated additional C++ declarations:\n{}", declarations);
            println!("Generated additional C++ definitions:\n{}", definitions);
            let extra_allowlist = self
                .additional_functions
                .iter()
                .map(|x| x.name.to_string())
                .collect();
            Some(AdditionalCpp {
                declarations,
                definitions,
                extra_allowlist,
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
        let name = format!("{}_make_unique", ty.to_cxx_name());
        let constructor_args = constructor_arg_types
            .iter()
            .enumerate()
            .map(|(counter, ty)| format!("{} arg{}", ty.to_cxx_name(), counter))
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
        let declaration = format!("struct {};\n{};", ty.to_cxx_name(), declaration);
        self.additional_functions.push(AdditionalFunction {
            name,
            declaration,
            definition,
        })
    }
}
