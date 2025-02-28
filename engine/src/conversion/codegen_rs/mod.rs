// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

mod fun_codegen;
mod function_wrapper_rs;
mod impl_item_creator;
mod lifetime;
mod namespace_organizer;
mod non_pod_struct;
pub(crate) mod unqualify;
mod utils;

use indexmap::map::IndexMap as HashMap;
use indexmap::set::IndexSet as HashSet;

use autocxx_parser::{ExternCppType, IncludeCppConfig, RustFun, UnsafePolicy};

use itertools::Itertools;
use proc_macro2::{Span, TokenStream};
use syn::{
    parse_quote, punctuated::Punctuated, token::Comma, Attribute, Expr, FnArg, ForeignItem,
    ForeignItemFn, Ident, ImplItem, Item, ItemForeignMod, ItemMod, TraitItem, Type, TypePath,
};
use utils::{find_output_mod_root, generate_cxx_use_stmt};

use crate::{
    conversion::codegen_rs::unqualify::{unqualify_params, unqualify_ret_type},
    minisyn::minisynize_punctuated,
    types::{make_ident, Namespace, QualifiedName},
};
use impl_item_creator::create_impl_items;

use self::{
    fun_codegen::gen_function,
    namespace_organizer::{HasNs, NamespaceEntries},
};

use super::{
    analysis::{
        fun::{FnPhase, PodAndDepAnalysis, ReceiverMutability},
        pod::PodAnalysis,
    },
    api::{AnalysisPhase, Api, SubclassName, TypeKind},
    convert_error::ErrorContextType,
    doc_attr::get_doc_attrs,
};
use super::{
    api::{Provenance, RustSubclassFnDetails, SuperclassMethod, TraitImplSignature},
    apivec::ApiVec,
    codegen_cpp::type_to_cpp::CppNameMap,
};
use super::{convert_error::ErrorContext, ConvertErrorFromCpp};
use quote::quote;

/// An entry which needs to go into an `impl` block for a given type.
struct ImplBlockDetails {
    item: ImplItem,
    ty: Type,
}

struct TraitImplBlockDetails {
    item: TraitItem,
    key: TraitImplSignature,
}

