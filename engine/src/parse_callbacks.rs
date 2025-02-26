// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::{cell::RefCell, fmt::Display, panic::UnwindSafe, rc::Rc};

use crate::types::{strip_bindgen_original_suffix, Namespace};
use crate::{conversion::CppEffectiveName, types::QualifiedName, RebuildDependencyRecorder};
use autocxx_bindgen::callbacks::Virtualness;
use autocxx_bindgen::callbacks::{
    DiscoveredItem, DiscoveredItemId, Explicitness, SpecialMemberKind, Visibility,
};
use autocxx_bindgen::callbacks::{ItemInfo, ItemKind, ParseCallbacks};
use indexmap::IndexMap as HashMap;
use indexmap::IndexSet as HashSet;
use quote::quote;

/// Newtype wrapper for a C++ "original name"; that is, an annotation
/// derived from bindgen that this is the original name of the C++ item.
///
/// At present these various newtype wrappers for kinds of names
/// (Rust, C++, cxx::bridge) have various conversions between them that
/// are probably not safe. They're marked with FIXMEs. Over time we should
/// remove them, or make them safe by doing name validation at the point
/// of conversion.
#[derive(PartialEq, PartialOrd, Eq, Hash, Clone, Debug)]
pub struct CppOriginalName(String);

impl Display for CppOriginalName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl CppOriginalName {
    pub(crate) fn is_nested(&self) -> bool {
        self.0.contains("::")
    }

    pub(crate) fn from_final_item_of_pre_existing_qualified_name(name: &QualifiedName) -> Self {
        Self(name.get_final_item().to_string())
    }

    pub(crate) fn to_qualified_name(&self) -> QualifiedName {
        QualifiedName::new_from_cpp_name(&self.0)
    }

    pub(crate) fn to_effective_name(&self) -> CppEffectiveName {
        CppEffectiveName(self.0.clone())
    }

    /// This is the main output of this type; it's fed into a mapping
    /// from <weird bindgen name format> to
    /// <sensible namespace::outer::inner format>; this contributes "inner".
    pub(crate) fn for_original_name_map(&self) -> &str {
        &self.0
    }

    /// Used to give the final part of the name which can be used
    /// to figure out the name for constructors, destructors etc.
    pub(crate) fn get_final_segment_for_special_members(&self) -> Option<&str> {
        self.0.rsplit_once("::").map(|(_, suffix)| suffix)
    }

    pub(crate) fn from_type_name_for_constructor(name: String) -> Self {
        Self(name)
    }

    /// Work out what to call a Rust-side API given a C++-side name.
    pub(crate) fn to_string_for_rust_name(&self) -> String {
        self.0.clone()
    }

    /// Return the string inside for validation purposes.
    pub(crate) fn for_validation(&self) -> &str {
        &self.0
    }

    /// Used for diagnostics early in function analysis before we establish
    /// the correct naming.
    pub(crate) fn diagnostic_display_name(&self) -> &String {
        &self.0
    }

    // FIXME - remove
    pub(crate) fn from_rust_name(string: String) -> Self {
        Self(string)
    }

    /// Determines whether we need to generate a cxxbridge::name attribute
    pub(crate) fn does_not_match_cxxbridge_name(
        &self,
        cxxbridge_name: &crate::minisyn::Ident,
    ) -> bool {
        cxxbridge_name.0 != self.0
    }

