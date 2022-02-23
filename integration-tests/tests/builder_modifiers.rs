// Copyright 2021 Google LLC
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

use autocxx_engine::Builder;

use autocxx_integration_tests::{BuilderModifier, BuilderModifierFns, TestBuilderContext};

struct ClangArgAdder(Vec<String>);

pub(crate) fn make_clang_arg_adder(args: &[&str]) -> Option<BuilderModifier> {
    let args: Vec<_> = args.iter().map(|a| a.to_string()).collect();
    Some(Box::new(ClangArgAdder(args)))
}

impl BuilderModifierFns for ClangArgAdder {
    fn modify_autocxx_builder(
        &self,
        builder: Builder<TestBuilderContext>,
    ) -> Builder<TestBuilderContext> {
        let refs: Vec<_> = self.0.iter().map(|s| s.as_str()).collect();
        builder.extra_clang_args(&refs)
    }

    fn modify_cc_builder<'a>(&self, mut builder: &'a mut cc::Build) -> &'a mut cc::Build {
        for f in &self.0 {
            builder = builder.flag(f);
        }
        builder
    }
}

pub(crate) struct SetSuppressSystemHeaders;

impl BuilderModifierFns for SetSuppressSystemHeaders {
    fn modify_autocxx_builder(
        &self,
        builder: Builder<TestBuilderContext>,
    ) -> Builder<TestBuilderContext> {
        builder.suppress_system_headers(true)
    }
}

pub(crate) struct EnableAutodiscover;

impl BuilderModifierFns for EnableAutodiscover {
    fn modify_autocxx_builder(
        &self,
        builder: Builder<TestBuilderContext>,
    ) -> Builder<TestBuilderContext> {
        builder.auto_allowlist(true)
    }
}

pub(crate) struct SkipCxxGen;

impl BuilderModifierFns for SkipCxxGen {
    fn modify_autocxx_builder(
        &self,
        builder: Builder<TestBuilderContext>,
    ) -> Builder<TestBuilderContext> {
        builder.skip_cxx_gen(true)
    }
}
