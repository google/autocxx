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
        api::{ApiCommon, TypedefKind, UnanalyzedApi},
        ConvertError,
    },
    types::Namespace,
    types::QualifiedName,
};
use crate::{
    conversion::{
        convert_error::{ConvertErrorWithContext, ErrorContext},
        error_reporter::report_any_error,
    },
    types::validate_ident_ok_for_cxx,
};
use autocxx_parser::IncludeCppConfig;
use syn::{parse_quote, Attribute, Fields, Ident, Item, LitStr, TypePath, UseTree};

use super::super::utilities::generate_utilities;

use super::parse_foreign_mod::ParseForeignMod;

/// Parses a bindgen mod in order to understand the APIs within it.
pub(crate) struct ParseBindgen<'a> {
    config: &'a IncludeCppConfig,
    apis: Vec<UnanalyzedApi>,
    /// Here we track the last struct which bindgen told us about.
    /// Any subsequent "extern 'C'" blocks are methods belonging to that type,
    /// even if the 'this' is actually recorded as void in the
    /// function signature.
    latest_virtual_this_type: Option<QualifiedName>,
}

pub(crate) fn api_common(ns: &Namespace, id: Ident, attrs: &[Attribute]) -> ApiCommon {
    api_common_with_deps(ns, id, attrs, &[])
}

pub(crate) fn api_common_with_deps(
    ns: &Namespace,
    id: Ident,
    attrs: &[Attribute],
    deps: &[QualifiedName],
) -> ApiCommon {
    ApiCommon {
        name: QualifiedName::new(ns, id),
        cpp_name: get_bindgen_original_name_annotation(attrs),
        deps: deps.iter().cloned().collect(),
    }
}

pub(crate) fn api_common_qualified(
    ns: &Namespace,
    id: Ident,
    attrs: &[Attribute],
) -> Result<ApiCommon, ConvertErrorWithContext> {
    match validate_ident_ok_for_cxx(&id.to_string()) {
        Err(e) => {
            let ctx = ErrorContext::Item(id);
            Err(ConvertErrorWithContext(e, Some(ctx)))
        }
        Ok(..) => Ok(api_common_with_deps(ns, id, attrs, &[])),
    }
}

fn get_bindgen_original_name_annotation(attrs: &[Attribute]) -> Option<String> {
    attrs
        .iter()
        .filter_map(|a| {
            if a.path.is_ident("bindgen_original_name") {
                let r: Result<LitStr, syn::Error> = a.parse_args();
                match r {
                    Ok(ls) => Some(ls.value()),
                    Err(_) => None,
                }
            } else {
                None
            }
        })
        .next()
}

impl<'a> ParseBindgen<'a> {
    pub(crate) fn new(config: &'a IncludeCppConfig) -> Self {
        ParseBindgen {
            config,
            apis: Vec::new(),
            latest_virtual_this_type: None,
        }
    }

    /// Parses items found in the `bindgen` output and returns a set of
    /// `Api`s together with some other data.
    pub(crate) fn parse_items(
        mut self,
        items: Vec<Item>,
    ) -> Result<Vec<UnanalyzedApi>, ConvertError> {
        let items = Self::find_items_in_root(items)?;
        if !self.config.exclude_utilities() {
            generate_utilities(&mut self.apis, &self.config);
        }
        let root_ns = Namespace::new();
        self.parse_mod_items(items, root_ns);
        self.confirm_all_generate_directives_obeyed()?;
        Ok(self.apis)
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
        self.apis.append(&mut more_apis);
        mod_converter.finished(&mut self.apis);
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
                let is_forward_declaration = Self::spot_forward_declaration(&s.fields);
                // cxx::bridge can't cope with type aliases to generic
                // types at the moment.
                let common = api_common_qualified(ns, s.ident.clone(), &s.attrs)?;
                let api = if is_forward_declaration {
                    UnanalyzedApi::ForwardDeclaration { common }
                } else {
                    UnanalyzedApi::Struct {
                        common,
                        item: s,
                        analysis: (),
                    }
                };
                self.latest_virtual_this_type = Some(api.name().clone());
                if !self.config.is_on_blocklist(&api.name().to_cpp_name()) {
                    self.apis.push(api);
                }
                Ok(())
            }
            Item::Enum(e) => {
                let api = UnanalyzedApi::Enum {
                    common: api_common_qualified(ns, e.ident.clone(), &e.attrs)?,
                    item: e,
                };
                if !self.config.is_on_blocklist(&api.name().to_cpp_name()) {
                    self.apis.push(api);
                }
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
                            self.apis.push(UnanalyzedApi::Typedef {
                                common: api_common_with_deps(
                                    ns,
                                    new_id.clone(),
                                    &use_item.attrs,
                                    &[old_tyname],
                                ),
                                item: TypedefKind::Use(parse_quote! {
                                    pub use #old_path as #new_id;
                                }),
                                analysis: (),
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
                self.apis.push(UnanalyzedApi::Const {
                    common: api_common(ns, const_item.ident.clone(), &const_item.attrs),
                    const_item,
                });
                Ok(())
            }
            Item::Type(ity) => {
                self.apis.push(UnanalyzedApi::Typedef {
                    common: api_common(ns, ity.ident.clone(), &ity.attrs),
                    item: TypedefKind::Type(ity),
                    analysis: (),
                });
                Ok(())
            }
            _ => Err(ConvertErrorWithContext(
                ConvertError::UnexpectedItemInMod,
                None,
            )),
        }
    }

    fn spot_forward_declaration(s: &Fields) -> bool {
        s.iter()
            .filter_map(|f| f.ident.as_ref())
            .any(|id| id == "_unused")
    }

    fn confirm_all_generate_directives_obeyed(&self) -> Result<(), ConvertError> {
        let api_names: HashSet<_> = self
            .apis
            .iter()
            .map(|api| api.name().to_cpp_name())
            .collect();
        for generate_directive in self.config.must_generate_list() {
            if !api_names.contains(&generate_directive) {
                return Err(ConvertError::DidNotGenerateAnything(generate_directive));
            }
        }
        Ok(())
    }
}
