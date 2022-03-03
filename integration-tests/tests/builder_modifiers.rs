// Copyright 2021 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

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
