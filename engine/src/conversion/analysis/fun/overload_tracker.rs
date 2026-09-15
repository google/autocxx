// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::collections::{HashMap, HashSet};

type Offsets = HashMap<String, usize>;

/// Registry of all the overloads of a function found within a given
/// namespace (i.e. mod in bindgen's output). If necessary we'll append
/// a numeric suffix to a function's Rust name to disambiguate overloads.
/// Note that this is NOT necessarily the same as the suffix added by
/// bindgen to disambiguate overloads it discovers. Its suffix is
/// global across all functions, whereas ours is local within a given
/// type.
/// If bindgen adds a suffix it will be included in 'found_name'
/// but not 'original_name' which is an annotation added by our autocxx-bindgen
/// fork.
///
/// When a numeric overload rename would collide with another identifier that
/// already exists in the same scope (e.g. overload `byteSwap` → `byteSwap2`
/// while a real `byteSwap2` also exists), we instead use
/// `{base}_autocxx_overload{N}` as a temporary non-colliding name. See
/// https://github.com/google/autocxx/issues/1316.
#[derive(Default)]
pub(crate) struct OverloadTracker {
    offset_by_name: Offsets,
    offset_by_type_and_name: HashMap<String, Offsets>,
    /// Ideal/original Rust names that exist in this namespace (free functions).
    reserved_names: HashSet<String>,
    /// Ideal/original Rust names that exist as methods, keyed by type.
    reserved_by_type: HashMap<String, HashSet<String>>,
    /// Final Rust names already assigned in this namespace.
    used_names: HashSet<String>,
    /// Final Rust names already assigned as methods, keyed by type.
    used_by_type: HashMap<String, HashSet<String>>,
}

impl OverloadTracker {
    /// Record that `name` is a native/ideal identifier for a free function so
    /// overload renames will not steal it.
    pub(crate) fn reserve_function_name(&mut self, name: String) {
        self.reserved_names.insert(name);
    }

    /// Record that `name` is a native/ideal identifier for a method on `type_name`.
    pub(crate) fn reserve_method_name(&mut self, type_name: &str, name: String) {
        self.reserved_by_type
            .entry(type_name.to_string())
            .or_default()
            .insert(name);
    }

    pub(crate) fn get_function_real_name(&mut self, found_name: String) -> String {
        self.get_name(None, found_name)
    }

    pub(crate) fn get_method_real_name(&mut self, type_name: &str, found_name: String) -> String {
        self.get_name(Some(type_name), found_name)
    }

    fn get_name(&mut self, type_name: Option<&str>, cpp_method_name: String) -> String {
        let this_offset = {
            let registry = match type_name {
                Some(type_name) => self
                    .offset_by_type_and_name
                    .entry(type_name.to_string())
                    .or_default(),
                None => &mut self.offset_by_name,
            };
            let offset = registry.entry(cpp_method_name.clone()).or_default();
            let this_offset = *offset;
            *offset += 1;
            this_offset
        };

        let assigned = if this_offset == 0 {
            cpp_method_name.clone()
        } else {
            let candidate = format!("{cpp_method_name}{this_offset}");
            if self.is_reserved(type_name, &candidate) || self.is_used(type_name, &candidate) {
                self.next_autocxx_overload_name(type_name, &cpp_method_name, this_offset)
            } else {
                candidate
            }
        };

        self.mark_used(type_name, assigned.clone());
        assigned
    }

    fn is_reserved(&self, type_name: Option<&str>, name: &str) -> bool {
        match type_name {
            Some(type_name) => self
                .reserved_by_type
                .get(type_name)
                .is_some_and(|r| r.contains(name)),
            None => self.reserved_names.contains(name),
        }
    }

    fn is_used(&self, type_name: Option<&str>, name: &str) -> bool {
        match type_name {
            Some(type_name) => self
                .used_by_type
                .get(type_name)
                .is_some_and(|u| u.contains(name)),
            None => self.used_names.contains(name),
        }
    }

    fn mark_used(&mut self, type_name: Option<&str>, name: String) {
        match type_name {
            Some(type_name) => {
                self.used_by_type
                    .entry(type_name.to_string())
                    .or_default()
                    .insert(name);
            }
            None => {
                self.used_names.insert(name);
            }
        }
    }

    fn next_autocxx_overload_name(
        &self,
        type_name: Option<&str>,
        base: &str,
        mut n: usize,
    ) -> String {
        loop {
            let candidate = format!("{base}_autocxx_overload{n}");
            if !self.is_reserved(type_name, &candidate) && !self.is_used(type_name, &candidate) {
                return candidate;
            }
            n += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OverloadTracker;

    #[test]
    fn test_by_function() {
        let mut ot = OverloadTracker::default();
        assert_eq!(ot.get_function_real_name("bob".into()), "bob");
        assert_eq!(ot.get_function_real_name("bob".into()), "bob1");
        assert_eq!(ot.get_function_real_name("bob".into()), "bob2");
    }

    #[test]
    fn test_by_method() {
        let mut ot = OverloadTracker::default();
        assert_eq!(ot.get_method_real_name("Ty1", "bob".into()), "bob");
        assert_eq!(ot.get_method_real_name("Ty1", "bob".into()), "bob1");
        assert_eq!(ot.get_method_real_name("Ty2", "bob".into()), "bob");
        assert_eq!(ot.get_method_real_name("Ty2", "bob".into()), "bob1");
    }

    #[test]
    fn test_method_avoids_existing_digit_suffix() {
        // Image::byteSwap ×3 plus a real Image::byteSwap2 (#1316).
        let mut ot = OverloadTracker::default();
        ot.reserve_method_name("Image", "byteSwap".into());
        ot.reserve_method_name("Image", "byteSwap2".into());
        assert_eq!(
            ot.get_method_real_name("Image", "byteSwap".into()),
            "byteSwap"
        );
        assert_eq!(
            ot.get_method_real_name("Image", "byteSwap".into()),
            "byteSwap1"
        );
        assert_eq!(
            ot.get_method_real_name("Image", "byteSwap".into()),
            "byteSwap_autocxx_overload2"
        );
        assert_eq!(
            ot.get_method_real_name("Image", "byteSwap2".into()),
            "byteSwap2"
        );
    }

    #[test]
    fn test_function_avoids_existing_digit_suffix() {
        let mut ot = OverloadTracker::default();
        ot.reserve_function_name("daft".into());
        ot.reserve_function_name("daft2".into());
        assert_eq!(ot.get_function_real_name("daft".into()), "daft");
        assert_eq!(ot.get_function_real_name("daft".into()), "daft1");
        assert_eq!(
            ot.get_function_real_name("daft".into()),
            "daft_autocxx_overload2"
        );
        assert_eq!(ot.get_function_real_name("daft2".into()), "daft2");
    }
}
