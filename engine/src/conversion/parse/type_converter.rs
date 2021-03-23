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

use crate::{
    conversion::codegen_cpp::AdditionalNeed,
    conversion::{
        api::{TypeApiDetails, UnanalyzedApi},
        codegen_cpp::type_to_cpp::type_to_cpp,
        ConvertError,
    },
    known_types::KNOWN_TYPES,
    types::{make_ident, Namespace, TypeName},
};
use std::collections::{HashMap, HashSet};
use syn::{
    parse_quote, punctuated::Punctuated, GenericArgument, PathArguments, PathSegment, Type,
    TypePath, TypePtr,
};

/// Results of some type conversion, annotated with a list of every type encountered,
/// and optionally any extra APIs we need in order to use this type.
pub(crate) struct Annotated<T> {
    pub(crate) ty: T,
    pub(crate) types_encountered: HashSet<TypeName>,
    pub(crate) extra_apis: Vec<UnanalyzedApi>,
    pub(crate) requires_unsafe: bool,
}

impl<T> Annotated<T> {
    fn new(
        ty: T,
        types_encountered: HashSet<TypeName>,
        extra_apis: Vec<UnanalyzedApi>,
        requires_unsafe: bool,
    ) -> Self {
        Self {
            ty,
            types_encountered,
            extra_apis,
            requires_unsafe,
        }
    }

    fn map<T2, F: FnOnce(T) -> T2>(self, fun: F) -> Annotated<T2> {
        Annotated {
            ty: fun(self.ty),
            types_encountered: self.types_encountered,
            extra_apis: self.extra_apis,
            requires_unsafe: self.requires_unsafe,
        }
    }
}

/// A type which can convert from a type encountered in `bindgen`
/// output to the sort of type we should represeent to `cxx`.
/// As a simple example, `std::string` should be replaced
/// with [CxxString]. This also involves keeping track
/// of typedefs, and any instantiated concrete types.
///
/// This object is a bit of a pest. The information here
/// is compiled during the parsing phase (which is why it lives
/// in the parse mod) but is used during various other phases.
/// As such it contributes to both the parsing and analysis phases.
/// It's possible that the information here largely duplicates
/// information stored elsewhere in the list of `Api`s, or can
/// easily be moved into it, which would enable us to
/// distribute this logic elsewhere.
pub(crate) struct TypeConverter {
    types_found: Vec<TypeName>,
    typedefs: HashMap<TypeName, Type>,
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

    pub(crate) fn insert_typedef(&mut self, id: TypeName, target: Type) {
        self.typedefs.insert(id, target);
    }

    pub(crate) fn convert_boxed_type(
        &mut self,
        ty: Box<Type>,
        ns: &Namespace,
        convert_ptrs_to_reference: bool,
    ) -> Result<Annotated<Box<Type>>, ConvertError> {
        Ok(self
            .convert_type(*ty, ns, convert_ptrs_to_reference)?
            .map(Box::new))
    }

    pub(crate) fn convert_type(
        &mut self,
        ty: Type,
        ns: &Namespace,
        convert_ptrs_to_reference: bool,
    ) -> Result<Annotated<Type>, ConvertError> {
        let result = match ty {
            Type::Path(p) => {
                let newp = self.convert_type_path(p, ns)?;
                if let Type::Path(newpp) = &newp.ty {
                    // Special handling because rust_Str (as emitted by bindgen)
                    // doesn't simply get renamed to a different type _identifier_.
                    // This plain type-by-value (as far as bindgen is concerned)
                    // is actually a &str.
                    if KNOWN_TYPES.should_dereference_in_cpp(newpp) {
                        Annotated::new(
                            Type::Reference(parse_quote! {
                                &str
                            }),
                            newp.types_encountered,
                            newp.extra_apis,
                            false,
                        )
                    } else {
                        newp
                    }
                } else {
                    newp
                }
            }
            Type::Reference(mut r) => {
                let innerty = self.convert_boxed_type(r.elem, ns, false)?;
                r.elem = innerty.ty;
                Annotated::new(
                    Type::Reference(r),
                    innerty.types_encountered,
                    innerty.extra_apis,
                    false,
                )
            }
            Type::Ptr(ptr) if convert_ptrs_to_reference => {
                self.convert_ptr_to_reference(ptr, ns)?
            }
            Type::Ptr(mut ptr) => {
                let innerty = self.convert_boxed_type(ptr.elem, ns, false)?;
                ptr.elem = innerty.ty;
                Annotated::new(
                    Type::Ptr(ptr),
                    innerty.types_encountered,
                    innerty.extra_apis,
                    true,
                )
            }
            _ => Annotated::new(ty, HashSet::new(), Vec::new(), false),
        };
        Ok(result)
    }

