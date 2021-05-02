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

mod doc_attr;
mod fun_codegen;
mod function_wrapper_rs;
mod impl_item_creator;
mod namespace_organizer;
mod non_pod_struct;
mod unqualify;

use std::collections::HashMap;

// The following should not need to be exposed outside
// codegen_rs but currently Rust codegen happens everywhere... TODO
pub(crate) use non_pod_struct::make_non_pod;

use proc_macro2::TokenStream;
use syn::{parse_quote, Attribute, ForeignItem, Ident, Item, ItemForeignMod, ItemMod};

use crate::types::{make_ident, Namespace, QualifiedName};
use impl_item_creator::create_impl_items;

use self::{
    fun_codegen::gen_function,
    namespace_organizer::{HasNs, NamespaceEntries},
    non_pod_struct::new_non_pod_struct,
};

use super::codegen_cpp::type_to_cpp::{
    namespaced_name_using_original_name_map, original_name_map_from_apis, OriginalNameMap,
};
use super::{
    analysis::fun::FnAnalysis,
    api::{AnalysisPhase, Api, ApiDetail, ImplBlockDetails, TypeKind, TypedefKind},
};
use super::{convert_error::ErrorContext, ConvertError};
use quote::quote;

unzip_n::unzip_n!(pub 3);

/// Whether and how this item should be exposed in the mods constructed
/// for actual end-user use.
#[derive(Clone)]
enum Use {
    /// Not used
    Unused,
    /// Uses from cxx::bridge
    UsedFromCxxBridge,
    /// 'use' points to cxx::bridge with a different name
    UsedFromCxxBridgeWithAlias(Ident),
    /// 'use' directive points to bindgen
    UsedFromBindgen,
    /// Some kind of custom item
    Custom(Box<Item>),
}

fn get_string_items() -> Vec<Item> {
    [
        Item::Trait(parse_quote! {
            pub trait ToCppString {
                fn into_cpp(self) -> cxx::UniquePtr<cxx::CxxString>;
            }
        }),
        // We can't just impl<T: AsRef<str>> ToCppString for T
        // because the compiler says that this trait could be implemented
        // in future for cxx::UniquePtr<cxx::CxxString>. Fair enough.
        Item::Impl(parse_quote! {
            impl ToCppString for &str {
                fn into_cpp(self) -> cxx::UniquePtr<cxx::CxxString> {
                    cxxbridge::make_string(self)
                }
            }
        }),
        Item::Impl(parse_quote! {
            impl ToCppString for String {
                fn into_cpp(self) -> cxx::UniquePtr<cxx::CxxString> {
                    cxxbridge::make_string(&self)
                }
            }
        }),
        Item::Impl(parse_quote! {
            impl ToCppString for &String {
                fn into_cpp(self) -> cxx::UniquePtr<cxx::CxxString> {
                    cxxbridge::make_string(self)
                }
            }
        }),
        Item::Impl(parse_quote! {
            impl ToCppString for cxx::UniquePtr<cxx::CxxString> {
                fn into_cpp(self) -> cxx::UniquePtr<cxx::CxxString> {
                    self
                }
            }
        }),
    ]
    .to_vec()
}

fn remove_nones<T>(input: Vec<Option<T>>) -> Vec<T> {
    input.into_iter().flatten().collect()
}

