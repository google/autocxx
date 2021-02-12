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

use crate::conversion::{
    api::UnanalyzedApi,
};
use crate::{
    conversion::ConvertError,
    conversion::{api::ApiDetail},
    types::{Namespace, TypeName},
};

use std::collections::{HashMap, HashSet};
use syn::{
    ForeignItem, ForeignItemFn, Ident, ImplItem, ItemImpl, Type,
};

use super::{
    super::api::Use,
};

/// Ways in which the conversion of a given extern "C" mod can
/// have more global effects or require more global knowledge outside
/// of its immediate conversion.
pub(crate) trait ForeignModParseCallbacks {
    fn add_api(&mut self, api: UnanalyzedApi);
}

/// A ForeignItemFn with a little bit of context about the
/// type which is most likely to be 'this'
struct FuncToConvert {
    item: ForeignItemFn,
    virtual_this_type: Option<TypeName>,
}

/// Converts a given bindgen-generated 'mod' into suitable
/// cxx::bridge runes. In bindgen output, a given mod concerns
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
    ) -> Result<(), ConvertError> {
        for i in foreign_mod_items {
            match i {
                ForeignItem::Fn(item) => {
                    self.funcs_to_convert.push(FuncToConvert {
                        item,
                        virtual_this_type: virtual_this_type.clone(),
                    });
                }
                _ => return Err(ConvertError::UnexpectedForeignItem),
            }
        }
        Ok(())
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
    pub(crate) fn finished(
        &mut self,
        callbacks: &mut impl ForeignModParseCallbacks,
    ) -> Result<(), ConvertError> {
        while !self.funcs_to_convert.is_empty() {
            let fun = self.funcs_to_convert.remove(0);
            let self_ty = self.method_receivers.get(&fun.item.sig.ident).cloned();
            callbacks.add_api(UnanalyzedApi {
                ns: self.ns.clone(),
                id: fun.item.sig.ident.clone(),
                use_stmt: Use::Used, // TODO
                deps: HashSet::new(), // TODO
                id_for_allowlist: Some(fun.item.sig.ident.clone()), // TODO
                additional_cpp: None, // TODO
                detail: ApiDetail::Function {
                    item: fun.item,
                    virtual_this_type: fun.virtual_this_type,
                    self_ty,
                    analysis: (),
                }
            });
        }
        Ok(())
    }

}