fn get_string_items() -> Vec<Item> {
    [
        Item::Trait(parse_quote! {
            /// A trait to be implemented by any type that can be turned
            /// into a C++ string.
            /// This trait is generated once per autocxx FFI mod and each
            /// implementation is incompatible and separate, because each
            /// will use a function generated independently for each mod
            /// in order to do the actual conversion to a C++ string.
            pub trait ToCppString {
                /// Convert `self` into a C++ string in a [`cxx::UniquePtr`].
                fn into_cpp(self) -> cxx::UniquePtr<cxx::CxxString>;
            }
        }),
        // We can't just impl<T: AsRef<str>> ToCppString for T
        // because the compiler says that this trait could be implemented
        // in future for cxx::UniquePtr<cxx::CxxString>. Fair enough.
        Item::Impl(parse_quote! {
            impl ToCppString for &str {
                fn into_cpp(self) -> cxx::UniquePtr<cxx::CxxString> {
                    make_string(self)
                }
            }
        }),
        Item::Impl(parse_quote! {
            impl ToCppString for String {
                fn into_cpp(self) -> cxx::UniquePtr<cxx::CxxString> {
                    make_string(&self)
                }
            }
        }),
        Item::Impl(parse_quote! {
            impl ToCppString for &String {
                fn into_cpp(self) -> cxx::UniquePtr<cxx::CxxString> {
                    make_string(self)
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

/// Type which handles generation of Rust code.
/// In practice, much of the "generation" involves connecting together
/// existing lumps of code within the Api structures.
pub(crate) struct RsCodeGenerator<'a> {
    unsafe_policy: &'a UnsafePolicy,
    include_list: &'a [String],
    bindgen_mod: ItemMod,
    original_name_map: CppNameMap,
    config: &'a IncludeCppConfig,
    header_name: Option<String>,
}

impl<'a> RsCodeGenerator<'a> {
    /// Generate code for a set of APIs that was discovered during parsing.
    pub(crate) fn generate_rs_code(
        all_apis: ApiVec<FnPhase>,
        unsafe_policy: &'a UnsafePolicy,
        include_list: &'a [String],
        bindgen_mod: ItemMod,
        config: &'a IncludeCppConfig,
        header_name: Option<String>,
    ) -> Vec<Item> {
        let c = Self {
            unsafe_policy,
            include_list,
            bindgen_mod,
            original_name_map: CppNameMap::new_from_apis(&all_apis),
            config,
            header_name,
        };
        c.rs_codegen(all_apis)
    }

    fn rs_codegen(mut self, all_apis: ApiVec<FnPhase>) -> Vec<Item> {
        // ... and now let's start to generate the output code.
        // First off, when we generate structs we may need to add some methods
        // if they're superclasses.
        let methods_by_superclass = self.accumulate_superclass_methods(&all_apis);
        let subclasses_with_a_single_trivial_constructor =
            find_trivially_constructed_subclasses(&all_apis);
        let non_pod_types = find_non_pod_types(&all_apis);
        // Now let's generate the Rust code.
        let (rs_codegen_results_and_namespaces, additional_cpp_needs): (Vec<_>, Vec<_>) = all_apis
            .into_iter()
            .map(|api| {
                let more_cpp_needed = api.needs_cpp_codegen();
                let name = api.name().clone();
                let gen = self.generate_rs_for_api(
                    api,
                    &methods_by_superclass,
                    &subclasses_with_a_single_trivial_constructor,
                    &non_pod_types,
                );
                ((name, gen), more_cpp_needed)
            })
            .unzip();
        // First, the hierarchy of mods containing lots of 'use' statements
        // and other items which are the final API exposed as 'ffi'.
        let mut output_mod_items =
            Self::generate_final_output_namespace(&rs_codegen_results_and_namespaces);
        // Both of the above ('use' hierarchy and bindgen mod) are organized into
        // sub-mods by namespace. From here on, things are flat.
        let (_, rs_codegen_results): (Vec<_>, Vec<_>) =
            rs_codegen_results_and_namespaces.into_iter().unzip();
        let (extern_c_mod_items, extern_rust_mod_items, all_items, bridge_items): (
            Vec<_>,
            Vec<_>,
            Vec<_>,
            Vec<_>,
        ) = rs_codegen_results
            .into_iter()
            .map(|api| {
                (
                    api.extern_c_mod_items,
                    api.extern_rust_mod_items,
                    api.global_items,
                    api.bridge_items,
                )
            })
            .multiunzip();
        // Items for the [cxx::bridge] mod...
        let mut bridge_items: Vec<Item> = bridge_items.into_iter().flatten().collect();
        // Things to include in the "extern "C"" mod passed within the cxx::bridge
        let mut extern_c_mod_items: Vec<ForeignItem> =
            extern_c_mod_items.into_iter().flatten().collect();
        // The same for extern "Rust"
        let mut extern_rust_mod_items = extern_rust_mod_items.into_iter().flatten().collect();
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
        let mut extern_rust_mod: ItemForeignMod = parse_quote!(
            extern "Rust" {}
        );
        extern_rust_mod.items.append(&mut extern_rust_mod_items);
        bridge_items.push(Item::ForeignMod(extern_rust_mod));
        // The extensive use of parse_quote here could end up
        // being a performance bottleneck. If so, we might want
        // to set the 'contents' field of the ItemMod
        // structures directly.
        self.bindgen_mod.vis = parse_quote! {};
        self.bindgen_mod.attrs.push(parse_quote! {
            #[doc = "A private mod containing the bindings generated by `bindgen`. Do not use the contents directly - the useful parts will be re-exported into the main FFI mod."]
        });
        all_items.push(Item::Mod(self.bindgen_mod));
        all_items.push(Item::Mod(parse_quote! {
            /// A private mod containing the bindings generated by [`cxx`]. Do not use the contents directly - the useful parts will be re-exported into the main FFI mod.
            #[cxx::bridge]
            mod cxxbridge {
                #(#bridge_items)*
            }
        }));

        all_items.push(Item::Use(parse_quote! {
            #[allow(unused_imports)]
            use bindgen::root;
        }));
        let ffi_mod_name = self.config.get_mod_name();
        all_items.push(Item::Use(parse_quote! {
            #[allow(unused_imports)]
            use super::#ffi_mod_name as output;
        }));
        all_items.append(&mut output_mod_items);
        all_items
    }

    fn accumulate_superclass_methods(
        &self,
        apis: &ApiVec<FnPhase>,
    ) -> HashMap<QualifiedName, Vec<SuperclassMethod>> {
        let mut results = HashMap::new();
        results.extend(
            self.config
                .superclasses()
                .map(|sc| (QualifiedName::new_from_cpp_name(sc), Vec::new())),
        );
        for api in apis.iter() {
            if let Api::SubclassTraitItem { details, .. } = api {
                let list = results.get_mut(&details.receiver);
                if let Some(list) = list {
                    list.push(details.clone());
                }
            }
        }
        results
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
            Some(self.header_name.clone().unwrap())
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

    /// Generate the final output mod hierarchy which the user will actually
    /// interact with. This is mostly lots of 'use' statements.
    fn generate_final_output_namespace(
        input_items: &[(QualifiedName, RsCodegenResult)],
    ) -> Vec<Item> {
        let mut output_items = Vec::new();
        let ns_entries = NamespaceEntries::new(input_items);
        Self::append_child_output_namespace(&ns_entries, &mut output_items);
        output_items
    }

    fn append_child_output_namespace(
        ns_entries: &NamespaceEntries<(QualifiedName, RsCodegenResult)>,
        output_items: &mut Vec<Item>,
    ) {
        for (_name, codegen) in ns_entries.entries() {
            output_items.extend(codegen.output_mod_items.iter().cloned());
        }

        let mut impl_entries_by_type: HashMap<_, Vec<_>> = HashMap::new();
        let mut trait_impl_entries_by_trait_and_ty: HashMap<_, Vec<_>> = HashMap::new();
        for item in ns_entries.entries() {
            if let Some(impl_entry) = &item.1.impl_entry {
                impl_entries_by_type
                    .entry(impl_entry.ty.clone())
                    .or_default()
                    .push(&impl_entry.item);
            }
            if let Some(trait_impl_entry) = &item.1.trait_impl_entry {
                trait_impl_entries_by_trait_and_ty
                    .entry(trait_impl_entry.key.clone())
                    .or_default()
                    .push(&trait_impl_entry.item);
            }
        }
        for (ty, entries) in impl_entries_by_type.into_iter() {
            output_items.push(Item::Impl(parse_quote! {
                impl #ty {
                    #(#entries)*
                }
            }))
        }
        for (key, entries) in trait_impl_entries_by_trait_and_ty.into_iter() {
            let unsafety = key.unsafety;
            let ty = key.ty;
            let trt = key.trait_signature;
            output_items.push(Item::Impl(parse_quote! {
                #unsafety impl #trt for #ty {
                    #(#entries)*
                }
            }))
        }

        for (child_name, child_ns_entries) in ns_entries.children() {
            if child_ns_entries.is_empty() {
                continue;
            }
            let child_id = make_ident(child_name);
            let mut new_mod: ItemMod = parse_quote!(
                pub mod #child_id {
                    #[allow(unused_imports)]
                    use super::{cxxbridge, output, bindgen};
                }
            );
            Self::append_child_output_namespace(
                child_ns_entries,
                &mut new_mod.content.as_mut().unwrap().1,
            );
            output_items.push(Item::Mod(new_mod));
        }
    }

    fn id_to_expr(id: &Ident) -> Expr {
        parse_quote! { #id }
    }

    fn generate_rs_for_api(
        &self,
        api: Api<FnPhase>,
        associated_methods: &HashMap<QualifiedName, Vec<SuperclassMethod>>,
        subclasses_with_a_single_trivial_constructor: &HashSet<QualifiedName>,
        non_pod_types: &HashSet<QualifiedName>,
    ) -> RsCodegenResult {
        let name = api.name().clone();
        let id = name.get_final_ident();
        match api {
            Api::StringConstructor { .. } => {
                let make_string_name = make_ident(self.config.get_makestring_name());
                RsCodegenResult {
                    extern_c_mod_items: vec![ForeignItem::Fn(parse_quote!(
                        /// Make a C++ [`cxx::UniquePtr`] to a [`cxx::CxxString`]
                        /// from a Rust `&str`.
                        fn #make_string_name(str_: &str) -> UniquePtr<CxxString>;
                    ))],
                    global_items: get_string_items(),
                    output_mod_items: vec![generate_cxx_use_stmt(
                        &name,
                        Some(&make_ident("make_string").0),
                    )],
                    ..Default::default()
                }
            }
            Api::Function { fun, analysis, .. } => {
                gen_function(&name, *fun, analysis, non_pod_types)
            }
            Api::Const { .. } | Api::Typedef { .. } => RsCodegenResult {
                output_mod_items: vec![Self::generate_bindgen_use_stmt(&name)],
                ..Default::default()
            },
            Api::Struct {
                details,
                analysis:
                    PodAndDepAnalysis {
                        pod:
                            PodAnalysis {
                                num_generics, kind, ..
                            },
                        constructors,
                        ..
                    },
                ..
            } => {
                let doc_attrs = get_doc_attrs(&details.item.attrs);
                self.generate_type(
                    &name,
                    id,
                    kind,
                    constructors.move_constructor,
                    constructors.destructor,
                    || Some((Item::Struct(details.item.into()), doc_attrs)),
                    associated_methods,
                    num_generics,
                )
            }
            Api::Enum { item, .. } => {
                let doc_attrs = get_doc_attrs(&item.attrs);
                self.generate_type(
                    &name,
                    id,
                    TypeKind::Pod,
                    true,
                    true,
                    || Some((Item::Enum(item.into()), doc_attrs)),
                    associated_methods,
                    0,
                )
            }
            Api::ConcreteType { .. } => self.generate_type(
                &name,
                id,
                TypeKind::Abstract,
                false, // assume for now that these types can't be kept in a Vector
                true,  // assume for now that these types can be put in a smart pointer
                || None,
                associated_methods,
                0,
            ),
            Api::ForwardDeclaration { .. } | Api::OpaqueTypedef { .. } => self.generate_type(
                &name,
                id,
                TypeKind::Abstract,
                false, // these types can't be kept in a Vector
                false, // these types can't be put in a smart pointer
                || None,
                associated_methods,
                0,
            ),
            Api::CType { .. } => RsCodegenResult {
                extern_c_mod_items: vec![ForeignItem::Verbatim(quote! {
                    type #id = autocxx::#id;
                })],
                ..Default::default()
            },
            Api::RustType { path, .. } => {
                let id = path.get_final_ident();
                RsCodegenResult {
                    global_items: vec![parse_quote! {
                        use super::#path;
                    }],
                    extern_rust_mod_items: vec![parse_quote! {
                        type #id;
                    }],
                    ..Default::default()
                }
            }
            Api::RustFn {
                details:
                    RustFun {
                        path,
                        mut sig,
                        has_receiver,
                        ..
                    },
                ..
            } => {
                sig.inputs = unqualify_params(sig.inputs);
                sig.output = unqualify_ret_type(sig.output);
                RsCodegenResult {
                    global_items: if !has_receiver {
                        vec![parse_quote! {
                            use super::#path;
                        }]
                    } else {
                        Vec::new()
                    },
                    extern_rust_mod_items: vec![parse_quote! {
                        #sig;
                    }],
                    ..Default::default()
                }
            }
            Api::RustSubclassFn {
                details, subclass, ..
            } => Self::generate_subclass_fn(id.into(), *details, subclass),
            Api::Subclass {
                name, superclass, ..
            } => {
                let methods = associated_methods.get(&superclass);
                let generate_peer_constructor = subclasses_with_a_single_trivial_constructor.contains(&name.0.name) &&
                    // TODO: Create an UnsafeCppPeerConstructor trait for calling an unsafe
                    // constructor instead? Need to create unsafe versions of everything that uses
                    // it too.
                    matches!(self.unsafe_policy, UnsafePolicy::AllFunctionsSafe);
                self.generate_subclass(name, &superclass, methods, generate_peer_constructor)
            }
            Api::ExternCppType {
                details: ExternCppType { rust_path, .. },
                ..
            } => self.generate_extern_cpp_type(&name, rust_path),
            Api::IgnoredItem {
                err,
                ctx: Some(ctx),
                ..
            } => Self::generate_error_entry(err, ctx),
            Api::IgnoredItem { .. } | Api::SubclassTraitItem { .. } => RsCodegenResult::default(),
        }
    }

    fn generate_subclass(
        &self,
        sub: SubclassName,
        superclass: &QualifiedName,
        methods: Option<&Vec<SuperclassMethod>>,
        generate_peer_constructor: bool,
    ) -> RsCodegenResult {
        let super_name = superclass.get_final_item();
        let super_path = superclass.to_type_path();
        let super_cxxxbridge_id = superclass.get_final_ident();
        let id = sub.id();
        let holder = sub.holder();
        let full_cpp = sub.cpp();
        let cpp_path = full_cpp.to_type_path();
        let cpp_id = full_cpp.get_final_ident();
        let mut global_items = Vec::new();
        let relinquish_ownership_call = sub.cpp_remove_ownership();
        let mut output_mod_items: Vec<Item> = vec![
            parse_quote! {
                pub use cxxbridge::#cpp_id;
            },
            parse_quote! {
                pub struct #holder(pub autocxx::subclass::CppSubclassRustPeerHolder<super::#id>);
            },
            parse_quote! {
                impl autocxx::subclass::CppSubclassCppPeer for #cpp_id {
                    fn relinquish_ownership(&self) {
                        self.#relinquish_ownership_call();
                    }
                }
            },
        ];
        let mut extern_c_mod_items = vec![
            self.generate_cxxbridge_type(&full_cpp, false, Vec::new()),
            parse_quote! {
                fn #relinquish_ownership_call(self: &#cpp_id);
            },
        ];
        if let Some(methods) = methods {
            let supers = SubclassName::get_supers_trait_name(superclass).to_type_path();
            let methods_impls: Vec<ImplItem> = methods
                .iter()
                .filter(|m| !m.is_pure_virtual)
                .map(|m| {
                    let cpp_super_method_name =
                        SubclassName::get_super_fn_name(&Namespace::new(), &m.name.to_string())
                            .get_final_ident();
                    let mut params = m.params.clone();
                    let ret = &m.ret_type.clone();
                    let (peer_fn, first_param) = match m.receiver_mutability {
                        ReceiverMutability::Const => ("peer", parse_quote!(&self)),
                        ReceiverMutability::Mutable => ("peer_mut", parse_quote!(&mut self)),
                    };
                    let peer_fn = make_ident(peer_fn);
                    *(params.iter_mut().next().unwrap()) = first_param;
                    let param_names = m.param_names.iter().skip(1);
                    let unsafe_token = m.requires_unsafe.wrapper_token();
                    parse_quote! {
                        #unsafe_token fn #cpp_super_method_name(#params) #ret {
                            use autocxx::subclass::CppSubclass;
                            self.#peer_fn().#cpp_super_method_name(#(#param_names),*)
                        }
                    }
                })
                .collect();
            if !methods_impls.is_empty() {
                output_mod_items.push(parse_quote! {
                    #[allow(non_snake_case)]
                    impl #supers for super::#id {
                        #(#methods_impls)*
                    }
                });
            }
        }
        if generate_peer_constructor {
            output_mod_items.push(parse_quote! {
                impl autocxx::subclass::CppPeerConstructor<#cpp_id> for super::#id {
                    fn make_peer(&mut self, peer_holder: autocxx::subclass::CppSubclassRustPeerHolder<Self>) -> cxx::UniquePtr<#cpp_path> {
                        use autocxx::moveit::Emplace;
                        cxx::UniquePtr::emplace(#cpp_id :: new(peer_holder))
                    }
                }
            })
        };

        // Once for each superclass, in future...
        let as_id = make_ident(format!("As_{super_name}"));
        extern_c_mod_items.push(parse_quote! {
            fn #as_id(self: &#cpp_id) -> &#super_cxxxbridge_id;
        });
        let as_mut_id = make_ident(format!("As_{super_name}_mut"));
        extern_c_mod_items.push(parse_quote! {
            fn #as_mut_id(self: Pin<&mut #cpp_id>) -> Pin<&mut #super_cxxxbridge_id>;
        });
        let as_unique_ptr_id = make_ident(format!("{cpp_id}_As_{super_name}_UniquePtr"));
        extern_c_mod_items.push(parse_quote! {
            fn #as_unique_ptr_id(u: UniquePtr<#cpp_id>) -> UniquePtr<#super_cxxxbridge_id>;
        });
        output_mod_items.push(parse_quote! {
            impl AsRef<#super_path> for super::#id {
                fn as_ref(&self) -> &cxxbridge::#super_cxxxbridge_id {
                    use autocxx::subclass::CppSubclass;
                    self.peer().#as_id()
                }
            }
        });
        // TODO it would be nice to impl AsMut here but pin prevents us
        output_mod_items.push(parse_quote! {
            impl super::#id {
                pub fn pin_mut(&mut self) -> ::core::pin::Pin<&mut cxxbridge::#super_cxxxbridge_id> {
                    use autocxx::subclass::CppSubclass;
                    self.peer_mut().#as_mut_id()
                }
            }
        });
        let rs_as_unique_ptr_id = make_ident(format!("as_{super_name}_unique_ptr"));
        output_mod_items.push(parse_quote! {
            impl super::#id {
                pub fn #rs_as_unique_ptr_id(u: cxx::UniquePtr<#cpp_id>) -> cxx::UniquePtr<cxxbridge::#super_cxxxbridge_id> {
                    cxxbridge::#as_unique_ptr_id(u)
                }
            }
        });
        let remove_ownership = sub.remove_ownership();
        global_items.push(parse_quote! {
            #[allow(non_snake_case)]
            pub fn #remove_ownership(me: Box<#holder>) -> Box<#holder> {
                Box::new(#holder(me.0.relinquish_ownership()))
            }
        });
        RsCodegenResult {
            extern_c_mod_items,
            // For now we just assume we can't keep subclasses in vectors, but we can put them in
            // smart pointers.
            // That's the reason for the 'false' and 'true'
            bridge_items: create_impl_items(&cpp_id, false, true, self.config),
            output_mod_items,
            global_items,
            extern_rust_mod_items: vec![
                parse_quote! {
                    pub type #holder;
                },
                parse_quote! {
                    fn #remove_ownership(me: Box<#holder>) -> Box<#holder>;
                },
            ],
            ..Default::default()
        }
    }

    fn generate_subclass_fn(
        api_name: Ident,
        details: RustSubclassFnDetails,
        subclass: SubclassName,
    ) -> RsCodegenResult {
        let params = details.params;
        let ret = details.ret;
        let unsafe_token = details.requires_unsafe.wrapper_token();
        let global_def = quote! { #unsafe_token fn #api_name(#params) #ret };
        let params = unqualify_params(minisynize_punctuated(params));
        let ret = unqualify_ret_type(ret.into());
        let method_name = details.method_name;
        let cxxbridge_decl: ForeignItemFn =
            parse_quote! { #unsafe_token fn #api_name(#params) #ret; };
        let args: Punctuated<Expr, Comma> =
            Self::args_from_sig(&cxxbridge_decl.sig.inputs).collect();
        let superclass_id = details.superclass.get_final_ident();
        let methods_trait = SubclassName::get_methods_trait_name(&details.superclass);
        let methods_trait = methods_trait.to_type_path();
        let (deref_ty, deref_call, borrow, mut_token) = match details.receiver_mutability {
            ReceiverMutability::Const => ("Deref", "deref", "try_borrow", None),
            ReceiverMutability::Mutable => (
                "DerefMut",
                "deref_mut",
                "try_borrow_mut",
                Some(syn::token::Mut(Span::call_site())),
            ),
        };
        let deref_ty = make_ident(deref_ty);
        let deref_call = make_ident(deref_call);
        let borrow = make_ident(borrow);
        let destroy_panic_msg = format!("Rust subclass API (method {} of subclass {} of superclass {}) called after subclass destroyed", method_name, subclass.0.name, superclass_id);
        let reentrancy_panic_msg = format!("Rust subclass API (method {} of subclass {} of superclass {}) called whilst subclass already borrowed - likely a re-entrant call",  method_name, subclass.0.name, superclass_id);
        RsCodegenResult {
            global_items: vec![parse_quote! {
                #global_def {
                    let rc = me.0
                        .get()
                        .expect(#destroy_panic_msg);
                    let #mut_token b = rc
                        .as_ref()
                        .#borrow()
                        .expect(#reentrancy_panic_msg);
                    let r = ::core::ops::#deref_ty::#deref_call(& #mut_token b);
                    #methods_trait :: #method_name
                        (r,
                        #args)
                }
            }],
            extern_rust_mod_items: vec![ForeignItem::Fn(cxxbridge_decl)],
            ..Default::default()
        }
    }

    fn args_from_sig(params: &Punctuated<FnArg, Comma>) -> impl Iterator<Item = Expr> + '_ {
        params.iter().skip(1).filter_map(|fnarg| match fnarg {
            syn::FnArg::Receiver(_) => None,
            syn::FnArg::Typed(fnarg) => match &*fnarg.pat {
                syn::Pat::Ident(id) => Some(Self::id_to_expr(&id.ident)),
                _ => None,
            },
        })
    }

    #[allow(clippy::too_many_arguments)] // currently the least unclear way
    fn generate_type<F>(
        &self,
        name: &QualifiedName,
        id: crate::minisyn::Ident,
        type_kind: TypeKind,
        movable: bool,
        destroyable: bool,
        item_creator: F,
        associated_methods: &HashMap<QualifiedName, Vec<SuperclassMethod>>,
        num_generics: usize,
    ) -> RsCodegenResult
    where
        F: FnOnce() -> Option<(Item, Vec<Attribute>)>,
    {
        let mut output_mod_items = Vec::new();
        Self::add_superclass_stuff_to_type(
            name,
            &mut output_mod_items,
            associated_methods.get(name),
        );
        let orig_item = item_creator();
        let doc_attrs = orig_item
            .as_ref()
            .map(|maybe_item| maybe_item.1.clone())
            .unwrap_or_default();
        // We have a choice here to either:
        // a) tell cxx to generate an opaque type using 'type A;'
        // b) generate a concrete type definition, e.g. by using bindgen's
        //    or doing our own, and then telling cxx 'type A = bindgen::A';'
        match type_kind {
            TypeKind::Pod | TypeKind::NonPod | TypeKind::Opaque => {
                // Feed cxx "type T = root::bindgen::T"
                // For non-POD types, there might be the option of simply giving
                // cxx a "type T;" as we do for abstract types below. There's
                // two reasons we don't:
                // a) we want to specify size and alignment for the sake of
                //    moveit;
                // b) for nested types such as 'A::B', there is no combination
                //    of cxx-acceptable attributes which will inform cxx that
                //    A is a class rather than a namespace.
                output_mod_items.push(match type_kind {
                    TypeKind::Pod => Self::generate_bindgen_use_stmt(name),
                    _ => non_pod_struct::generate_opaque_type(name, num_generics, &doc_attrs),
                });
                if num_generics > 0 {
                    // Still generate the type as emitted by bindgen,
                    // but don't attempt to tell cxx about it
                    RsCodegenResult {
                        output_mod_items,
                        ..Default::default()
                    }
                } else {
                    output_mod_items.append(&mut self.generate_extern_type_impl(type_kind, name));
                    RsCodegenResult {
                        bridge_items: create_impl_items(&id, movable, destroyable, self.config),
                        extern_c_mod_items: vec![
                            self.generate_cxxbridge_type(name, true, doc_attrs)
                        ],
                        output_mod_items,
                        ..Default::default()
                    }
                }
            }
            TypeKind::Abstract => {
                if num_generics > 0 {
                    RsCodegenResult::default()
                } else {
                    // Feed cxx "type T;"
                    // We MUST do this because otherwise cxx assumes this can be
                    // instantiated using UniquePtr etc.
                    output_mod_items.push(generate_cxx_use_stmt(name, None));
                    RsCodegenResult {
                        extern_c_mod_items: vec![
                            self.generate_cxxbridge_type(name, false, doc_attrs)
                        ],
                        bridge_items: create_impl_items(&id, movable, destroyable, self.config),
                        output_mod_items,
                        ..Default::default()
                    }
                }
            }
        }
    }

    fn add_superclass_stuff_to_type(
        name: &QualifiedName,
        output_mod_items: &mut Vec<Item>,
        methods: Option<&Vec<SuperclassMethod>>,
    ) {
        if let Some(methods) = methods {
            let (supers, mains): (Vec<_>, Vec<_>) = methods
                .iter()
                .map(|method| {
                    let id = &method.name;
                    let super_id =
                        SubclassName::get_super_fn_name(&Namespace::new(), &id.to_string())
                            .get_final_ident();
                    let params = minisynize_punctuated(method.params.clone());
                    let param_names: Punctuated<Expr, Comma> =
                        Self::args_from_sig(&params).collect();
                    let mut params = method.params.clone();
                    *(params.iter_mut().next().unwrap()) = match method.receiver_mutability {
                        ReceiverMutability::Const => parse_quote!(&self),
                        ReceiverMutability::Mutable => parse_quote!(&mut self),
                    };
                    let ret_type = &method.ret_type;
                    let unsafe_token = method.requires_unsafe.wrapper_token();
                    if method.is_pure_virtual {
                        (
                            None,
                            parse_quote!(
                                #unsafe_token fn #id(#params) #ret_type;
                            ),
                        )
                    } else {
                        let a: Option<TraitItem> = Some(parse_quote!(
                            #unsafe_token fn #super_id(#params) #ret_type;
                        ));
                        let b: TraitItem = parse_quote!(
                            #unsafe_token fn #id(#params) #ret_type {
                                self.#super_id(#param_names)
                            }
                        );
                        (a, b)
                    }
                })
                .unzip();
            let supers: Vec<_> = supers.into_iter().flatten().collect();
            let supers_name = SubclassName::get_supers_trait_name(name).get_final_ident();
            let methods_name = SubclassName::get_methods_trait_name(name).get_final_ident();
            if !supers.is_empty() {
                output_mod_items.push(parse_quote! {
                    #[allow(non_snake_case)]
                    pub trait #supers_name {
                        #(#supers)*
                    }
                });
                output_mod_items.push(parse_quote! {
                    #[allow(non_snake_case)]
                    pub trait #methods_name : #supers_name {
                        #(#mains)*
                    }
                });
            } else {
                output_mod_items.push(parse_quote! {
                    #[allow(non_snake_case)]
                    pub trait #methods_name {
                        #(#mains)*
                    }
                });
            }
        }
    }

    fn generate_extern_cpp_type(
        &self,
        name: &QualifiedName,
        rust_path: TypePath,
    ) -> RsCodegenResult {
        let name_final = name.get_final_ident();
        RsCodegenResult {
            extern_c_mod_items: vec![self.generate_cxxbridge_type(name, true, Vec::new())],
            output_mod_items: vec![parse_quote! { pub use #rust_path as #name_final; }],
            ..Default::default()
        }
    }

    /// Generates something in the output mod that will carry a docstring
    /// explaining why a given type or function couldn't have bindings
    /// generated.
    fn generate_error_entry(err: ConvertErrorFromCpp, ctx: ErrorContext) -> RsCodegenResult {
        let err = format!(" autocxx bindings couldn't be generated: {err}");
        let (impl_entry, output_mod_items) = match ctx.into_type() {
            ErrorContextType::Item(id) | ErrorContextType::SanitizedItem(id) => (
                None,
                vec![parse_quote! {
                    #[doc = #err]
                    pub struct #id;
                }],
            ),
            ErrorContextType::Method { self_ty, method } => (
                Some(Box::new(ImplBlockDetails {
                    item: parse_quote! {
                        #[doc = #err]
                        fn #method(_uhoh: autocxx::BindingGenerationFailure) {
                        }
                    },
                    ty: parse_quote! { #self_ty },
                })),
                vec![],
            ),
        };
        RsCodegenResult {
            impl_entry,
            output_mod_items,
            ..Default::default()
        }
    }

    fn generate_bindgen_use_stmt(name: &QualifiedName) -> Item {
        let segs = find_output_mod_root(name.get_namespace()).chain(name.get_bindgen_path_idents());
        Item::Use(parse_quote! {
            #[allow(unused_imports)]
            pub use #(#segs)::*;
        })
    }

    fn generate_extern_type_impl(&self, type_kind: TypeKind, tyname: &QualifiedName) -> Vec<Item> {
        let tynamestring = self.original_name_map.map(tyname);
        let ty_ident = tyname.get_final_ident();
        let kind_item = match type_kind {
            TypeKind::Pod => "Trivial",
            _ => "Opaque",
        };
        let kind_item = make_ident(kind_item);
        vec![Item::Impl(parse_quote! {
            unsafe impl cxx::ExternType for #ty_ident {
                type Id = cxx::type_id!(#tynamestring);
                type Kind = cxx::kind::#kind_item;
            }
        })]
    }

    fn generate_cxxbridge_type(
        &self,
        name: &QualifiedName,
        references_bindgen: bool,
        doc_attrs: Vec<Attribute>,
    ) -> ForeignItem {
        let ns = name.get_namespace();
        let id = name.get_final_ident();
        // The following lines actually Tell A Lie.
        // If we have a nested class, B::C, within namespace A,
        // we actually have to tell cxx that we have nested class C
        // within namespace A.
        let mut ns_components: Vec<_> = ns.iter().map(|s| s.to_string()).collect();
        let mut cxx_name = None;
        if let Some(cpp_name) = self.original_name_map.get(name) {
            let cpp_name = cpp_name.to_qualified_name();
            cxx_name = Some(cpp_name.get_final_item().to_string());
            ns_components.extend(cpp_name.ns_segment_iter().map(|s| s.to_string()));
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
            #(#doc_attrs)*
        });

        if references_bindgen {
            for_extern_c_ts.extend(quote! {
                type #id = super::
            });
            for_extern_c_ts.extend(ns.iter().map(make_ident).map(|id| {
                quote! {
                    #id::
                }
            }));
            for_extern_c_ts.extend(quote! {
                #id;
            });
        } else {
            for_extern_c_ts.extend(quote! {
                type #id;
            });
        }
        ForeignItem::Verbatim(for_extern_c_ts)
    }
}

