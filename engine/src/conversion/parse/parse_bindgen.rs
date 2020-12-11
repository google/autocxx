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

use std::{collections::HashMap, collections::HashSet};

use crate::{
    byvalue_checker::ByValueChecker,
    conversion::{api::ParseResults, ConvertError},
    type_database::TypeDatabase,
    types::make_ident,
    types::Namespace,
    types::TypeName,
};
use proc_macro2::{TokenStream as TokenStream2, TokenTree};
use quote::quote;
use syn::{
    parse::Parser, parse_quote, Field, Fields, ForeignItem, GenericParam, Item, ItemStruct, Type,
};

use super::super::{
    api::{Api, Use},
    bridge_name_tracker::BridgeNameTracker,
    rust_name_tracker::RustNameTracker,
    type_converter::TypeConverter,
    utilities::generate_utilities,
};

use super::parse_foreign_mod::{ForeignModParseCallbacks, ParseForeignMod};

#[derive(Copy, Clone, Eq, PartialEq)]
enum TypeKind {
    POD,                // trivial. Can be moved and copied in Rust.
    NonPOD, // has destructor or non-trivial move constructors. Can only hold by UniquePtr
    ForwardDeclaration, // no full C++ declaration available - can't even generate UniquePtr
}

/// Parses a bindgen mod in order to understand the APIs within it.
pub(crate) struct ParseBindgen<'a> {
    type_converter: TypeConverter,
    byvalue_checker: ByValueChecker,
    type_database: &'a TypeDatabase,
    bridge_name_tracker: BridgeNameTracker,
    rust_name_tracker: RustNameTracker,
    incomplete_types: HashSet<TypeName>,
    results: ParseResults,
}

impl<'a> ParseBindgen<'a> {
    pub(crate) fn new(byvalue_checker: ByValueChecker, type_database: &'a TypeDatabase) -> Self {
        ParseBindgen {
            type_converter: TypeConverter::new(),
            byvalue_checker,
            bridge_name_tracker: BridgeNameTracker::new(),
            rust_name_tracker: RustNameTracker::new(),
            type_database,
            incomplete_types: HashSet::new(),
            results: ParseResults {
                apis: Vec::new(),
                extern_c_mod: None,
                use_stmts_by_mod: HashMap::new(),
            },
        }
    }

    /// Main function which goes through and performs conversion from
    /// `bindgen`-style Rust output into `cxx::bridge`-style Rust input.
    /// At present, it significantly rewrites the bindgen mod,
    /// as well as generating an additional cxx::bridge mod, and an outer
    /// mod with all sorts of 'use' statements. A valid alternative plan
    /// might be to keep the bindgen mod untouched and _only_ generate
    /// additional bindings, but the sticking point there is that it's not
    /// obviously possible to stop folks allocating opaque types in the
    /// bindgen mod. (We mark all types as opaque until we're told
    /// otherwise, which is the opposite of what bindgen does, so we can't
    /// just give it lots of directives to make all types opaque.)
    /// One future option could be to provide a mode to bindgen where
    /// everything is opaque unless specifically allowlisted to be
    /// transparent.
    pub(crate) fn convert_items(
        mut self,
        items: Vec<Item>,
        exclude_utilities: bool,
    ) -> Result<ParseResults, ConvertError> {
        if !exclude_utilities {
            generate_utilities(&mut self.results.apis);
        }
        let root_ns = Namespace::new();
        self.convert_mod_items(items, root_ns)?;
        Ok(self.results)
    }