/// Type which handles generation of Rust code.
/// In practice, much of the "generation" involves connecting together
/// existing lumps of code within the Api structures.
pub(crate) struct RsCodeGenerator<'a> {
    include_list: &'a [String],
    bindgen_mod: ItemMod,
    original_name_map: OriginalNameMap,
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
            original_name_map: original_name_map_from_apis(&all_apis),
        };
        c.rs_codegen(all_apis)
    }

    fn rs_codegen(mut self, all_apis: Vec<Api<FnAnalysis>>) -> Vec<Item> {
        // ... and now let's start to generate the output code.
        // First let's see if we plan to generate the string construction utilities, as this will affect
        // what 'use' statements we need here and there.
        let generate_utilities = all_apis
            .iter()
            .any(|api| matches!(&api.detail, ApiDetail::StringConstructor));
        // Now let's generate the Rust code.
        let (rs_codegen_results_and_namespaces, additional_cpp_needs): (Vec<_>, Vec<_>) = all_apis
            .into_iter()
            .map(|api| {
                let more_cpp_needed = api.additional_cpp().is_some();
                let gen = self.generate_rs_for_api(&api.name, api.detail);
                ((api.name, gen), more_cpp_needed)
            })
            .unzip();
        // First, the hierarchy of mods containing lots of 'use' statements
        // which is the final API exposed as 'ffi'.
        let mut use_statements =
            Self::generate_final_use_statements(&rs_codegen_results_and_namespaces);
        // And work out what we need for the bindgen mod.
        let bindgen_root_items = self
            .generate_final_bindgen_mods(&rs_codegen_results_and_namespaces, generate_utilities);
        // Both of the above ('use' hierarchy and bindgen mod) are organized into
        // sub-mods by namespace. From here on, things are flat.
        let (_, rs_codegen_results): (Vec<_>, Vec<_>) =
            rs_codegen_results_and_namespaces.into_iter().unzip();
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
            self.bindgen_mod.vis = parse_quote! {};
            self.bindgen_mod.content.as_mut().unwrap().1 = vec![Item::Mod(parse_quote! {
                pub(super) mod root {
                    #(#bindgen_root_items)*
                }
            })];
            all_items.push(Item::Mod(self.bindgen_mod));
        }
        all_items.push(Item::Mod(parse_quote! {
            #[cxx::bridge]
            mod cxxbridge {
                #(#bridge_items)*
            }
        }));

        all_items.push(Item::Use(parse_quote! {
            #[allow(unused_imports)]
            use bindgen::root;
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
    fn generate_final_use_statements(
        input_items: &[(QualifiedName, RsCodegenResult)],
    ) -> Vec<Item> {
        let mut output_items = Vec::new();
        let ns_entries = NamespaceEntries::new(input_items);
        Self::append_child_use_namespace(&ns_entries, &mut output_items);
        output_items
    }

    fn append_child_use_namespace(
        ns_entries: &NamespaceEntries<(QualifiedName, RsCodegenResult)>,
        output_items: &mut Vec<Item>,
    ) {
        for (name, codegen) in ns_entries.entries() {
            match &codegen.materialization {
                Use::UsedFromCxxBridgeWithAlias(alias) => {
                    output_items.push(Self::generate_cxx_use_stmt(name, Some(alias)))
                }
                Use::UsedFromCxxBridge => {
                    output_items.push(Self::generate_cxx_use_stmt(name, None))
                }
                Use::UsedFromBindgen => output_items.push(Self::generate_bindgen_use_stmt(name)),
                Use::Unused => {}
                Use::Custom(item) => output_items.push(*item.clone()),
            };
        }
        for (child_name, child_ns_entries) in ns_entries.children() {
            if child_ns_entries.is_empty() {
                continue;
            }
            let child_id = make_ident(child_name);
            let mut new_mod: ItemMod = parse_quote!(
                pub mod #child_id {
                }
            );
            Self::append_child_use_namespace(
                child_ns_entries,
                &mut new_mod.content.as_mut().unwrap().1,
            );
            output_items.push(Item::Mod(new_mod));
        }
    }

    fn append_uses_for_ns(
        &mut self,
        items: &mut Vec<Item>,
        ns: &Namespace,
        generate_utilities: bool,
    ) {
        let super_duper = std::iter::repeat(make_ident("super")); // I'll get my coat
        let supers = super_duper.clone().take(ns.depth() + 2);
        items.push(Item::Use(parse_quote! {
            #[allow(unused_imports)]
            use self::
                #(#supers)::*
            ::cxxbridge;
        }));
        if generate_utilities {
            let supers = super_duper.clone().take(ns.depth() + 2);
            items.push(Item::Use(parse_quote! {
                #[allow(unused_imports)]
                use self::
                    #(#supers)::*
                ::ToCppString;
            }));
        }
        let supers = super_duper.take(ns.depth() + 1);
        items.push(Item::Use(parse_quote! {
            #[allow(unused_imports)]
            use self::
                #(#supers)::*
            ::root;
        }));
    }

    fn remove_bindgen_attrs_from_item(mut item: Item) -> Item {
        fn is_bindgen_attr(attr: &Attribute) -> bool {
            let segments = &attr.path.segments;
            segments.len() == 1
                && segments
                    .first()
                    .unwrap()
                    .ident
                    .to_string()
                    .starts_with("bindgen_")
        }

        fn remove_bindgen_attrs(attrs: &mut Vec<Attribute>) {
            *attrs = attrs
                .drain(..)
                .filter(|attr| !is_bindgen_attr(&attr))
                .collect();
        }

        match &mut item {
            Item::Const(item_const) => remove_bindgen_attrs(&mut item_const.attrs),
            Item::Enum(item_enum) => remove_bindgen_attrs(&mut item_enum.attrs),
            Item::ExternCrate(item_extern_crate) => {
                remove_bindgen_attrs(&mut item_extern_crate.attrs)
            }
            Item::Fn(item_fn) => remove_bindgen_attrs(&mut item_fn.attrs),
            Item::ForeignMod(item_foreign_mod) => remove_bindgen_attrs(&mut item_foreign_mod.attrs),
            Item::Impl(item_impl) => remove_bindgen_attrs(&mut item_impl.attrs),
            Item::Macro(item_macro) => remove_bindgen_attrs(&mut item_macro.attrs),
            Item::Macro2(item_macro2) => remove_bindgen_attrs(&mut item_macro2.attrs),
            Item::Mod(item_mod) => remove_bindgen_attrs(&mut item_mod.attrs),
            Item::Static(item_static) => remove_bindgen_attrs(&mut item_static.attrs),
            Item::Struct(item_struct) => remove_bindgen_attrs(&mut item_struct.attrs),
            Item::Trait(item_trait) => remove_bindgen_attrs(&mut item_trait.attrs),
            Item::TraitAlias(item_trait_alias) => remove_bindgen_attrs(&mut item_trait_alias.attrs),
            Item::Type(item_type) => remove_bindgen_attrs(&mut item_type.attrs),
            Item::Union(item_union) => remove_bindgen_attrs(&mut item_union.attrs),
            Item::Use(item_use) => remove_bindgen_attrs(&mut item_use.attrs),
            Item::Verbatim(_) => {}
            #[cfg(test)]
            Item::__TestExhaustive(_) => unimplemented!(),
            #[cfg(not(test))]
            // Do nothing -- assumne there are no Attributes in the item.
            _ => (),
        }
        item
    }

    fn append_child_bindgen_namespace(
        &mut self,
        ns_entries: &NamespaceEntries<(QualifiedName, RsCodegenResult)>,
        output_items: &mut Vec<Item>,
        ns: &Namespace,
        generate_utilities: bool,
    ) {
        let mut impl_entries_by_type: HashMap<_, Vec<_>> = HashMap::new();
        for item in ns_entries.entries() {
            // Remove `bindgen_` attributes. They don't have a corresponding macro defined anywhere,
            // so they will cause compilation errors if we leave them in.
            output_items.extend(
                item.1
                    .bindgen_mod_item
                    .iter()
                    .cloned()
                    .map(Self::remove_bindgen_attrs_from_item),
            );
            if let Some(impl_entry) = &item.1.impl_entry {
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
            self.append_child_bindgen_namespace(
                child_ns_entries,
                &mut inner_output_items,
                &new_ns,
                generate_utilities,
            );
            if !inner_output_items.is_empty() {
                let mut new_mod: ItemMod = parse_quote!(
                    pub mod #child_id {
                    }
                );
                self.append_uses_for_ns(&mut inner_output_items, &new_ns, generate_utilities);
                new_mod.content.as_mut().unwrap().1 = inner_output_items;
                output_items.push(Item::Mod(new_mod));
            }
        }
    }

    fn generate_final_bindgen_mods(
        &mut self,
        input_items: &[(QualifiedName, RsCodegenResult)],
        generate_utilities: bool,
    ) -> Vec<Item> {
        let mut output_items = Vec::new();
        let ns = Namespace::new();
        let ns_entries = NamespaceEntries::new(input_items);
        self.append_child_bindgen_namespace(
            &ns_entries,
            &mut output_items,
            &ns,
            generate_utilities,
        );
        self.append_uses_for_ns(&mut output_items, &ns, generate_utilities);
        output_items
    }

    fn generate_rs_for_api(
        &self,
        name: &QualifiedName,
        api_detail: ApiDetail<FnAnalysis>,
    ) -> RsCodegenResult {
        let id = name.get_final_ident();
        match api_detail {
            ApiDetail::StringConstructor => RsCodegenResult {
                extern_c_mod_item: Some(ForeignItem::Fn(parse_quote!(
                    fn make_string(str_: &str) -> UniquePtr<CxxString>;
                ))),
                bridge_items: Vec::new(),
                global_items: get_string_items(),
                bindgen_mod_item: None,
                impl_entry: None,
                materialization: Use::Unused,
            },
            ApiDetail::ConcreteType { .. } => RsCodegenResult {
                global_items: self.generate_extern_type_impl(TypeKind::NonPod, &name),
                bridge_items: create_impl_items(&id),
                extern_c_mod_item: Some(ForeignItem::Verbatim(self.generate_cxxbridge_type(name))),
                bindgen_mod_item: Some(Item::Struct(new_non_pod_struct(id.clone()))),
                impl_entry: None,
                materialization: Use::Unused,
            },
            ApiDetail::ForwardDeclaration => RsCodegenResult {
                extern_c_mod_item: Some(ForeignItem::Verbatim(self.generate_cxxbridge_type(name))),
                bridge_items: Vec::new(),
                global_items: self.generate_extern_type_impl(TypeKind::NonPod, &name),
                bindgen_mod_item: Some(Item::Struct(new_non_pod_struct(id))),
                impl_entry: None,
                materialization: Use::UsedFromCxxBridge,
            },
            ApiDetail::Function { fun, analysis } => {
                gen_function(name.get_namespace(), *fun, analysis)
            }
            ApiDetail::Const { const_item } => RsCodegenResult {
                global_items: Vec::new(),
                impl_entry: None,
                bridge_items: Vec::new(),
                extern_c_mod_item: None,
                bindgen_mod_item: Some(Item::Const(const_item)),
                materialization: Use::UsedFromBindgen,
            },
            ApiDetail::Typedef { item: _, analysis } => RsCodegenResult {
                extern_c_mod_item: None,
                bridge_items: Vec::new(),
                global_items: Vec::new(),
                bindgen_mod_item: Some(match analysis {
                    TypedefKind::Type(type_item) => Item::Type(type_item),
                    TypedefKind::Use(use_item) => Item::Use(use_item),
                }),
                impl_entry: None,
                materialization: Use::UsedFromBindgen,
            },
            ApiDetail::Type {
                bindgen_mod_item,
                analysis,
            } => RsCodegenResult {
                global_items: self.generate_extern_type_impl(analysis, &name),
                impl_entry: None,
                bridge_items: if analysis.can_be_instantiated() {
                    create_impl_items(&id)
                } else {
                    Vec::new()
                },
                extern_c_mod_item: Some(ForeignItem::Verbatim(self.generate_cxxbridge_type(name))),
                bindgen_mod_item,
                materialization: Use::UsedFromCxxBridge,
            },
            ApiDetail::CType { .. } => RsCodegenResult {
                global_items: Vec::new(),
                impl_entry: None,
                bridge_items: Vec::new(),
                extern_c_mod_item: Some(ForeignItem::Verbatim(quote! {
                    type #id = autocxx::#id;
                })),
                bindgen_mod_item: None,
                materialization: Use::Unused,
            },
            ApiDetail::IgnoredItem { err, ctx } => Self::generate_error_entry(err, ctx),
        }
    }

    /// Generates something in the output mod that will carry a docstring
    /// explaining why a given type or function couldn't have bindings
    /// generated.
    fn generate_error_entry(err: ConvertError, ctx: ErrorContext) -> RsCodegenResult {
        let err = format!("autocxx bindings couldn't be generated: {}", err);
        let (impl_entry, materialization) = match ctx {
            ErrorContext::Item(id) => (
                None,
                Use::Custom(Box::new(parse_quote! {
                    #[doc = #err]
                    pub struct #id;
                })),
            ),
            ErrorContext::Method { self_ty, method } => (
                Some(Box::new(ImplBlockDetails {
                    item: parse_quote! {
                        #[doc = #err]
                        fn #method(_uhoh: autocxx::BindingGenerationFailure) {
                        }
                    },
                    ty: self_ty,
                })),
                Use::Unused,
            ),
        };
        RsCodegenResult {
            global_items: Vec::new(),
            impl_entry,
            bridge_items: Vec::new(),
            extern_c_mod_item: None,
            bindgen_mod_item: None,
            materialization,
        }
    }

    fn generate_cxx_use_stmt(name: &QualifiedName, alias: Option<&Ident>) -> Item {
        let segs = Self::find_output_mod_root(name.get_namespace())
            .chain(std::iter::once(make_ident("cxxbridge")))
            .chain(std::iter::once(name.get_final_ident()));
        Item::Use(match alias {
            None => parse_quote! {
                pub use #(#segs)::*;
            },
            Some(alias) => parse_quote! {
                pub use #(#segs)::* as #alias;
            },
        })
    }

    fn generate_bindgen_use_stmt(name: &QualifiedName) -> Item {
        let segs =
            Self::find_output_mod_root(name.get_namespace()).chain(name.get_bindgen_path_idents());
        Item::Use(parse_quote! {
            pub use #(#segs)::*;
        })
    }

    fn generate_extern_type_impl(&self, type_kind: TypeKind, tyname: &QualifiedName) -> Vec<Item> {
        let tynamestring = namespaced_name_using_original_name_map(tyname, &self.original_name_map);
        let fulltypath = tyname.get_bindgen_path_idents();
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

    fn generate_cxxbridge_type(&self, name: &QualifiedName) -> TokenStream {
        let ns = name.get_namespace();
        let id = name.get_final_ident();
        let mut ns_components: Vec<String> = ns.iter().cloned().collect();
        let mut cxx_name = None;
        if let Some(original_name) = self.original_name_map.get(name) {
            let mut name_components: Vec<_> = original_name.split("::").collect();
            cxx_name = Some(name_components.pop().unwrap());
            ns_components.extend(name_components.iter().map(|s| s.to_string()));
        };

        let mut for_extern_c_ts = if !ns_components.is_empty() {
            let ns_string = ns_components.join("::");
            quote! {
                #[namespace = #ns_string]
            }
        } else {
            TokenStream::new()
        };

        if let Some(n) = cxx_name {
            for_extern_c_ts.extend(quote! {
                #[cxx_name = #n]
            });
        }

        for_extern_c_ts.extend(quote! {
            type #id = super::bindgen::root::
        });
        for_extern_c_ts.extend(ns.iter().map(make_ident).map(|id| {
            quote! {
                #id::
            }
        }));
        for_extern_c_ts.extend(quote! {
            #id;
        });
        for_extern_c_ts
    }

    fn find_output_mod_root(ns: &Namespace) -> impl Iterator<Item = Ident> {
        std::iter::repeat(make_ident("super")).take(ns.depth())
    }
}

impl HasNs for (QualifiedName, RsCodegenResult) {
    fn get_namespace(&self) -> &Namespace {
        &self.0.get_namespace()
    }
}

impl<T: AnalysisPhase> HasNs for Api<T> {
    fn get_namespace(&self) -> &Namespace {
        &self.name.get_namespace()
    }
}

/// Snippets of code generated from a particular API.
/// These are then concatenated together into the final generated code.
struct RsCodegenResult {
    extern_c_mod_item: Option<ForeignItem>,
    bridge_items: Vec<Item>,
    global_items: Vec<Item>,
    bindgen_mod_item: Option<Item>,
    impl_entry: Option<Box<ImplBlockDetails>>,
    materialization: Use,
}
