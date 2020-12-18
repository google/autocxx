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

use super::api::Api;
use std::collections::BTreeMap;

pub struct NamespaceEntries<'a> {
    entries: Vec<&'a Api>,
    children: BTreeMap<&'a String, NamespaceEntries<'a>>,
}

impl<'a> NamespaceEntries<'a> {
    pub(crate) fn new(apis: &'a [Api]) -> Self {
        let api_refs = apis.iter().collect::<Vec<_>>();
        Self::sort_by_inner_namespace(api_refs, 0)
    }

    pub(crate) fn entries(&self) -> &[&'a Api] {
        &self.entries
    }

    pub(crate) fn children(&self) -> impl Iterator<Item = (&&String, &NamespaceEntries)> {
        self.children.iter()
    }

    fn sort_by_inner_namespace(apis: Vec<&'a Api>, depth: usize) -> Self {
        let mut root = NamespaceEntries {
            entries: Vec::new(),
            children: BTreeMap::new(),
        };

        let mut kids_by_child_ns = BTreeMap::new();
        for api in apis {
            let first_ns_elem = api.ns.iter().nth(depth);
            if let Some(first_ns_elem) = first_ns_elem {
                let list = kids_by_child_ns
                    .entry(first_ns_elem)
                    .or_insert_with(Vec::new);
                list.push(api);
                continue;
            }
            root.entries.push(api);
        }

        for (k, v) in kids_by_child_ns.into_iter() {
            root.children
                .insert(k, Self::sort_by_inner_namespace(v, depth + 1));
        }

        root
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::Api;
    use super::NamespaceEntries;
    use crate::{conversion::api::Use, types::Namespace};
    use proc_macro2::{Ident, Span};

    #[test]
    fn test_ns_entries_sort() {
        let entries = vec![
            make_api(None, "C"),
            make_api(None, "A"),
            make_api(Some("G"), "E"),
            make_api(Some("D"), "F"),
            make_api(Some("G"), "H"),
            make_api(Some("D::K"), "L"),
            make_api(Some("D::K"), "M"),
            make_api(None, "B"),
            make_api(Some("D"), "I"),
            make_api(Some("D"), "J"),
        ];
        let ns = NamespaceEntries::new(&entries);
        let root_entries = ns.entries();
        assert_eq!(root_entries.len(), 3);
        assert_ident(root_entries[0], "C");
        assert_ident(root_entries[1], "A");
        assert_ident(root_entries[2], "B");
        let mut kids = ns.children();
        let (d_id, d_nse) = kids.next().unwrap();
        assert_eq!(d_id.to_string(), "D");
        let (g_id, g_nse) = kids.next().unwrap();
        assert_eq!(g_id.to_string(), "G");
        assert!(kids.next().is_none());
        let d_nse_entries = d_nse.entries();
        assert_eq!(d_nse_entries.len(), 3);
        assert_ident(d_nse_entries[0], "F");
        assert_ident(d_nse_entries[1], "I");
        assert_ident(d_nse_entries[2], "J");
        let g_nse_entries = g_nse.entries();
        assert_eq!(g_nse_entries.len(), 2);
        assert_ident(g_nse_entries[0], "E");
        assert_ident(g_nse_entries[1], "H");
        let mut g_kids = g_nse.children();
        assert!(g_kids.next().is_none());
        let mut d_kids = d_nse.children();
        let (k_id, k_nse) = d_kids.next().unwrap();
        assert_eq!(k_id.to_string(), "K");
        let k_nse_entries = k_nse.entries();
        assert_eq!(k_nse_entries.len(), 2);
        assert_ident(k_nse_entries[0], "L");
        assert_ident(k_nse_entries[1], "M");
    }

    fn assert_ident(api: &Api, expected: &str) {
        assert_eq!(api.id.to_string(), expected);
    }

    fn make_api(ns: Option<&str>, id: &str) -> Api {
        let ns = match ns {
            Some(st) => Namespace::from_user_input(st),
            None => Namespace::new(),
        };
        Api {
            ns,
            id: Ident::new(id, Span::call_site()),
            use_stmt: Use::Used,
            deps: HashSet::new(),
            extern_c_mod_item: None,
            bridge_item: None,
            global_items: Vec::new(),
            additional_cpp: None,
            id_for_allowlist: None,
            bindgen_mod_item: None,
            impl_entry: None,
        }
    }
}
