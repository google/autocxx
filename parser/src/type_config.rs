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

/// Configuration about types.
/// At present this is very minimal; in future we should roll
/// known_types.rs into this and possibly other things as well.
#[derive(Default, Hash, Debug)]
pub struct TypeConfig {
    pub(crate) pod_requests: Vec<String>,
    pub(crate) allowlist: Vec<String>,
    pub(crate) blocklist: Vec<String>,
    pub(crate) exclude_utilities: bool,
}

const UTILITIES: &[&str] = &["make_string"];
const NO_UTILITIES: &[&str] = &[];

impl TypeConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_pod_requests(&self) -> &[String] {
        &self.pod_requests
    }

    /// Whether to avoid generating the standard helpful utility
    /// functions which we normally include in every mod.
    pub fn exclude_utilities(&self) -> bool {
        self.exclude_utilities
    }

    pub fn allowlist(&self) -> impl Iterator<Item = String> + '_ {
        self.allowlist
            .iter()
            .map(|s| s.to_string())
            .chain(self.active_utilities().iter().map(|s| s.to_string()))
    }

    pub fn allowlist_is_empty(&self) -> bool {
        self.allowlist.is_empty()
    }

    fn active_utilities(&self) -> &'static [&'static str] {
        if self.exclude_utilities {
            NO_UTILITIES
        } else {
            UTILITIES
        }
    }

    /// Whether this type is on the allowlist specified by the user.
    ///
    /// A note on the allowlist handling in general. It's used in two places:
    /// 1) As directives to bindgen
    /// 2) After bindgen has generated code, to filter the APIs which
    ///    we pass to cxx.
    /// This second pass may seem redundant. But sometimes bindgen generates
    /// unnecessary stuff.
    pub fn is_on_allowlist(&self, cpp_name: &str) -> bool {
        self.allowlist.iter().any(|item| item == cpp_name)
            || self.active_utilities().iter().any(|item| *item == cpp_name)
    }

    pub fn is_on_blocklist(&self, cpp_name: &str) -> bool {
        self.blocklist.contains(&cpp_name.to_string())
    }

    pub fn get_blocklist(&self) -> impl Iterator<Item = &String> {
        self.blocklist.iter()
    }
}