fn find_trivially_constructed_subclasses(apis: &ApiVec<FnPhase>) -> HashSet<QualifiedName> {
    let (simple_constructors, complex_constructors): (Vec<_>, Vec<_>) = apis
        .iter()
        .filter_map(|api| match api {
            Api::Function { fun, .. } => match &fun.provenance {
                Provenance::SynthesizedSubclassConstructor(details) => {
                    Some((&details.subclass.0.name, details.is_trivial))
                }
                _ => None,
            },
            _ => None,
        })
        .partition(|(_, trivial)| *trivial);
    let simple_constructors: HashSet<_> =
        simple_constructors.into_iter().map(|(qn, _)| qn).collect();
    let complex_constructors: HashSet<_> =
        complex_constructors.into_iter().map(|(qn, _)| qn).collect();
    (&simple_constructors - &complex_constructors)
        .into_iter()
        .cloned()
        .collect()
}

fn find_non_pod_types(apis: &ApiVec<FnPhase>) -> HashSet<QualifiedName> {
    apis.iter()
        .filter_map(|api| match api {
            Api::Struct {
                name,
                analysis:
                    PodAndDepAnalysis {
                        pod:
                            PodAnalysis {
                                kind: TypeKind::NonPod,
                                ..
                            },
                        ..
                    },
                ..
            } => Some(name.name.clone()),
            _ => None,
        })
        .collect()
}