    pub(crate) fn generate_cxxbridge_name_attribute(&self) -> proc_macro2::TokenStream {
        let cpp_call_name = &self.to_string_for_rust_name();
        quote!(
            #[cxx_name = #cpp_call_name]
        )
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
struct NameAndParent {
    parent: DiscoveredItemId,
    name: String,
}

#[derive(Debug, Default, Clone)]
/// Information communicated to us from bindgen using its `ParseCallbacks`
/// mechanism.
///
/// The various accessor methods here return `None` if a
/// given `QualifiedName` can't be found, because bindgen only tells us
/// information when it actually has it.
pub(crate) struct UnindexedParseCallbackResults {
    original_names: HashMap<DiscoveredItemId, CppOriginalName>,
    virtuals: HashMap<DiscoveredItemId, Virtualness>,
    root_mod: Option<DiscoveredItemId>,
    visibility: HashMap<DiscoveredItemId, Visibility>,
    special_member_kinds: HashMap<DiscoveredItemId, SpecialMemberKind>,
    explicitness: HashMap<DiscoveredItemId, Explicitness>,
    discards_template_param: HashSet<DiscoveredItemId>,
    names: HashMap<DiscoveredItemId, String>,
    mods_for_items: HashMap<DiscoveredItemId, DiscoveredItemId>,
}

impl UnindexedParseCallbackResults {
    pub(crate) fn index(self) -> ParseCallbackResults {
        let index = self
            .mods_for_items
            .iter()
            .filter_map(|(id, parent)| {
                self.names.get(id).map(|name| {
                    (
                        NameAndParent {
                            parent: *parent,
                            name: name.clone(),
                        },
                        *id,
                    )
                })
            })
            .collect();

        ParseCallbackResults {
            results: self,
            index,
        }
    }
}

/// A version of [`UnindexedParseCallbackResults`] with an index constructed
/// for efficient access.
pub(crate) struct ParseCallbackResults {
    results: UnindexedParseCallbackResults,
    index: HashMap<NameAndParent, DiscoveredItemId>,
}

impl ParseCallbackResults {
    fn get_item_by_parentage(&self, search_key: &NameAndParent) -> Option<DiscoveredItemId> {
        self.index.get(search_key).cloned()
    }

    pub(crate) fn get_fn_original_name(&self, name: &QualifiedName) -> Option<CppOriginalName> {
        self.id_by_name(name)
            .and_then(|id| self.results.original_names.get(&id).cloned())
    }

    pub(crate) fn get_original_name(&self, name: &QualifiedName) -> Option<CppOriginalName> {
        self.id_by_name(name)
            .and_then(|id| self.results.original_names.get(&id).cloned())
    }

    pub(crate) fn get_virtualness(&self, name: &QualifiedName) -> Option<Virtualness> {
        self.id_by_name(name)
            .and_then(|id| self.results.virtuals.get(&id).cloned())
    }

    fn id_by_name(&self, name: &QualifiedName) -> Option<DiscoveredItemId> {
        self.mod_id_by_namespace(name.get_namespace())
            .and_then(|parent| {
                let search_key = NameAndParent {
                    parent,
                    name: name.get_final_item().to_string(),
                };
                self.get_item_by_parentage(&search_key)
            })
    }

    fn get_root_mod(&self) -> DiscoveredItemId {
        self.results
            .root_mod
            .expect("Root mod not yet reported by bindgen")
    }

    fn mod_id_by_namespace(&self, namespace: &Namespace) -> Option<DiscoveredItemId> {
        self.mod_id_by_inner_namespace(self.get_root_mod(), namespace.iter())
    }

    fn mod_id_by_inner_namespace<'a>(
        &self,
        parent: DiscoveredItemId,
        mut ns_iter: impl Iterator<Item = &'a str>,
    ) -> Option<DiscoveredItemId> {
        match ns_iter.next() {
            Some(child_mod_name) => {
                let search_key = NameAndParent {
                    parent,
                    name: child_mod_name.to_string(),
                };
                self.get_item_by_parentage(&search_key)
                    .and_then(|child_mod_id| self.mod_id_by_inner_namespace(child_mod_id, ns_iter))
            }
            None => Some(parent),
        }
    }

    pub(crate) fn get_cpp_visibility(&self, name: &QualifiedName) -> Visibility {
        self.id_by_name(name)
            .and_then(|id| self.results.visibility.get(&id).cloned())
            .unwrap_or(Visibility::Public)
    }

    pub(crate) fn special_member_kind(&self, name: &QualifiedName) -> Option<SpecialMemberKind> {
        self.id_by_name(name)
            .and_then(|id| self.results.special_member_kinds.get(&id).cloned())
    }

    pub(crate) fn get_deleted_or_defaulted(&self, name: &QualifiedName) -> Option<Explicitness> {
        self.id_by_name(name)
            .and_then(|id| self.results.explicitness.get(&id).cloned())
    }

    pub(crate) fn discards_template_param(&self, name: &QualifiedName) -> bool {
        self.id_by_name(name)
            .map(|id| self.results.discards_template_param.contains(&id))
            .unwrap_or_default()
    }
}

#[derive(Debug)]
pub(crate) struct AutocxxParseCallbacks {
    pub(crate) rebuild_dependency_recorder: Option<Box<dyn RebuildDependencyRecorder>>,
    pub(crate) results: Rc<RefCell<UnindexedParseCallbackResults>>,
}

impl AutocxxParseCallbacks {
    pub(crate) fn new(
        rebuild_dependency_recorder: Option<Box<dyn RebuildDependencyRecorder>>,
        results: Rc<RefCell<UnindexedParseCallbackResults>>,
    ) -> Self {
        Self {
            rebuild_dependency_recorder,
            results,
        }
    }
}

impl UnwindSafe for AutocxxParseCallbacks {}

impl ParseCallbacks for AutocxxParseCallbacks {
    fn include_file(&self, filename: &str) {
        if let Some(rebuild_dependency_recorder) = &self.rebuild_dependency_recorder {
            rebuild_dependency_recorder.record_header_file_dependency(filename);
        }
    }