    /// Interpret the bindgen-generated .rs for a particular
    /// mod, which corresponds to a C++ namespace.
    fn convert_mod_items(&mut self, items: Vec<Item>, ns: Namespace) -> Result<(), ConvertError> {
        // This object maintains some state specific to this namespace, i.e.
        // this particular mod.
        let mut mod_converter = ParseForeignMod::new(ns.clone());
        let mut use_statements_for_this_mod = Vec::new();
        for item in items {
            match item {
                Item::ForeignMod(mut fm) => {
                    let items = fm.items;
                    fm.items = Vec::new();
                    if self.results.extern_c_mod.is_none() {
                        self.results.extern_c_mod = Some(fm);
                        // We'll use the first 'extern "C"' mod we come
                        // across for attributes, spans etc. but we'll stuff
                        // the contents of all bindgen 'extern "C"' mods into this
                        // one.
                    }
                    mod_converter.convert_foreign_mod_items(items)?;
                }
                Item::Struct(mut s) => {
                    let tyname = TypeName::new(&ns, &s.ident.to_string());
                    let type_kind = if Self::spot_forward_declaration(&s.fields) {
                        self.incomplete_types.insert(tyname.clone());
                        TypeKind::ForwardDeclaration
                    } else if self.byvalue_checker.is_pod(&tyname) {
                        TypeKind::POD
                    } else {
                        TypeKind::NonPOD
                    };
                    // We either leave a bindgen struct untouched, or we completely
                    // replace its contents with opaque nonsense.
                    let field_types = match type_kind {
                        TypeKind::POD => self.get_struct_field_types(&ns, &s)?,
                        _ => {
                            Self::make_non_pod(&mut s);
                            HashSet::new()
                        }
                    };
                    // cxx::bridge can't cope with type aliases to generic
                    // types at the moment.
                    self.generate_type(tyname, type_kind, field_types, Some(Item::Struct(s)))?;
                }
                Item::Enum(e) => {
                    let tyname = TypeName::new(&ns, &e.ident.to_string());
                    self.generate_type(tyname, TypeKind::POD, HashSet::new(), Some(Item::Enum(e)))?;
                }
                Item::Impl(imp) => {
                    // We *mostly* ignore all impl blocks generated by bindgen.
                    // Methods also appear in 'extern "C"' blocks which
                    // we will convert instead. At that time we'll also construct
                    // synthetic impl blocks.
                    // We do however record which methods were spotted, since
                    // we have no other way of working out which functions are
                    // static methods vs plain functions.
                    mod_converter.convert_impl_items(imp);
                }
                Item::Mod(itm) => {
                    if let Some((_, items)) = itm.content {
                        let new_ns = ns.push(itm.ident.to_string());
                        self.convert_mod_items(items, new_ns)?;
                    }
                }
                Item::Use(_) => {
                    use_statements_for_this_mod.push(item);
                }
                Item::Const(itc) => {
                    // TODO the following puts this constant into
                    // the global namespace which is bug
                    // https://github.com/google/autocxx/issues/133
                    self.add_api(Api {
                        id: itc.ident.clone(),
                        ns: ns.clone(),
                        bridge_item: None,
                        extern_c_mod_item: None,
                        global_items: vec![Item::Const(itc)],
                        additional_cpp: None,
                        deps: HashSet::new(),
                        use_stmt: Use::Unused,
                        id_for_allowlist: None,
                        bindgen_mod_item: None,
                    });
                }
                Item::Type(ity) => {
                    let tyname = TypeName::new(&ns, &ity.ident.to_string());
                    self.type_converter.insert_typedef(tyname, ity.ty.as_ref());
                    self.add_api(Api {
                        id: ity.ident.clone(),
                        ns: ns.clone(),
                        bridge_item: None,
                        extern_c_mod_item: None,
                        global_items: Vec::new(),
                        additional_cpp: None,
                        deps: HashSet::new(),
                        use_stmt: Use::Unused,
                        id_for_allowlist: None,
                        bindgen_mod_item: Some(Item::Type(ity)),
                    });
                }
                _ => return Err(ConvertError::UnexpectedItemInMod),
            }
        }
        mod_converter.finished(self)?;

        // We don't immediately blat 'use' statements into any particular
        // Api. We'll squirrel them away and insert them into the output mod later
        // iff this mod ends up having any output items after garbage collection
        // of unnecessary APIs.
        let supers = std::iter::repeat(make_ident("super")).take(ns.depth() + 2);
        use_statements_for_this_mod.push(Item::Use(parse_quote! {
            #[allow(unused_imports)]
            use self::
                #(#supers)::*
            ::cxxbridge;
        }));
        for thing in &["UniquePtr", "CxxString"] {
            let thing = make_ident(thing);
            use_statements_for_this_mod.push(Item::Use(parse_quote! {
                #[allow(unused_imports)]
                use cxx:: #thing;
            }));
        }
        self.results
            .use_stmts_by_mod
            .insert(ns, use_statements_for_this_mod);
        Ok(())
    }

