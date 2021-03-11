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

mod fun_codegen;
mod impl_item_creator;
mod namespace_organizer;
mod non_pod_struct;
mod unqualify;

use std::collections::HashMap;

// The following should not need to be exposed outside
// codegen_rs but currently Rust codegen happens everywhere... TODO
pub(crate) use non_pod_struct::make_non_pod;

use syn::{parse_quote, ForeignItem, Ident, Item, ItemForeignMod, ItemMod};

use crate::types::{make_ident, Namespace};
use impl_item_creator::create_impl_items;

use self::{
    fun_codegen::gen_function,
    namespace_organizer::{HasNs, NamespaceEntries},
    non_pod_struct::new_non_pod_struct,
};

use super::{
    analysis::fun::FnAnalysis,
    api::{Api, ApiAnalysis, ApiDetail, ImplBlockDetails, TypeApiDetails, TypeKind, Use},
};
use quote::quote;

unzip_n::unzip_n!(pub 3);

fn remove_nones<T>(input: Vec<Option<T>>) -> Vec<T> {
    input.into_iter().flatten().collect()
}

/// Type which handles generation of Rust code.
/// In practice, much of the "generation" involves connecting together
/// existing lumps of code within the Api structures.
pub(crate) struct RsCodeGenerator<'a> {
    include_list: &'a [String],
    bindgen_mod: ItemMod,
}

impl<'a> RsCodeGenerator<'a> {
    /// Generate code for a set of APIs that was discovered during parsing.
    pub(crate) fn generate_rs_code(
        all_apis: Vec<Api<FnAnalysis>>,
        include_list: &'a [String],
        bindgen_mod: ItemMod,
    ) -> Vec<Item> {
        let c = Self {
            include_list,
            bindgen_mod,
        };
        c.rs_codegen(all_apis)
    }

    fn rs_codegen(mut self, all_apis: Vec<Api<FnAnalysis>>) -> Vec<Item> {
        // ... and now let's start to generate the output code.
        // First, the hierarchy of mods containing lots of 'use' statements
        // which is the final API exposed as 'ffi'.
        let mut use_statements = Self::generate_final_use_statements(&all_apis);
        // Now let's generate the Rust code.
        let (rs_codegen_results_and_namespaces, additional_cpp_needs): (Vec<_>, Vec<_>) = all_apis
            .into_iter()
            .map(|api| {
                let more_cpp_needed = api.additional_cpp().is_some();
                let gen = Self::generate_rs_for_api(&api.ns, &api.id, api.detail);
                ((api.ns, api.id, gen), more_cpp_needed)
            })
            .unzip();
        // And work out what we need for the bindgen mod.
        let bindgen_root_items =
            self.generate_final_bindgen_mods(&rs_codegen_results_and_namespaces);
        // Both of the above ('use' hierarchy and bindgen mod) are organized into
        // sub-mods by namespace. From here on, things are flat.
        let (_, _, rs_codegen_results) =
            rs_codegen_results_and_namespaces.into_iter().unzip_n_vec();
        let (extern_c_mod_items, all_items, bridge_items) = rs_codegen_results
            .into_iter()
            .map(|api| (api.extern_c_mod_item, api.global_items, api.bridge_items))
            .unzip_n_vec();
        // Items for the [cxx::bridge] mod...
        let mut bridge_items: Vec<Item> = bridge_items.into_iter().flatten().collect();
        // Things to include in the "extern "C"" mod passed within the cxx::bridge
        let mut extern_c_mod_items = remove_nones(extern_c_mod_items);
        // And a list of global items to include at the top level.
        let mut all_items: Vec<Item> = all_items.into_iter().flatten().collect();
        // And finally any C++ we need to generate. And by "we" I mean autocxx not cxx.
        let has_additional_cpp_needs = additional_cpp_needs.into_iter().any(std::convert::identity);
        extern_c_mod_items.extend(self.build_include_foreign_items(has_additional_cpp_needs));
        // We will always create an extern "C" mod even if bindgen
        // didn't generate one, e.g. because it only generated types.
        // We still want cxx to know about those types.
        let mut extern_c_mod: ItemForeignMod = parse_quote!(
            extern "C++" {}
        );
        extern_c_mod.items.append(&mut extern_c_mod_items);
        bridge_items.push(Self::make_foreign_mod_unsafe(extern_c_mod));
        // The extensive use of parse_quote here could end up
        // being a performance bottleneck. If so, we might want
        // to set the 'contents' field of the ItemMod
        // structures directly.
        if !bindgen_root_items.is_empty() {
            self.bindgen_mod.content.as_mut().unwrap().1 = vec![Item::Mod(parse_quote! {
                pub mod root {
                    #(#bindgen_root_items)*
                }
            })];
            all_items.push(Item::Mod(self.bindgen_mod));
        }
        all_items.push(Item::Mod(parse_quote! {
            #[cxx::bridge]
            pub mod cxxbridge {
                #(#bridge_items)*
            }
        }));
        all_items.append(&mut use_statements);
        all_items
    }

