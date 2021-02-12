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
    conversion::{
        api::{ApiDetail, ParseResults, TypeApiDetails, UnanalyzedApi},
        ConvertError,
    },
    types::make_ident,
    types::Namespace,
    types::TypeName,
};
use autocxx_parser::TypeDatabase;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{parse_quote, Fields, Item};

use super::{
    super::{api::Use, utilities::generate_utilities},
    type_converter::TypeConverter,
};

use super::parse_foreign_mod::ParseForeignMod;

/// Parses a bindgen mod in order to understand the APIs within it.
pub(crate) struct ParseBindgen<'a> {
    type_converter: &'a mut TypeConverter,
    type_database: &'a TypeDatabase,
    results: ParseResults,
    /// Here we track the last struct which bindgen told us about.
    /// Any subsequent "extern 'C'" blocks are methods belonging to that type,
    /// even if the 'this' is actually recorded as void in the
    /// function signature.
    latest_virtual_this_type: Option<TypeName>,
}

impl<'a> ParseBindgen<'a> {
    pub(crate) fn new(
        type_database: &'a TypeDatabase,
        type_converter: &'a mut TypeConverter,
    ) -> Self {
        ParseBindgen {
            type_converter,
            type_database,
            results: ParseResults {
                apis: Vec::new(),
                use_stmts_by_mod: HashMap::new(),
                incomplete_types: HashSet::new(),
            },
            latest_virtual_this_type: None,
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
                Item::ForeignMod(fm) => {
                    mod_converter.convert_foreign_mod_items(
                        fm.items,
                        self.latest_virtual_this_type.clone(),
                    )?;
                }
                Item::Struct(s) => {
                    if s.ident.to_string().ends_with("__bindgen_vtable") {
                        continue;
                    }
                    let tyname = TypeName::new(&ns, &s.ident.to_string());
                    let is_forward_declaration = Self::spot_forward_declaration(&s.fields);
                    if is_forward_declaration {
                        self.results.incomplete_types.insert(tyname.clone());
                    }
                    // cxx::bridge can't cope with type aliases to generic
                    // types at the moment.
                    self.generate_type(
                        tyname.clone(),
                        is_forward_declaration,
                        HashSet::new(),
                        Some(Item::Struct(s)),
                    );
                    self.latest_virtual_this_type = Some(tyname);
                }
                Item::Enum(e) => {
                    let tyname = TypeName::new(&ns, &e.ident.to_string());
                    self.generate_type(tyname, false, HashSet::new(), Some(Item::Enum(e)));
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
                Item::Const(const_item) => {
                    // The following puts this constant into
                    // the global namespace which is bug
                    // https://github.com/google/autocxx/issues/133
                    self.results.apis.push(UnanalyzedApi {
                        id: const_item.ident.clone(),
                        ns: ns.clone(),
                        deps: HashSet::new(),
                        use_stmt: Use::Unused,
                        id_for_allowlist: None,
                        detail: ApiDetail::Const { const_item },
                        additional_cpp: None,
                    });
                }
                Item::Type(mut ity) => {
                    let tyname = TypeName::new(&ns, &ity.ident.to_string());
                    let mut final_type = self.type_converter.convert_type(*ity.ty, &ns, false)?;
                    ity.ty = Box::new(final_type.ty.clone());
                    self.type_converter.insert_typedef(tyname, final_type.ty);
                    self.results.apis.append(&mut final_type.extra_apis);
                    self.results.apis.push(UnanalyzedApi {
                        id: ity.ident.clone(),
                        ns: ns.clone(),
                        deps: final_type.types_encountered,
                        use_stmt: Use::Unused,
                        id_for_allowlist: None,
                        additional_cpp: None,
                        detail: ApiDetail::Typedef { type_item: ity },
                    });
                }
                _ => return Err(ConvertError::UnexpectedItemInMod),
            }
        }
        mod_converter.finished(&mut self.results.apis);

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
        use_statements_for_this_mod.push(Item::Use(parse_quote! {
            #[allow(unused_imports)]
            use std::pin::Pin;
        }));
        self.results
            .use_stmts_by_mod
            .insert(ns, use_statements_for_this_mod);
        Ok(())
    }

    fn spot_forward_declaration(s: &Fields) -> bool {
        s.iter()
            .filter_map(|f| f.ident.as_ref())
            .any(|id| id == "_unused")
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
        is_forward_declaration: bool,
        deps: HashSet<TypeName>,
        bindgen_mod_item: Option<Item>,
    ) {
        let final_ident = make_ident(tyname.get_final_ident());
        if self.type_database.is_on_blocklist(&tyname.to_cpp_name()) {
            return;
        }
        let tynamestring = tyname.to_cpp_name();
        let mut for_extern_c_ts = if tyname.has_namespace() {
            let ns_string = tyname
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

        let mut fulltypath: Vec<_> = ["bindgen", "root"].iter().map(make_ident).collect();
        for_extern_c_ts.extend(quote! {
            type #final_ident = super::bindgen::root::
        });
        for segment in tyname.ns_segment_iter() {
            let id = make_ident(segment);
            for_extern_c_ts.extend(quote! {
                #id::
            });
            fulltypath.push(id);
        }
        for_extern_c_ts.extend(quote! {
            #final_ident;
        });
        fulltypath.push(final_ident.clone());
        let api = UnanalyzedApi {
            ns: tyname.get_namespace().clone(),
            id: final_ident.clone(),
            use_stmt: Use::Used,
            deps,
            id_for_allowlist: None,
            additional_cpp: None,
            detail: ApiDetail::Type {
                ty_details: TypeApiDetails {
                    fulltypath,
                    final_ident,
                    tynamestring,
                },
                for_extern_c_ts,
                is_forward_declaration,
                bindgen_mod_item,
                analysis: (),
            },
        };
        self.results.apis.push(api);
        self.type_converter.push(tyname);
    }
}
