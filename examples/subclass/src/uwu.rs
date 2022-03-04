// Copyright 2021 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub(crate) fn uwu(msg: &str) -> String {
    uwuifier::uwuify_str_sse(msg)
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
pub(crate) fn uwu(_msg: &str) -> String {
    "uwuification is unavailable for this pwatform :(".to_string()
}