impl HasNs for (QualifiedName, RsCodegenResult) {
    fn get_namespace(&self) -> &Namespace {
        self.0.get_namespace()
    }
}

impl<T: AnalysisPhase> HasNs for Api<T> {
    fn get_namespace(&self) -> &Namespace {
        self.name().get_namespace()
    }
}

/// Snippets of code generated from a particular API.
/// These are then concatenated together into the final generated code.
#[derive(Default)]
struct RsCodegenResult {
    extern_c_mod_items: Vec<ForeignItem>,
    extern_rust_mod_items: Vec<ForeignItem>,
    bridge_items: Vec<Item>,
    /// Items that go in the top level.
    global_items: Vec<Item>,
    impl_entry: Option<Box<ImplBlockDetails>>,
    trait_impl_entry: Option<Box<TraitImplBlockDetails>>,
    /// Items that go into a per-namespace mod exposed to the user.
    output_mod_items: Vec<Item>,
}

/// An [`Item`] that always needs to be in an unsafe block.
#[derive(Clone)]
enum MaybeUnsafeStmt {
    // This could almost be a syn::Stmt, but that doesn't quite work
    // because the last stmt in a function is actually an expression
    // thus lacking a semicolon.
    Normal(TokenStream),
    NeedsUnsafe(TokenStream),
    Binary {
        in_safe_context: TokenStream,
        in_unsafe_context: TokenStream,
    },
}