    fn make_foreign_mod_unsafe(ifm: ItemForeignMod) -> Item {
        // At the moment syn does not support outputting 'unsafe extern "C"' except in verbatim
        // items. See https://github.com/dtolnay/syn/pull/938
        Item::Verbatim(quote! {
            unsafe #ifm
        })
    }

    fn build_include_foreign_items(&self, has_additional_cpp_needs: bool) -> Vec<ForeignItem> {
        let extra_inclusion = if has_additional_cpp_needs {
            Some("autocxxgen.h".to_string())
        } else {
            None
        };
        let chained = self.include_list.iter().chain(extra_inclusion.iter());
        chained
            .map(|inc| {
                ForeignItem::Macro(parse_quote! {
                    include!(#inc);
                })
            })
            .collect()
    }

    /// Generate lots of 'use' statements to pull cxxbridge items into the output
    /// mod hierarchy according to C++ namespaces.
    fn generate_final_use_statements(input_items: &[Api<FnAnalysis>]) -> Vec<Item> {
        let mut output_items = Vec::new();
        let ns_entries = NamespaceEntries::new(input_items);
        Self::append_child_use_namespace(&ns_entries, &mut output_items);
        output_items
    }

    fn append_child_use_namespace(
        ns_entries: &NamespaceEntries<Api<FnAnalysis>>,
        output_items: &mut Vec<Item>,
    ) {
        for item in ns_entries.entries() {
            let id = &item.id;
            match &item.use_stmt() {
                Use::UsedWithAlias(alias) => output_items.push(Item::Use(parse_quote!(
                    pub use cxxbridge :: #id as #alias;
                ))),
                Use::Used => output_items.push(Item::Use(parse_quote!(
                    pub use cxxbridge :: #id;
                ))),
                Use::Unused => {}
            };
        }
        for (child_name, child_ns_entries) in ns_entries.children() {
            if child_ns_entries.is_empty() {
                continue;
            }
            let child_id = make_ident(child_name);
            let mut new_mod: ItemMod = parse_quote!(
                pub mod #child_id {
                    #[allow(unused_imports)]
                    use super::cxxbridge;
                }
            );
            Self::append_child_use_namespace(
                child_ns_entries,
                &mut new_mod.content.as_mut().unwrap().1,
            );
            output_items.push(Item::Mod(new_mod));
        }
    }

    fn append_uses_for_ns(&mut self, items: &mut Vec<Item>, ns: &Namespace) {
        let super_duper = std::iter::repeat(make_ident("super")); // I'll get my coat
        let supers = super_duper.clone().take(ns.depth() + 2);
        items.push(Item::Use(parse_quote! {
            #[allow(unused_imports)]
            use self::
                #(#supers)::*
            ::cxxbridge;
        }));
        let supers = super_duper.take(ns.depth() + 1);
        items.push(Item::Use(parse_quote! {
            #[allow(unused_imports)]
            use self::
                #(#supers)::*
            ::root;
        }));
    }

