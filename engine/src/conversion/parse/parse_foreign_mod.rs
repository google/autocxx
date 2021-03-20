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

use crate::conversion::api::{FuncToConvert, UnanalyzedApi};
use crate::{
    conversion::api::ApiDetail,
    conversion::ConvertError,
    types::{Namespace, TypeName},
};
use std::collections::{HashMap, HashSet};
use syn::{ForeignItem, Ident, ImplItem, ItemImpl, Type};

/// Parses a given bindgen-generated 'mod' into suitable
/// [Api]s. In bindgen output, a given mod concerns
/// a specific C++ namespace.
pub(crate) struct ParseForeignMod {
    ns: Namespace,
    // We mostly act upon the functions we see within the 'extern "C"'
    // block of bindgen output, but we can't actually do this until
    // we've seen the (possibly subsequent) 'impl' blocks so we can
    // deduce which functions are actually static methods. Hence
    // store them.
    funcs_to_convert: Vec<FuncToConvert>,
    // Evidence from 'impl' blocks about which of these items
    // may actually be methods (static or otherwise). Mapping from
    // function name to type name.
    method_receivers: HashMap<Ident, TypeName>,
}

impl ParseForeignMod {
    pub(crate) fn new(ns: Namespace) -> Self {
        Self {
            ns,
            funcs_to_convert: Vec::new(),
            method_receivers: HashMap::new(),
        }
    }

    /// Record information from foreign mod items encountered
    /// in bindgen output.
    pub(crate) fn convert_foreign_mod_items(
        &mut self,
        foreign_mod_items: Vec<ForeignItem>,
        virtual_this_type: Option<TypeName>,
    ) {
        for i in foreign_mod_items {
            let r = self.parse_foreign_item(i, &virtual_this_type);
            match r {
                Err(err) if err.is_ignorable() => {
                    eprintln!("Ignored item discovered whilst parsing: {}", err)
                }
                Err(_) => r.unwrap(),
                Ok(_) => {}
            }
        }
    }

    fn parse_foreign_item(
        &mut self,
        i: ForeignItem,
        virtual_this_type: &Option<TypeName>,
    ) -> Result<(), ConvertError> {
        match i {
            ForeignItem::Fn(item) => {
                self.funcs_to_convert.push(FuncToConvert {
                    item,
                    virtual_this_type: virtual_this_type.clone(),
                    self_ty: None,
                });
                Ok(())
            }
            ForeignItem::Static(item) => Err(ConvertError::StaticData(item.ident.to_string())),
            _ => Err(ConvertError::UnexpectedForeignItem),
        }
    }

    /// Record information from impl blocks encountered in bindgen
    /// output.
    pub(crate) fn convert_impl_items(&mut self, imp: ItemImpl) {
        let ty_id = match *imp.self_ty {
            Type::Path(typ) => typ.path.segments.last().unwrap().ident.clone(),
            _ => return,
        };
        for i in imp.items {
            if let ImplItem::Method(itm) = i {
                let effective_fun_name = if itm.sig.ident == "new" {
                    ty_id.clone()
                } else {
                    itm.sig.ident
                };
                self.method_receivers.insert(
                    effective_fun_name,
                    TypeName::new(&self.ns, &ty_id.to_string()),
                );
            }
        }
    }

    /// Indicate that all foreign mods and all impl blocks have been
    /// fed into us, and we should process that information to generate
    /// the resulting APIs.
    pub(crate) fn finished(&mut self, apis: &mut Vec<UnanalyzedApi>) {
        while !self.funcs_to_convert.is_empty() {
            let mut fun = self.funcs_to_convert.remove(0);
            fun.self_ty = self.method_receivers.get(&fun.item.sig.ident).cloned();
            apis.push(UnanalyzedApi {
                ns: self.ns.clone(),
                id: fun.item.sig.ident.clone(),
                deps: HashSet::new(), // filled in later - TODO make compile-time safe
                detail: ApiDetail::Function { fun, analysis: () },
            })
        }
    }
}