impl MaybeUnsafeStmt {
    fn new(stmt: TokenStream) -> Self {
        Self::Normal(stmt)
    }

    fn needs_unsafe(stmt: TokenStream) -> Self {
        Self::NeedsUnsafe(stmt)
    }

    fn maybe_unsafe(stmt: TokenStream, needs_unsafe: bool) -> Self {
        if needs_unsafe {
            Self::NeedsUnsafe(stmt)
        } else {
            Self::Normal(stmt)
        }
    }

    fn binary(in_safe_context: TokenStream, in_unsafe_context: TokenStream) -> Self {
        Self::Binary {
            in_safe_context,
            in_unsafe_context,
        }
    }
}

fn maybe_unsafes_to_tokens(
    items: Vec<MaybeUnsafeStmt>,
    context_is_already_unsafe: bool,
) -> TokenStream {
    if context_is_already_unsafe {
        let items = items.into_iter().map(|item| match item {
            MaybeUnsafeStmt::Normal(stmt)
            | MaybeUnsafeStmt::NeedsUnsafe(stmt)
            | MaybeUnsafeStmt::Binary {
                in_unsafe_context: stmt,
                ..
            } => stmt,
        });
        quote! {
            #(#items)*
        }
    } else {
        let mut currently_unsafe_list = None;
        let mut output = Vec::new();
        for item in items {
            match item {
                MaybeUnsafeStmt::NeedsUnsafe(stmt) => {
                    if currently_unsafe_list.is_none() {
                        currently_unsafe_list = Some(Vec::new());
                    }
                    currently_unsafe_list.as_mut().unwrap().push(stmt);
                }
                MaybeUnsafeStmt::Normal(stmt)
                | MaybeUnsafeStmt::Binary {
                    in_safe_context: stmt,
                    ..
                } => {
                    if let Some(currently_unsafe_list) = currently_unsafe_list.take() {
                        output.push(quote! {
                            unsafe {
                                #(#currently_unsafe_list)*
                            }
                        })
                    }
                    output.push(stmt);
                }
            }
        }
        if let Some(currently_unsafe_list) = currently_unsafe_list.take() {
            output.push(quote! {
                unsafe {
                    #(#currently_unsafe_list)*
                }
            })
        }
        quote! {
            #(#output)*
        }
    }
}

#[test]
fn test_maybe_unsafes_to_tokens() {
    let items = vec![
        MaybeUnsafeStmt::new(quote! { use A; }),
        MaybeUnsafeStmt::new(quote! { use B; }),
        MaybeUnsafeStmt::needs_unsafe(quote! { use C; }),
        MaybeUnsafeStmt::needs_unsafe(quote! { use D; }),
        MaybeUnsafeStmt::new(quote! { use E; }),
        MaybeUnsafeStmt::needs_unsafe(quote! { use F; }),
    ];
    assert_eq!(
        maybe_unsafes_to_tokens(items.clone(), false).to_string(),
        quote! {
            use A;
            use B;
            unsafe {
                use C;
                use D;
            }
            use E;
            unsafe {
                use F;
            }
        }
        .to_string()
    );
    assert_eq!(
        maybe_unsafes_to_tokens(items, true).to_string(),
        quote! {
            use A;
            use B;
            use C;
            use D;
            use E;
            use F;
        }
        .to_string()
    );
}
