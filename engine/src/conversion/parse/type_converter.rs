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

use crate::typedef_analyzer::{analyze_typedef_target, TypedefTarget};
use crate::{
    additional_cpp_generator::AdditionalNeed,
    conversion::{
        api::{Api, Use},
        ConvertError,
    },
    known_types::KNOWN_TYPES,
    type_to_cpp::type_to_cpp,
    types::{make_ident, Namespace, TypeName},
};
use quote::quote;
use std::collections::{HashMap, HashSet};
use syn::{
    parse_quote, punctuated::Punctuated, ForeignItem, GenericArgument, Item, PathArguments,
    PathSegment, Type, TypePath, TypePtr,
};

use super::non_pod_struct::new_non_pod_struct;

/// Results of some type conversion, annotated with a list of every type encountered,
/// and optionally any extra API we need in order to use this type.
pub(crate) struct Annotated<T> {
    pub(crate) ty: T,
    pub(crate) types_encountered: HashSet<TypeName>,
    pub(crate) extra_apis: Vec<Api>,
}

impl<T> Annotated<T> {
    fn new(ty: T, types_encountered: HashSet<TypeName>, extra_apis: Vec<Api>) -> Self {
        Self {
            ty,
            types_encountered,
            extra_apis,
        }
    }

    fn map<T2, F: FnOnce(T) -> T2>(self, fun: F) -> Annotated<T2> {
        Annotated {
            ty: fun(self.ty),
            types_encountered: self.types_encountered,
            extra_apis: self.extra_apis,
        }
    }
}

pub(crate) struct TypeConverter {
    types_found: Vec<TypeName>,
    typedefs: HashMap<TypeName, TypedefTarget>,
    concrete_templates: HashMap<String, TypeName>,
}

impl TypeConverter {
    pub(crate) fn new() -> Self {
        Self {
            types_found: Vec::new(),
            typedefs: HashMap::new(),
            concrete_templates: HashMap::new(),
        }
    }

    pub(crate) fn push(&mut self, ty: TypeName) {
        self.types_found.push(ty);
    }

    pub(crate) fn insert_typedef(&mut self, id: TypeName, ty: &Type) {
        let target = analyze_typedef_target(ty);
        self.typedefs.insert(id, target);
    }

    pub(crate) fn convert_boxed_type(
        &mut self,
        ty: Box<Type>,
        ns: &Namespace,
    ) -> Result<Annotated<Box<Type>>, ConvertError> {
        Ok(self.convert_type(*ty, ns)?.map(Box::new))
    }

    pub(crate) fn convert_type(
        &mut self,
        ty: Type,
        ns: &Namespace,
    ) -> Result<Annotated<Type>, ConvertError> {
        let result = match ty {
            Type::Path(p) => {
                let newp = self.convert_type_path(p, ns)?;
                // Special handling because rust_Str (as emitted by bindgen)
                // doesn't simply get renamed to a different type _identifier_.
                // This plain type-by-value (as far as bindgen is concerned)
                // is actually a &str.
                if KNOWN_TYPES.should_dereference_in_cpp(&newp.ty) {
                    Annotated::new(
                        Type::Reference(parse_quote! {
                            &str
                        }),
                        newp.types_encountered,
                        newp.extra_apis,
                    )
                } else {
                    newp.map(Type::Path)
                }
            }
            Type::Reference(mut r) => {
                let innerty = self.convert_boxed_type(r.elem, ns)?;
                r.elem = innerty.ty;
                Annotated::new(
                    Type::Reference(r),
                    innerty.types_encountered,
                    innerty.extra_apis,
                )
            }
            Type::Ptr(ptr) => self.convert_ptr_to_reference(ptr, ns)?,
            _ => Annotated::new(ty, HashSet::new(), Vec::new()),
        };
        Ok(result)
    }