    fn get_struct_field_types(
        &self,
        ns: &Namespace,
        s: &ItemStruct,
    ) -> Result<HashSet<TypeName>, ConvertError> {
        let mut results = HashSet::new();
        for f in &s.fields {
            let annotated = self.type_converter.convert_type(f.ty.clone(), ns)?;
            results.extend(annotated.types_encountered);
        }
        Ok(results)
    }

    fn spot_forward_declaration(s: &Fields) -> bool {
        s.iter()
            .filter_map(|f| f.ident.as_ref())
            .any(|id| id == "_unused")
    }

    fn make_non_pod(s: &mut ItemStruct) {
        // Thanks to dtolnay@ for this explanation of why the following
        // is needed:
        // If the real alignment of the C++ type is smaller and a reference
        // is returned from C++ to Rust, mere existence of an insufficiently
        // aligned reference in Rust causes UB even if never dereferenced
        // by Rust code
        // (see https://doc.rust-lang.org/1.47.0/reference/behavior-considered-undefined.html).
        // Rustc can use least-significant bits of the reference for other storage.
        s.attrs = vec![parse_quote!(
            #[repr(C, packed)]
        )];
        // Now fill in fields. Usually, we just want a single field
        // but if this is a generic type we need to faff a bit.
        let generic_type_fields =
            s.generics
                .params
                .iter()
                .enumerate()
                .filter_map(|(counter, gp)| match gp {
                    GenericParam::Type(gpt) => {
                        let id = &gpt.ident;
                        let field_name = make_ident(&format!("_phantom_{}", counter));
                        let toks = quote! {
                            #field_name: ::std::marker::PhantomData<::std::cell::UnsafeCell< #id >>
                        };
                        let parser = Field::parse_named;
                        Some(parser.parse2(toks).unwrap())
                    }
                    _ => None,
                });
        // See cxx's opaque::Opaque for rationale for this type... in
        // short, it's to avoid being Send/Sync.
        s.fields = syn::Fields::Named(parse_quote! {
            {
                do_not_attempt_to_allocate_nonpod_types: [*const u8; 0],
                #(#generic_type_fields),*
            }
        });
    }

