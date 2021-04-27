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

use std::collections::HashSet;

use crate::{
    conversion::{
        api::{ApiDetail, ParseResults, TypedefKind, UnanalyzedApi},
        ConvertError,
    },
    types::Namespace,
    types::QualifiedName,
};
use crate::{
    conversion::{
        convert_error::{ConvertErrorWithContext, ErrorContext},
        error_reporter::report_any_error,
        parse::type_converter::Annotated,
    },
    types::validate_ident_ok_for_cxx,
};
use autocxx_parser::TypeConfig;
use syn::{parse_quote, Fields, Ident, Item, Type, TypePath, UseTree};

use super::{super::utilities::generate_utilities, type_converter::TypeConverter};

use super::parse_foreign_mod::ParseForeignMod;

/// Parses a bindgen mod in order to understand the APIs within it.
pub(crate) struct ParseBindgen<'a> {
    type_config: &'a TypeConfig,
    results: ParseResults<'a>,
    /// Here we track the last struct which bindgen told us about.
    /// Any subsequent "extern 'C'" blocks are methods belonging to that type,
    /// even if the 'this' is actually recorded as void in the
    /// function signature.
    latest_virtual_this_type: Option<QualifiedName>,
}

impl<'a> ParseBindgen<'a> {
    pub(crate) fn new(type_config: &'a TypeConfig) -> Self {
        ParseBindgen {
            type_config,
            results: ParseResults {
                apis: Vec::new(),
                type_converter: TypeConverter::new(type_config),
            },
            latest_virtual_this_type: None,
        }
    }