    fn convert_type_path(
        &mut self,
        mut typ: TypePath,
        ns: &Namespace,
    ) -> Result<Annotated<TypePath>, ConvertError> {
        let mut types_encountered = HashSet::new();
        if typ.path.segments.iter().next().unwrap().ident != "root" {
            let ty = TypeName::from_type_path(&typ);
            // If the type looks like it is unqualified, check we know it
            // already, and if not, qualify it according to the current
            // namespace. This is a bit of a shortcut compared to having a full
            // resolution pass which can search all known namespaces.
            if !KNOWN_TYPES.is_known_type(&ty) {
                let num_segments = typ.path.segments.len();
                if num_segments > 1 {
                    return Err(ConvertError::UnsupportedBuiltInType(ty));
                }
                if !self.types_found.contains(&ty) {
                    typ.path.segments = std::iter::once(&"root".to_string())
                        .chain(ns.iter())
                        .map(|s| {
                            let i = make_ident(s);
                            parse_quote! { #i }
                        })
                        .chain(typ.path.segments.into_iter())
                        .collect();
                }
            }
        }

        typ.path.segments = typ
            .path
            .segments
            .into_iter()
            .map(|s| -> Result<PathSegment, ConvertError> {
                let ident = &s.ident;
                let args = match s.arguments {
                    PathArguments::AngleBracketed(mut ab) => {
                        let mut innerty = self.convert_punctuated(ab.args, ns)?;
                        ab.args = innerty.ty;
                        types_encountered.extend(innerty.types_encountered.drain());
                        PathArguments::AngleBracketed(ab)
                    }
                    _ => s.arguments,
                };
                Ok(parse_quote!( #ident #args ))
            })
            .collect::<Result<_, _>>()?;

        let mut last_seg_args = None;
        let mut seg_iter = typ.path.segments.iter().peekable();
        while let Some(seg) = seg_iter.next() {
            if !seg.arguments.is_empty() {
                if seg_iter.peek().is_some() {
                    panic!("Did not expect bindgen to create a type with path arguments on a non-final segment")
                } else {
                    last_seg_args = Some(seg.arguments.clone());
                }
            }
        }
        drop(seg_iter);
        let tn = TypeName::from_type_path(&typ);
        types_encountered.insert(tn.clone());
        // Let's see if this is a typedef.
        let typ = match self.resolve_typedef(&tn)? {
            None => typ,
            Some(resolved_tn) => {
                types_encountered.insert(resolved_tn.clone());
                resolved_tn.to_type_path()
            }
        };

        // This will strip off any path arguments...
        let mut typ = KNOWN_TYPES.known_type_substitute_path(&typ).unwrap_or(typ);
        let mut extra_apis = Vec::new();
        // but then we'll put them back again as necessary.
        if let Some(last_seg_args) = last_seg_args {
            let last_seg = typ.path.segments.last_mut().unwrap();
            last_seg.arguments = last_seg_args;
            // Is it one of the things built into cxx?
            if !KNOWN_TYPES.is_cxx_acceptable_generic(&tn) {
                // Oh poop. It's a generic type which cxx won't be able to handle.
                // We'll have to come up with a concrete type in both the cxx::bridge (in Rust)
                // and a corresponding typedef in C++.
                let (new_tn, api) = self.get_templated_typename(&Type::Path(typ));
                typ = new_tn.to_type_path();
                extra_apis.extend(api.into_iter());
                types_encountered.remove(&tn);
                types_encountered.insert(new_tn);
            }
        }
        Ok(Annotated::new(typ, types_encountered, extra_apis))
    }

    fn convert_punctuated<P>(
        &mut self,
        pun: Punctuated<GenericArgument, P>,
        ns: &Namespace,
    ) -> Result<Annotated<Punctuated<GenericArgument, P>>, ConvertError>
    where
        P: Default,
    {
        let mut new_pun = Punctuated::new();
        let mut types_encountered = HashSet::new();
        let mut extra_apis = Vec::new();
        for arg in pun.into_iter() {
            new_pun.push(match arg {
                GenericArgument::Type(t) => {
                    let mut innerty = self.convert_type(t, ns)?;
                    types_encountered.extend(innerty.types_encountered.drain());
                    extra_apis.extend(innerty.extra_apis.drain(..));
                    GenericArgument::Type(innerty.ty)
                }
                _ => arg,
            });
        }
        Ok(Annotated::new(new_pun, types_encountered, extra_apis))
    }

    fn resolve_typedef<'b>(
        &'b self,
        tn: &'b TypeName,
    ) -> Result<Option<&'b TypeName>, ConvertError> {
        match self.typedefs.get(&tn) {
            None => Ok(None),
            Some(TypedefTarget::NoArguments(original_tn)) => {
                match self.resolve_typedef(original_tn)? {
                    None => Ok(Some(original_tn)),
                    Some(further_resolution) => Ok(Some(further_resolution)),
                }
            }
            _ => Err(ConvertError::ComplexTypedefTarget(tn.to_cpp_name())),
        }
    }

    fn convert_ptr_to_reference(
        &mut self,
        ptr: TypePtr,
        ns: &Namespace,
    ) -> Result<Annotated<Type>, ConvertError> {
        let mutability = ptr.mutability;
        let elem = self.convert_boxed_type(ptr.elem, ns)?;
        // TODO - in the future, we should check if this is a rust::Str and throw
        // a wobbler if not. rust::Str should only be seen _by value_ in C++
        // headers; it manifests as &str in Rust but on the C++ side it must
        // be a plain value. We should detect and abort.
        Ok(elem.map(|elem| match mutability {
            Some(_) => Type::Path(parse_quote! {
                Pin < & #mutability #elem >
            }),
            None => Type::Reference(parse_quote! {
                & #elem
            }),
        }))
    }

    fn add_concrete_type(&self, tyname: &TypeName, rs_definition: &Type) -> Api {
        let final_ident = make_ident(tyname.get_final_ident());
        let bridge_item = Some(Item::Impl(parse_quote! {
            impl UniquePtr<#final_ident> {}
        }));
        let mut fulltypath: Vec<_> = ["bindgen", "root"].iter().map(|x| make_ident(x)).collect();
        fulltypath.push(final_ident.clone());
        let tynamestring = tyname.to_cpp_name();
        Api {
            ns: tyname.get_namespace().clone(),
            id: final_ident.clone(),
            use_stmt: Use::Unused,
            global_items: vec![Item::Impl(parse_quote! {
                unsafe impl cxx::ExternType for #(#fulltypath)::* {
                    type Id = cxx::type_id!(#tynamestring);
                    type Kind = cxx::kind::Opaque;
                }
            })],
            bridge_item,
            extern_c_mod_item: Some(ForeignItem::Verbatim(quote! {
                type #final_ident = super::bindgen::root::#final_ident;
            })),
            additional_cpp: Some(AdditionalNeed::ConcreteTemplatedTypeTypedef(
                tyname.clone(),
                Box::new(rs_definition.clone()),
            )),
            deps: HashSet::new(),
            id_for_allowlist: None,
            bindgen_mod_item: Some(Item::Struct(new_non_pod_struct(final_ident))),
            impl_entry: None,
        }
    }

    fn get_templated_typename(&mut self, rs_definition: &Type) -> (TypeName, Option<Api>) {
        let count = self.concrete_templates.len();
        // We just use this as a hash key, essentially.
        let cpp_definition = type_to_cpp(rs_definition);
        let e = self.concrete_templates.get(&cpp_definition);
        match e {
            Some(tn) => (tn.clone(), None),
            None => {
                let tn = TypeName::new(&Namespace::new(), &format!("AutocxxConcrete{}", count));
                self.concrete_templates
                    .insert(cpp_definition.clone(), tn.clone());
                let api = self.add_concrete_type(&tn, rs_definition);
                (tn, Some(api))
            }
        }
    }
}