    /// Record the Api for a type, e.g. enum or struct.
    /// Code generated includes the bindgen entry itself,
    /// various entries for the cxx::bridge to ensure cxx
    /// is aware of the type, and 'use' statements for the final
    /// output mod hierarchy. All are stored in the Api which
    /// this adds.
    fn generate_type(
        &mut self,
        tyname: TypeName,
        type_nature: TypeKind,
        deps: HashSet<TypeName>,
        bindgen_mod_item: Option<Item>,
    ) -> Result<(), ConvertError> {
        let final_ident = make_ident(tyname.get_final_ident());
        let kind_item = match type_nature {
            TypeKind::POD => "Trivial",
            _ => "Opaque",
        };
        let kind_item = make_ident(kind_item);
        let effective_type = self
            .type_database
            .get_effective_type(&tyname)
            .unwrap_or(&tyname);
        if self.type_database.is_on_blocklist(effective_type) {
            return Ok(());
        }
        let tynamestring = effective_type.to_cpp_name();
        let mut for_extern_c_ts = if effective_type.has_namespace() {
            let ns_string = effective_type
                .ns_segment_iter()
                .cloned()
                .collect::<Vec<String>>()
                .join("::");
            quote! {
                #[namespace = #ns_string]
            }
        } else {
            TokenStream2::new()
        };

        let mut fulltypath = Vec::new();
        // We can't use parse_quote! here because it doesn't support type aliases
        // at the moment.
        let colon = TokenTree::Punct(proc_macro2::Punct::new(':', proc_macro2::Spacing::Joint));
        for_extern_c_ts.extend(
            [
                TokenTree::Ident(make_ident("type")),
                TokenTree::Ident(final_ident.clone()),
                TokenTree::Punct(proc_macro2::Punct::new('=', proc_macro2::Spacing::Alone)),
                TokenTree::Ident(make_ident("super")),
                colon.clone(),
                colon.clone(),
                TokenTree::Ident(make_ident("bindgen")),
                colon.clone(),
                colon.clone(),
                TokenTree::Ident(make_ident("root")),
                colon.clone(),
                colon.clone(),
            ]
            .to_vec(),
        );
        fulltypath.push(make_ident("bindgen"));
        fulltypath.push(make_ident("root"));
        for segment in tyname.ns_segment_iter() {
            let id = make_ident(segment);
            for_extern_c_ts
                .extend([TokenTree::Ident(id.clone()), colon.clone(), colon.clone()].to_vec());
            fulltypath.push(id);
        }
        for_extern_c_ts.extend(
            [
                TokenTree::Ident(final_ident.clone()),
                TokenTree::Punct(proc_macro2::Punct::new(';', proc_macro2::Spacing::Alone)),
            ]
            .to_vec(),
        );
        let bridge_item = match type_nature {
            TypeKind::ForwardDeclaration => None,
            _ => Some(Item::Impl(parse_quote! {
                impl UniquePtr<#final_ident> {}
            })),
        };
        fulltypath.push(final_ident.clone());
        let api = Api {
            ns: tyname.get_namespace().clone(),
            id: final_ident,
            use_stmt: Use::Used,
            global_items: vec![Item::Impl(parse_quote! {
                unsafe impl cxx::ExternType for #(#fulltypath)::* {
                    type Id = cxx::type_id!(#tynamestring);
                    type Kind = cxx::kind::#kind_item;
                }
            })],
            bridge_item,
            extern_c_mod_item: Some(ForeignItem::Verbatim(for_extern_c_ts)),
            additional_cpp: None,
            deps,
            id_for_allowlist: None,
            bindgen_mod_item,
        };
        self.add_api(api);
        self.type_converter.push(tyname);
        Ok(())
    }
}

impl<'a> ForeignModParseCallbacks for ParseBindgen<'a> {
    fn convert_boxed_type(
        &self,
        ty: Box<Type>,
        ns: &Namespace,
    ) -> Result<(Box<Type>, HashSet<TypeName>), ConvertError> {
        let annotated = self.type_converter.convert_boxed_type(ty, ns)?;
        Ok((annotated.ty, annotated.types_encountered))
    }

    fn is_pod(&self, ty: &TypeName) -> bool {
        self.byvalue_checker.is_pod(ty)
    }

    fn add_api(&mut self, api: Api) {
        self.results.apis.push(api);
    }

    fn get_cxx_bridge_name(
        &mut self,
        type_name: Option<&str>,
        found_name: &str,
        ns: &Namespace,
    ) -> String {
        self.bridge_name_tracker
            .get_unique_cxx_bridge_name(type_name, found_name, ns)
    }

    fn ok_to_use_rust_name(&mut self, rust_name: &str) -> bool {
        self.rust_name_tracker.ok_to_use_rust_name(rust_name)
    }

    fn is_on_allowlist(&self, type_name: &TypeName) -> bool {
        self.type_database.is_on_allowlist(type_name)
    }

    fn avoid_generating_type(&self, type_name: &TypeName) -> bool {
        self.type_database.is_on_blocklist(type_name) || self.incomplete_types.contains(type_name)
    }
}