    fn append_child_bindgen_namespace(
        &mut self,
        ns_entries: &NamespaceEntries<(Namespace, Ident, RsCodegenResult)>,
        output_items: &mut Vec<Item>,
        ns: &Namespace,
    ) {
        let mut impl_entries_by_type: HashMap<_, Vec<_>> = HashMap::new();
        for item in ns_entries.entries() {
            output_items.extend(item.2.bindgen_mod_item.iter().cloned());
            if let Some(impl_entry) = &item.2.impl_entry {
                impl_entries_by_type
                    .entry(impl_entry.ty.clone())
                    .or_default()
                    .push(&impl_entry.item);
            }
        }
        for (ty, entries) in impl_entries_by_type.into_iter() {
            output_items.push(Item::Impl(parse_quote! {
                impl #ty {
                    #(#entries)*
                }
            }))
        }
        for (child_name, child_ns_entries) in ns_entries.children() {
            let new_ns = ns.push((*child_name).clone());
            let child_id = make_ident(child_name);

            let mut inner_output_items = Vec::new();
            self.append_child_bindgen_namespace(child_ns_entries, &mut inner_output_items, &new_ns);
            if !inner_output_items.is_empty() {
                let mut new_mod: ItemMod = parse_quote!(
                    pub mod #child_id {
                    }
                );
                self.append_uses_for_ns(&mut inner_output_items, &new_ns);
                new_mod.content.as_mut().unwrap().1 = inner_output_items;
                output_items.push(Item::Mod(new_mod));
            }
        }
    }

    fn generate_final_bindgen_mods(
        &mut self,
        input_items: &[(Namespace, Ident, RsCodegenResult)],
    ) -> Vec<Item> {
        let mut output_items = Vec::new();
        let ns = Namespace::new();
        let ns_entries = NamespaceEntries::new(input_items);
        self.append_child_bindgen_namespace(&ns_entries, &mut output_items, &ns);
        self.append_uses_for_ns(&mut output_items, &ns);
        output_items
    }

    fn generate_rs_for_api(
        ns: &Namespace,
        id: &Ident,
        api_detail: ApiDetail<FnAnalysis>,
    ) -> RsCodegenResult {
        match api_detail {
            ApiDetail::StringConstructor => RsCodegenResult {
                extern_c_mod_item: Some(ForeignItem::Fn(parse_quote!(
                    fn make_string(str_: &str) -> UniquePtr<CxxString>;
                ))),
                //additional_cpp: Some(AdditionalNeed::MakeStringConstructor),
                bridge_items: Vec::new(),
                global_items: vec![
                    Item::Trait(parse_quote! {
                        pub trait ToCppString {
                            fn to_cpp(&self) -> cxx::UniquePtr<cxx::CxxString>;
                        }
                    }),
                    Item::Impl(parse_quote! {
                        impl ToCppString for str {
                            fn to_cpp(&self) -> cxx::UniquePtr<cxx::CxxString> {
                                cxxbridge::make_string(self)
                            }
                        }
                    }),
                ],
                bindgen_mod_item: None,
                impl_entry: None,
            },
            ApiDetail::ConcreteType { ty_details, .. } => {
                let global_items = Self::generate_extern_type_impl(TypeKind::NonPod, &ty_details);
                let final_ident = &ty_details.final_ident;
                RsCodegenResult {
                    global_items,
                    bridge_items: create_impl_items(&final_ident),
                    extern_c_mod_item: Some(ForeignItem::Verbatim(quote! {
                        type #final_ident = super::bindgen::root::#final_ident;
                    })),
                    bindgen_mod_item: Some(Item::Struct(new_non_pod_struct(
                        ty_details.final_ident,
                    ))),
                    impl_entry: None,
                }
            }
            ApiDetail::Function { fun: _, analysis } => gen_function(ns, analysis),
            ApiDetail::SynthesizedFunction { analysis } => gen_function(ns, analysis),
            ApiDetail::Const { const_item } => RsCodegenResult {
                global_items: vec![Item::Const(const_item)],
                impl_entry: None,
                bridge_items: Vec::new(),
                extern_c_mod_item: None,
                bindgen_mod_item: None,
            },
            ApiDetail::Typedef { type_item } => RsCodegenResult {
                global_items: Vec::new(),
                impl_entry: None,
                bridge_items: Vec::new(),
                extern_c_mod_item: None,
                bindgen_mod_item: Some(Item::Type(type_item)),
            },
            ApiDetail::Type {
                ty_details,
                for_extern_c_ts,
                is_forward_declaration: _,
                bindgen_mod_item,
                analysis,
            } => RsCodegenResult {
                global_items: Self::generate_extern_type_impl(analysis, &ty_details),
                impl_entry: None,
                bridge_items: match analysis {
                    TypeKind::ForwardDeclaration => Vec::new(),
                    _ => create_impl_items(&ty_details.final_ident),
                },
                extern_c_mod_item: Some(ForeignItem::Verbatim(for_extern_c_ts)),
                bindgen_mod_item,
            },
            ApiDetail::CType { .. } => RsCodegenResult {
                global_items: Vec::new(),
                impl_entry: None,
                bridge_items: Vec::new(),
                extern_c_mod_item: Some(ForeignItem::Verbatim(quote! {
                    type #id = autocxx::#id;
                })),
                bindgen_mod_item: None,
            },
            ApiDetail::OpaqueTypedef => RsCodegenResult {
                global_items: Vec::new(),
                impl_entry: None,
                bridge_items: create_impl_items(id),
                extern_c_mod_item: Some(ForeignItem::Type(parse_quote! {
                    type #id;
                })),
                bindgen_mod_item: None,
            },
        }
    }

    fn generate_extern_type_impl(type_kind: TypeKind, ty_details: &TypeApiDetails) -> Vec<Item> {
        let tynamestring = &ty_details.tynamestring;
        let fulltypath = &ty_details.fulltypath;
        let kind_item = match type_kind {
            TypeKind::Pod => "Trivial",
            _ => "Opaque",
        };
        let kind_item = make_ident(kind_item);
        vec![Item::Impl(parse_quote! {
            unsafe impl cxx::ExternType for #(#fulltypath)::* {
                type Id = cxx::type_id!(#tynamestring);
                type Kind = cxx::kind::#kind_item;
            }
        })]
    }
}

impl HasNs for (Namespace, Ident, RsCodegenResult) {
    fn get_namespace(&self) -> &Namespace {
        &self.0
    }
}

impl<T: ApiAnalysis> HasNs for Api<T> {
    fn get_namespace(&self) -> &Namespace {
        &self.ns
    }
}

pub(crate) struct RsCodegenResult {
    extern_c_mod_item: Option<ForeignItem>,
    bridge_items: Vec<Item>,
    global_items: Vec<Item>,
    bindgen_mod_item: Option<Item>,
    impl_entry: Option<Box<ImplBlockDetails>>,
}