    fn generated_name_override(&self, _item_info: ItemInfo<'_>) -> Option<String> {
        // We rename all functions in the original bindgen mod because
        // we will generate alternative implementations instead. We still need
        // to retain the functions so that we can detect them as we
        // parse the bindgen output.
        // For free functions, this isn't necessary: we simply avoid
        // adding a 'use bindgen::root::some_function' in the output
        // namespace. But for methods, we have no way to avoid conflicts
        // if we generate an alternative implementation of a method
        // with a given name.
        match _item_info.kind {
            ItemKind::Function => Some(format!("{}_bindgen_original", _item_info.name)),
            _ => None,
        }
    }

    fn denote_cpp_name(
        &self,
        id: DiscoveredItemId,
        original_name: Option<&str>,
        namespace_mod: Option<DiscoveredItemId>,
    ) {
        let mut results = self.results.borrow_mut();
        if let Some(original_name) = original_name {
            let original_name = strip_bindgen_original_suffix(original_name);
            results
                .original_names
                .insert(id, CppOriginalName(original_name.to_string()));
        }
        if let Some(namespace_mod) = namespace_mod {
            results.mods_for_items.insert(id, namespace_mod);
        }
    }

    fn denote_virtualness(&self, id: DiscoveredItemId, virtualness: Virtualness) {
        self.results.borrow_mut().virtuals.insert(id, virtualness);
    }

    fn new_item_found(&self, id: DiscoveredItemId, item: DiscoveredItem) {
        match item {
            DiscoveredItem::Struct { final_name, .. }
            | DiscoveredItem::Enum { final_name, .. }
            | DiscoveredItem::Union { final_name, .. }
            | DiscoveredItem::Alias {
                alias_name: final_name,
                ..
            }
            | DiscoveredItem::Function { final_name } => {
                let final_name = strip_bindgen_original_suffix(&final_name);
                self.results.borrow_mut().names.insert(id, final_name);
            }
            DiscoveredItem::Mod {
                final_name,
                parent_id,
            } => {
                let mut results = self.results.borrow_mut();
                results.names.insert(id, final_name);
                if let Some(parent_id) = parent_id {
                    results.mods_for_items.insert(id, parent_id);
                } else {
                    results.root_mod.replace(id);
                }
            }
            _ => {}
        }
    }

    fn denote_visibility(&self, id: DiscoveredItemId, visibility: Visibility) {
        if !matches!(visibility, Visibility::Public) {
            // Public is the default; no need to record
            self.results.borrow_mut().visibility.insert(id, visibility);
        }
    }

    fn denote_special_member(&self, id: DiscoveredItemId, kind: SpecialMemberKind) {
        self.results
            .borrow_mut()
            .special_member_kinds
            .insert(id, kind);
    }

    fn denote_explicit(&self, id: DiscoveredItemId, explicitness: Explicitness) {
        self.results
            .borrow_mut()
            .explicitness
            .insert(id, explicitness);
    }

    fn denote_discards_template_param(&self, id: DiscoveredItemId) {
        self.results.borrow_mut().discards_template_param.insert(id);
    }
}