    /// Parses items found in the `bindgen` output and returns a set of
    /// `Api`s together with some other data.
    pub(crate) fn parse_items(
        mut self,
        items: Vec<Item>,
        exclude_utilities: bool,
    ) -> Result<ParseResults<'a>, ConvertError> {
        let items = Self::find_items_in_root(items)?;
        if !exclude_utilities {
            generate_utilities(&mut self.results.apis);
        }
        let root_ns = Namespace::new();
        self.parse_mod_items(items, root_ns);
        self.confirm_all_generate_directives_obeyed()?;
        Ok(self.results)
    }

    fn find_items_in_root(items: Vec<Item>) -> Result<Vec<Item>, ConvertError> {
        for item in items {
            match item {
                Item::Mod(root_mod) => {
                    // With namespaces enabled, bindgen always puts everything
                    // in a mod called 'root'. We don't want to pass that
                    // onto cxx, so jump right into it.
                    assert!(root_mod.ident == "root");
                    if let Some((_, items)) = root_mod.content {
                        return Ok(items);
                    }
                }
                _ => return Err(ConvertError::UnexpectedOuterItem),
            }
        }
        Ok(Vec::new())
    }

    /// Interpret the bindgen-generated .rs for a particular
    /// mod, which corresponds to a C++ namespace.
    fn parse_mod_items(&mut self, items: Vec<Item>, ns: Namespace) {
        // This object maintains some state specific to this namespace, i.e.
        // this particular mod.
        let mut mod_converter = ParseForeignMod::new(ns.clone());
        let mut more_apis = Vec::new();
        for item in items {
            report_any_error(&ns, &mut more_apis, || {
                self.parse_item(item, &mut mod_converter, &ns)
            });
        }
        self.results.apis.append(&mut more_apis);
        mod_converter.finished(&mut self.results.apis);
    }

    fn parse_item(
        &mut self,
        item: Item,
        mod_converter: &mut ParseForeignMod,
        ns: &Namespace,
    ) -> Result<(), ConvertErrorWithContext> {
        match item {
            Item::ForeignMod(fm) => {
                mod_converter
                    .convert_foreign_mod_items(fm.items, self.latest_virtual_this_type.clone());
                Ok(())
            }
            Item::Struct(s) => {
                if s.ident.to_string().ends_with("__bindgen_vtable") {
                    return Ok(());
                }
                let tyname = Self::qualify_name(ns, s.ident.clone())?;
                let is_forward_declaration = Self::spot_forward_declaration(&s.fields);
                // cxx::bridge can't cope with type aliases to generic
                // types at the moment.
                self.parse_type(
                    tyname.clone(),
                    is_forward_declaration,
                    HashSet::new(),
                    Some(Item::Struct(s)),
                );
                self.latest_virtual_this_type = Some(tyname);
                Ok(())
            }
            Item::Enum(e) => {
                let tyname = Self::qualify_name(ns, e.ident.clone())?;
                self.parse_type(tyname, false, HashSet::new(), Some(Item::Enum(e)));
                Ok(())
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
                Ok(())
            }
            Item::Mod(itm) => {
                if let Some((_, items)) = itm.content {
                    let new_ns = ns.push(itm.ident.to_string());
                    self.parse_mod_items(items, new_ns);
                }
                Ok(())
            }
            Item::Use(use_item) => {
                let mut segs = Vec::new();
                let mut tree = &use_item.tree;
                loop {
                    match tree {
                        UseTree::Path(up) => {
                            segs.push(up.ident.clone());
                            tree = &up.tree;
                        }
                        UseTree::Name(un) if un.ident == "root" => break, // we do not add this to any API since we generate equivalent
                        // use statements in our codegen phase.
                        UseTree::Rename(urn) => {
                            let old_id = &urn.ident;
                            let new_id = &urn.rename;
                            let new_tyname = QualifiedName::new(ns, new_id.clone());
                            if segs.remove(0) != "self" {
                                panic!("Path didn't start with self");
                            }
                            if segs.remove(0) != "super" {
                                panic!("Path didn't start with self::super");
                            }
                            // This is similar to the path encountered within 'tree'
                            // but without the self::super prefix which is unhelpful
                            // in our output mod, because we prefer relative paths
                            // (we're nested in another mod)
                            let old_path: TypePath = parse_quote! {
                                #(#segs)::* :: #old_id
                            };
                            let old_tyname = QualifiedName::from_type_path(&old_path);
                            if new_tyname == old_tyname {
                                return Err(ConvertErrorWithContext(
                                    ConvertError::InfinitelyRecursiveTypedef(new_tyname),
                                    Some(ErrorContext::Item(new_id.clone())),
                                ));
                            }
                            self.results
                                .type_converter
                                .insert_typedef(new_tyname, Type::Path(old_path.clone()));
                            let mut deps = HashSet::new();
                            deps.insert(old_tyname);
                            self.results.apis.push(UnanalyzedApi {
                                name: QualifiedName::new(ns, new_id.clone()),
                                deps,
                                detail: ApiDetail::Typedef {
                                    payload: TypedefKind::Use(parse_quote! {
                                        pub use #old_path as #new_id;
                                    }),
                                },
                            });
                            break;
                        }
                        _ => {
                            return Err(ConvertErrorWithContext(
                                ConvertError::UnexpectedUseStatement(segs.into_iter().last()),
                                None,
                            ))
                        }
                    }
                }
                Ok(())
            }
            Item::Const(const_item) => {
                // The following puts this constant into
                // the global namespace which is bug
                // https://github.com/google/autocxx/issues/133
                self.results.apis.push(UnanalyzedApi {
                    name: QualifiedName::new(ns, const_item.ident.clone()),
                    deps: HashSet::new(),
                    detail: ApiDetail::Const { const_item },
                });
                Ok(())
            }
            Item::Type(mut ity) => {
                let tyname = QualifiedName::new(ns, ity.ident.clone());
                let type_conversion_results =
                    self.results
                        .type_converter
                        .convert_type(*ity.ty, ns, false, &HashSet::new());
                match type_conversion_results {
                    Err(ConvertError::OpaqueTypeFound) => {
                        self.add_opaque_type(tyname);
                        Ok(())
                    }
                    Err(err) => Err(ConvertErrorWithContext(
                        err,
                        Some(ErrorContext::Item(ity.ident.clone())),
                    )),
                    Ok(Annotated {
                        ty: syn::Type::Path(ref typ),
                        ..
                    }) if QualifiedName::from_type_path(typ) == tyname => {
                        Err(ConvertErrorWithContext(
                            ConvertError::InfinitelyRecursiveTypedef(tyname),
                            Some(ErrorContext::Item(ity.ident)),
                        ))
                    }
                    Ok(mut final_type) => {
                        ity.ty = Box::new(final_type.ty.clone());
                        self.results
                            .type_converter
                            .insert_typedef(tyname, final_type.ty);
                        self.results.apis.append(&mut final_type.extra_apis);
                        self.results.apis.push(UnanalyzedApi {
                            name: QualifiedName::new(ns, ity.ident.clone()),
                            deps: final_type.types_encountered,
                            detail: ApiDetail::Typedef {
                                payload: TypedefKind::Type(ity),
                            },
                        });
                        Ok(())
                    }
                }
            }
            _ => Err(ConvertErrorWithContext(
                ConvertError::UnexpectedItemInMod,
                None,
            )),
        }
    }

    fn qualify_name(ns: &Namespace, id: Ident) -> Result<QualifiedName, ConvertErrorWithContext> {
        match validate_ident_ok_for_cxx(&id.to_string()) {
            Err(e) => {
                let ctx = ErrorContext::Item(id);
                Err(ConvertErrorWithContext(e, Some(ctx)))
            }
            Ok(..) => Ok(QualifiedName::new(ns, id)),
        }
    }

    fn spot_forward_declaration(s: &Fields) -> bool {
        s.iter()
            .filter_map(|f| f.ident.as_ref())
            .any(|id| id == "_unused")
    }

    fn add_opaque_type(&mut self, name: QualifiedName) {
        self.results.apis.push(UnanalyzedApi {
            name,
            deps: HashSet::new(),
            detail: ApiDetail::OpaqueTypedef,
        });
    }

    /// Record the Api for a type, e.g. enum or struct.
    /// Code generated includes the bindgen entry itself,
    /// various entries for the cxx::bridge to ensure cxx
    /// is aware of the type, and 'use' statements for the final
    /// output mod hierarchy. All are stored in the Api which
    /// this adds.
    fn parse_type(
        &mut self,
        name: QualifiedName,
        is_forward_declaration: bool,
        deps: HashSet<QualifiedName>,
        bindgen_mod_item: Option<Item>,
    ) {
        if self.type_config.is_on_blocklist(&name.to_cpp_name()) {
            return;
        }
        let api = UnanalyzedApi {
            name: name.clone(),
            deps,
            detail: if is_forward_declaration {
                ApiDetail::ForwardDeclaration
            } else {
                ApiDetail::Type {
                    bindgen_mod_item,
                    analysis: (),
                }
            },
        };
        self.results.apis.push(api);
        self.results.type_converter.push(name);
    }

    fn confirm_all_generate_directives_obeyed(&self) -> Result<(), ConvertError> {
        let api_names: HashSet<_> = self
            .results
            .apis
            .iter()
            .map(|api| api.typename().to_cpp_name())
            .collect();
        for generate_directive in self.type_config.allowlist() {
            if !api_names.contains(generate_directive) {
                return Err(ConvertError::DidNotGenerateAnything(
                    generate_directive.into(),
                ));
            }
        }
        Ok(())
    }
}
