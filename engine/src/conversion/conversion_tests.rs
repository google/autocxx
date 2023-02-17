// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#[allow(unused_imports)]
use crate::minisyn::parse_quote;
use crate::minisyn::ItemMod;
use autocxx_parser::UnsafePolicy;

use crate::CppCodegenOptions;

use super::BridgeConverter;

// This mod is for tests which take bindgen output directly.
// This should be avoided where possible, since these tests will
// become obsolete or have to change if and when we update
// bindgen. Instead, please add tests working directly from
// the original C++ in integration_tests.rs if possible.
// Also, if you're pasting in code from github issues, it's
// important to make sure that the underlying code has an
// acceptable license. That's why this file is currently blank.

#[allow(dead_code)]
fn do_test(input: ItemMod) {
    let tc = parse_quote! {};
    let bc = BridgeConverter::new(&[], &tc);
    let inclusions = "".into();
    bc.convert(
        input,
        UnsafePolicy::AllFunctionsSafe,
        inclusions,
        &CppCodegenOptions::default(),
        "",
    )
    .unwrap();
}

// How to add a test here
//
// #[test]
// fn test_xyz() {
//      do_test(parse_quote!{ /* paste bindgen output here */})
// }