    fn convert_type_path(
        &mut self,
        mut typ: TypePath,
        ns: &Namespace,
    ) -> Result<Annotated<Type>, ConvertError> {
        // First, qualify any unqualified paths.
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

        let original_tn = TypeName::from_type_path(&typ);
        let mut deps = HashSet::new();

        // Now convert this type itself.
        deps.insert(original_tn.clone());
        // First let's see if this is a typedef.
        let (typ, tn) = match self.resolve_typedef(&original_tn) {
            None => (typ, original_tn),
            Some(Type::Path(resolved_tp)) => {
                let resolved_tn = TypeName::from_type_path(&resolved_tp);
                deps.insert(resolved_tn.clone());
                (resolved_tp.clone(), resolved_tn)
            }
            Some(other) => {
                return Ok(Annotated::new(
                    other.clone(),
                    HashSet::new(),
                    Vec::new(),
                    false,
                ))
            }
        };

        // Now let's see if it's a known type.
        let mut typ = match KNOWN_TYPES.known_type_substitute_path(&typ) {
            Some(mut substitute_type) => {
                if let Some(last_seg_args) =
                    typ.path.segments.into_iter().last().map(|ps| ps.arguments)
                {
                    let last_seg = substitute_type.path.segments.last_mut().unwrap();
                    last_seg.arguments = last_seg_args;
                }
                substitute_type
            }
            None => typ,
        };

        let mut extra_apis = Vec::new();

        // Finally let's see if it's generic.
        if Self::is_generic(&typ) {
            if KNOWN_TYPES.is_cxx_acceptable_generic(&tn) {
                // this is a type of generic understood by cxx (e.g. CxxVector)
                // so let's convert any generic type arguments. This recurses.
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
                                deps.extend(innerty.types_encountered.drain());
                                PathArguments::AngleBracketed(ab)
                            }
                            _ => s.arguments,
                        };
                        Ok(parse_quote!( #ident #args ))
                    })
                    .collect::<Result<_, _>>()?;
            } else {
                // Oh poop. It's a generic type which cxx won't be able to handle.
                // We'll have to come up with a concrete type in both the cxx::bridge (in Rust)
                // and a corresponding typedef in C++.
                let (new_tn, api) = self.get_templated_typename(&Type::Path(typ))?;
                extra_apis.extend(api.into_iter());
                deps.remove(&tn);
                typ = new_tn.to_type_path();
                deps.insert(new_tn);
            }
        };
        Ok(Annotated::new(Type::Path(typ), deps, extra_apis, false))
    }

    fn is_generic(typ: &TypePath) -> bool {
        match typ.path.segments.last() {
            None => false,
            Some(s) => !s.arguments.is_empty(),
        }
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
                    let mut innerty = self.convert_type(t, ns, false)?;
                    types_encountered.extend(innerty.types_encountered.drain());
                    extra_apis.extend(innerty.extra_apis.drain(..));
                    GenericArgument::Type(innerty.ty)
                }
                _ => arg,
            });
        }
        Ok(Annotated::new(
            new_pun,
            types_encountered,
            extra_apis,
            false,
        ))
    }

    fn resolve_typedef<'b>(&'b self, tn: &TypeName) -> Option<&'b Type> {
        self.typedefs.get(&tn).map(|resolution| match resolution {
            Type::Path(typ) => {
                let tn = TypeName::from_type_path(typ);
                self.resolve_typedef(&tn).unwrap_or(resolution)
            }
            _ => resolution,
        })
    }

    fn convert_ptr_to_reference(
        &mut self,
        ptr: TypePtr,
        ns: &Namespace,
    ) -> Result<Annotated<Type>, ConvertError> {
        let mutability = ptr.mutability;
        let elem = self.convert_boxed_type(ptr.elem, ns, false)?;
        // TODO - in the future, we should check if this is a rust::Str and throw
        // a wobbler if not. rust::Str should only be seen _by value_ in C++
        // headers; it manifests as &str in Rust but on the C++ side it must
        // be a plain value. We should detect and abort.
        Ok(elem.map(|elem| match mutability {
            Some(_) => Type::Path(parse_quote! {
                std::pin::Pin < & #mutability #elem >
            }),
            None => Type::Reference(parse_quote! {
                & #elem
            }),
        }))
    }

    fn add_concrete_type(&self, tyname: &TypeName, rs_definition: &Type) -> UnanalyzedApi {
        let final_ident = make_ident(tyname.get_final_ident());
        let mut fulltypath: Vec<_> = ["bindgen", "root"].iter().map(make_ident).collect();
        fulltypath.push(final_ident.clone());
        let tynamestring = tyname.to_cpp_name();
        UnanalyzedApi {
            ns: tyname.get_namespace().clone(),
            id: final_ident.clone(),
            deps: HashSet::new(),
            detail: crate::conversion::api::ApiDetail::ConcreteType {
                ty_details: TypeApiDetails {
                    fulltypath,
                    final_ident,
                    tynamestring,
                },
                additional_cpp: AdditionalNeed::ConcreteTemplatedTypeTypedef(
                    tyname.clone(),
                    Box::new(rs_definition.clone()),
                ),
            },
        }
    }

    fn get_templated_typename(
        &mut self,
        rs_definition: &Type,
    ) -> Result<(TypeName, Option<UnanalyzedApi>), ConvertError> {
        let count = self.concrete_templates.len();
        // We just use this as a hash key, essentially.
        let cpp_definition = type_to_cpp(rs_definition)?;
        let e = self.concrete_templates.get(&cpp_definition);
        match e {
            Some(tn) => Ok((tn.clone(), None)),
            None => {
                let tn = TypeName::new(&Namespace::new(), &format!("AutocxxConcrete{}", count));
                self.concrete_templates
                    .insert(cpp_definition.clone(), tn.clone());
                let api = self.add_concrete_type(&tn, rs_definition);
                Ok((tn, Some(api)))
            }
        }
    }
}
