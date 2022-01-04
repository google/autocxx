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

use crate::conversion::api::ApiName;
use crate::conversion::doc_attr::get_doc_attr;
use crate::conversion::error_reporter::report_any_error;
use crate::conversion::{
    api::{FuncToConvert, UnanalyzedApi},
    convert_error::ConvertErrorWithContext,
    convert_error::ErrorContext,
};
use crate::{
    conversion::ConvertError,
    types::{Namespace, QualifiedName},
};
use std::collections::{HashMap, HashSet};
use syn::{
    Block, Expr, ExprCall, ForeignItem, ForeignItemFn, Ident, ImplItem, ItemImpl, LitStr, Stmt,
    Type,
};

use super::parse_bindgen::{
    get_bindgen_original_name_annotation, get_cpp_visibility, get_virtualness, has_attr,
};

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
    method_receivers: HashMap<Ident, QualifiedName>,
    ignored_apis: Vec<UnanalyzedApi>,
}

impl ParseForeignMod {
    pub(crate) fn new(ns: Namespace) -> Self {
        Self {
            ns,
            funcs_to_convert: Vec::new(),
            method_receivers: HashMap::new(),
            ignored_apis: Vec::new(),
        }
    }

    /// Record information from foreign mod items encountered
    /// in bindgen output.
    pub(crate) fn convert_foreign_mod_items(&mut self, foreign_mod_items: Vec<ForeignItem>) {
        let mut extra_apis = Vec::new();
        for i in foreign_mod_items {
            report_any_error(&self.ns.clone(), &mut extra_apis, || {
                self.parse_foreign_item(i)
            });
        }
        self.ignored_apis.append(&mut extra_apis);
    }

    fn parse_foreign_item(&mut self, i: ForeignItem) -> Result<(), ConvertErrorWithContext> {
        match i {
            ForeignItem::Fn(item) => {
                let cpp_vis = get_cpp_visibility(&item.attrs);
                let virtualness = get_virtualness(&item.attrs);
                let unused_template_param = has_attr(
                    &item.attrs,
                    "bindgen_unused_template_param_in_arg_or_return",
                );
                let is_move_constructor = Self::is_move_constructor(&item);
                let (reference_args, return_type_is_reference) =
                    Self::get_reference_parameters_and_return(&item);
                let original_name = get_bindgen_original_name_annotation(&item.attrs);
                let doc_attr = get_doc_attr(&item.attrs);
                self.funcs_to_convert.push(FuncToConvert {
                    self_ty: None,
                    ident: item.sig.ident,
                    doc_attr,
                    inputs: item.sig.inputs,
                    output: item.sig.output,
                    vis: item.vis,
                    virtualness,
                    cpp_vis,
                    is_move_constructor,
                    unused_template_param,
                    return_type_is_reference,
                    reference_args,
                    original_name,
                    synthesized_this_type: None,
                    synthesize_make_unique: false,
                });
                Ok(())
            }
            ForeignItem::Static(item) => Err(ConvertErrorWithContext(
                ConvertError::StaticData(item.ident.to_string()),
                Some(ErrorContext::Item(item.ident)),
            )),
            _ => Err(ConvertErrorWithContext(
                ConvertError::UnexpectedForeignItem,
                None,
            )),
        }
    }

    fn get_bindgen_special_member_annotation(fun: &ForeignItemFn) -> Option<String> {
        fun.attrs
            .iter()
            .filter_map(|a| {
                if a.path.is_ident("bindgen_special_member") {
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

    fn is_move_constructor(fun: &ForeignItemFn) -> bool {
        Self::get_bindgen_special_member_annotation(fun).map_or(false, |val| val == "move_ctor")
    }

    fn get_reference_parameters_and_return(fun: &ForeignItemFn) -> (HashSet<Ident>, bool) {
        let mut ref_params = HashSet::new();
        let mut ref_return = false;
        for a in &fun.attrs {
            if a.path.is_ident("bindgen_ret_type_reference") {
                ref_return = true;
            } else if a.path.is_ident("bindgen_arg_type_reference") {
                let r: Result<Ident, syn::Error> = a.parse_args();
                if let Ok(ls) = r {
                    ref_params.insert(ls);
                }
            }
        }
        (ref_params, ref_return)
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
                    match get_called_function(&itm.block) {
                        Some(id) => id.clone(),
                        None => itm.sig.ident,
                    }
                };
                self.method_receivers.insert(
                    effective_fun_name,
                    QualifiedName::new(&self.ns, ty_id.clone()),
                );
            }
        }
    }

    /// Indicate that all foreign mods and all impl blocks have been
    /// fed into us, and we should process that information to generate
    /// the resulting APIs.
    pub(crate) fn finished(mut self, apis: &mut Vec<UnanalyzedApi>) {
        apis.append(&mut self.ignored_apis);
        while !self.funcs_to_convert.is_empty() {
            let mut fun = self.funcs_to_convert.remove(0);
            fun.self_ty = self.method_receivers.get(&fun.ident).cloned();
            apis.push(UnanalyzedApi::Function {
                name: ApiName::new_with_cpp_name(
                    &self.ns,
                    fun.ident.clone(),
                    fun.original_name.clone(),
                ),
                fun: Box::new(fun),
                analysis: (),
                name_for_gc: None,
            })
        }
    }
}

/// bindgen sometimes generates an impl fn called a which calls
/// a function called a1(), if it's dealing with conflicting names.
/// We actually care about the name a1, so we have to parse the
/// name of the actual function call inside the block's body.
fn get_called_function(block: &Block) -> Option<&Ident> {
    match block.stmts.first() {
        Some(Stmt::Expr(Expr::Call(ExprCall { func, .. }))) => match **func {
            Expr::Path(ref exp) => exp.path.segments.first().map(|ps| &ps.ident),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod test {
    use super::get_called_function;
    use syn::parse_quote;
    use syn::Block;

    #[test]
    fn test_get_called_function() {
        let b: Block = parse_quote! {
            {
                call_foo()
            }
        };
        assert_eq!(get_called_function(&b).unwrap().to_string(), "call_foo");
    }
}
