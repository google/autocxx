// Copyright 2021 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use autocxx_engine::Builder;

use autocxx_integration_tests::{BuilderModifier, BuilderModifierFns, TestBuilderContext};

pub(crate) fn make_cpp17_adder() -> Option<BuilderModifier> {
    make_clang_arg_adder(&["-std=c++17"])
}

struct ClangArgAdder(Vec<String>, Vec<String>);

pub(crate) fn make_clang_arg_adder(args: &[&str]) -> Option<BuilderModifier> {
    make_clang_optional_arg_adder(args, &[])
}

pub(crate) fn make_clang_optional_arg_adder(
    args: &[&str],
    optional_args: &[&str],
) -> Option<BuilderModifier> {
    let args: Vec<_> = args.iter().map(|a| a.to_string()).collect();
    let optional_args: Vec<_> = optional_args.iter().map(|a| a.to_string()).collect();
    Some(Box::new(ClangArgAdder(args, optional_args)))
}

impl BuilderModifierFns for ClangArgAdder {
    fn modify_autocxx_builder<'a>(
        &self,
        builder: Builder<'a, TestBuilderContext>,
    ) -> Builder<'a, TestBuilderContext> {
        let refs: Vec<_> = self.0.iter().map(|s| s.as_str()).collect();
        builder.extra_clang_args(&refs)
    }

    fn modify_cc_builder<'a>(&self, mut builder: &'a mut cc::Build) -> &'a mut cc::Build {
        for f in &self.0 {
            builder = builder.flag(f);
        }
        for f in &self.1 {
            builder = builder.flag_if_supported(f);
        }
        builder
    }
}

pub(crate) struct SetSuppressSystemHeaders;

impl BuilderModifierFns for SetSuppressSystemHeaders {
    fn modify_autocxx_builder<'a>(
        &self,
        builder: Builder<'a, TestBuilderContext>,
    ) -> Builder<'a, TestBuilderContext> {
        builder.suppress_system_headers(true)
    }
}

pub(crate) struct EnableAutodiscover;

impl BuilderModifierFns for EnableAutodiscover {
    fn modify_autocxx_builder<'a>(
        &self,
        builder: Builder<'a, TestBuilderContext>,
    ) -> Builder<'a, TestBuilderContext> {
        builder.auto_allowlist(true)
    }
}
