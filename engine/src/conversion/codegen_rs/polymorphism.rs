use crate::types::QualifiedName;
use itertools::Itertools;
use proc_macro2::Span;
use quote::quote;
use syn::ExprPath;
use syn::{
    parse_quote, punctuated::Punctuated, token::Mut, Expr, FnArg, ForeignItem, ForeignItemFn,
    GenericArgument, Ident, ImplItem, ImplItemFn, Item, ItemTrait, Pat, PatType, PathArguments,
    Receiver, ReturnType, Signature, Stmt, TraitItem, TraitItemFn, Type, TypePath,
};

use super::{RsCodegenResult, Use};


#[doc = "Converts the type of references to the impl Types for polymorphism"]
pub(crate) fn convert_refs(typ: Box<Type>) -> Box<Type> {
    match *typ {
        Type::Reference(re) => {
            let typ = *re.elem;
            Box::new(match re.mutability {
                Some(_) => parse_quote! {impl CppPinMutRef <'_,  #typ >},
                None => parse_quote! {impl CppConstRef <'_,  #typ >},
            })
        },
        _ => typ
    }
}

#[doc = "Goes trough the entire codegen and modifies it to support polymorphism for refences and methods."]
pub(crate) fn polymorphise(rs_codegen_results: &mut Vec<(QualifiedName, RsCodegenResult)>) {
    let mut impls_to_trait = RequiredPolyMethods::default();

    if let Some(first) = rs_codegen_results.first_mut() {
        first.1.bindgen_mod_items.push(parse_quote!(
            use autocxx::*;
        ));
    }

    for results in &mut *rs_codegen_results {
        polymorphise_methods(&mut impls_to_trait, results);
        polymorphise_functions(&mut impls_to_trait, results);
    }

    make_method_traits(rs_codegen_results, &mut impls_to_trait);
}

#[doc = "Builds the function signature for a trait method with or without body"]
fn make_trait_method(
    group: &(TypePath, QualifiedName, syn::Signature, syn::ExprPath),
    mutability: bool,
    first_namespace: &mut Option<QualifiedName>,
) -> (TraitItem, syn::ExprPath) {
    let mut sig = group.2.clone();
    *first_namespace = Some(group.1.clone());
    for i in &mut sig.inputs {
        match i {
            FnArg::Typed(t) => {
                conv_ref_args(t, false);
            }
            FnArg::Receiver(r) => {
                *r = if mutability {
                    parse_quote!(&mut self)
                } else {
                    parse_quote!(&self)
                };
            }
        }
    }

    (TraitItem::Fn(parse_quote! {#sig; }), group.3.clone())
}

#[doc = "Adds the trait definition for the methods to the global items of codegen"]
fn add_methods_trait(
    ident: &Ident,
    methods: &Vec<(syn::TraitItem, syn::ExprPath)>,
    cgr: &mut RsCodegenResult,
) {
    let mut ti: ItemTrait = parse_quote! { pub trait #ident <Child> {} };
    ti.items = methods.iter().map(|m| m.0.clone()).collect();

    cgr.global_items.push(Item::Trait(ti));
}

#[doc = "Adds the implementations of the methods trait to the global items of codegen"]
fn add_methods_impl(
    mutability: bool,
    ty: &TypePath,
    methods: &Vec<(syn::TraitItem, syn::ExprPath)>,
    ident: &Ident,
    cgr: &mut RsCodegenResult,
) {
    let impl_methods = build_impl_methods(&methods, mutability);

    let target_class = if mutability {
        quote!(std::pin::Pin<Child>)
    } else {
        quote!(Child)
    };
    let target_deref = if mutability {
        quote!(std::ops::DerefMut<Target = C>)
    } else {
        quote!(std::ops::Deref<Target = C>)
    };

    cgr.global_items.push(parse_quote! {
        impl<Child: #target_deref, C: crate::ToBaseClass< #ty >> #ident <C> for #target_class {
            #( #impl_methods )*
        }
    });
}

#[doc = "Generates the neccessary traits and impl blocks for polymorphic methods"]
fn make_method_traits(
    rs_codegen_results: &mut Vec<(QualifiedName, RsCodegenResult)>,
    impls_to_trait: &mut RequiredPolyMethods,
) {
    let mut added_use = false;

    for (impls, mutability) in [
        (&mut impls_to_trait.const_methods, false),
        (&mut impls_to_trait.mut_methods, true),
    ] {
        for (ty, group) in &impls.iter().group_by(|k| &k.0) {
            let mut first_namespace = None;
            let methods: Vec<_> = group
                .map(|group| make_trait_method(group, mutability, &mut first_namespace))
                .collect();

            let mut cgr = RsCodegenResult::default();

            let ident = Ident::new(
                &format!(
                    "Methods{}{}",
                    ty.path.segments.last().unwrap().ident,
                    if mutability { "Mut" } else { "Const" }
                ),
                Span::call_site(),
            );
            add_methods_trait(&ident, &methods, &mut cgr);
            add_methods_impl(mutability, ty, &methods, &ident, &mut cgr);

            if !added_use {
                cgr.global_items.push(parse_quote!(
                    use autocxx::*;
                ));
                added_use = true;
            }

            rs_codegen_results.push((first_namespace.unwrap(), cgr))
        }
    }
}

#[doc = "Goes trough the entire codegen and modifies it to support polymorphism for refences and methods."]
fn build_impl_method(
    mutability: bool,
    mut inputs: Punctuated<FnArg, syn::token::Comma>,
    ident: &Ident,
    output: &ReturnType,
    handler: &ExprPath,
) -> ImplItemFn {
    let params_call = params_from_fnargs(&inputs);

    if mutability {
        for p in &mut inputs {
            if let FnArg::Typed(pat) = p
                && let syn::Pat::Ident(id) = &mut *pat.pat
            {
                id.mutability = Some(Mut::default());
            }
        }
    }

    let body: Vec<Stmt> = if mutability {
        parse_quote!(
            let ptr = self.as_mut();
            let raw_ptr = unsafe { ptr.get_unchecked_mut() };
            #handler (raw_ptr.as_base_mut() #(, #params_call )* )
        )
    } else {
        parse_quote!(#handler (self.deref().as_base() #(, #params_call )* ))
    };

    parse_quote! {
        fn #ident <'a>(#inputs ) #output {
            #( #body )*
        }
    }
}

#[doc = "Adds a single method to the list of methods that require a polymorphic implementation in a trait."]
fn build_impl_methods(
    methods: &Vec<(syn::TraitItem, syn::ExprPath)>,
    mutability: bool,
) -> Vec<ImplItemFn> {
    methods
        .iter()
        .map(|m| {
            if let (TraitItem::Fn(TraitItemFn { sig, .. }), handler) = m {
                let Signature {
                    ident,
                    output,
                    inputs,
                    ..
                } = sig.clone();

                build_impl_method(mutability, inputs, &ident, &output, handler)
            } else {
                panic!()
            }
        })
        .collect()
}

#[doc = "Adds the methods to the list of methods that require a polymorphic implementation in a trait."]
fn polymorphise_methods(
    impls_to_trait: &mut RequiredPolyMethods,
    results: &mut (QualifiedName, RsCodegenResult),
) {
    let imp = results.1.impl_entry.take();
    if let Some(impll) = imp {
        if let ImplItem::Fn(f) = impll.item.clone()
            && let Some(FnArg::Receiver(r)) = f.sig.inputs.first()
        {
            let handler = if let Stmt::Expr(e, _) = f.block.stmts.get(0).unwrap()
                && let Expr::Call(c) = e
                && let Expr::Path(pat) = &*c.func
            {
                pat.clone()
            } else {
                panic!();
            };
            polymorphise_receiver_func(r, impls_to_trait, results.0.clone(), &f.sig, handler);
        } else {
            results.1.impl_entry = Some(impll); //put back
        }
    }
}

#[doc = "Given a method signature from a method adds it to the list of methods that require a polymorphic implementation in a trait.
        Returns true if one is required."]
fn polymorphise_receiver_func(
    r: &Receiver,
    impls_to_trait: &mut RequiredPolyMethods,
    qn: QualifiedName,
    sig: &Signature,
    fnname: syn::ExprPath,
) -> bool {
    match &*r.ty {
        Type::Reference(re) => {
            if let Type::Path(path) = &*re.elem {
                impls_to_trait
                    .const_methods
                    .push((path.clone(), qn, sig.clone(), fnname));
            }
            true
        }
        Type::Path(pat) => {
            if let Some(ty) = try_get_pin_type(pat) {
                impls_to_trait
                    .mut_methods
                    .push((ty.clone(), qn, sig.clone(), fnname));
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

#[doc = "Returns true if the function requires a wrapper for the polymorphic parameters. Additionally if the function is a method it will be added to the polymorhic method trat"]
fn analyze_function(
    f: &Signature,
    fnname: &Ident,
    impls_to_trait: &mut RequiredPolyMethods,
    qn: QualifiedName,
) -> bool {
    let func_name = f.ident.to_string();
    f.inputs
        .iter()
        .filter(|i| match i {
            FnArg::Receiver(r) => {
                let fnname: syn::ExprPath = parse_quote!( #fnname );
                polymorphise_receiver_func(r, impls_to_trait, qn.clone(), &f, fnname)
            }
            FnArg::Typed(t) => match &*t.ty {
                Type::Path(p) => try_get_pin_type(p).is_some(),
                Type::Reference(_) => true,
                _ => false,
            },
        })
        .count()
        > 0
        && !func_name.contains("autocxx_make_string")
        && !func_name.contains("synthetic_const_copy_ctor")
        && !func_name.starts_with("cast_")
}

#[derive(Default)]
struct RequiredPolyMethods {
    const_methods: Vec<(TypePath, QualifiedName, Signature, syn::ExprPath)>,
    mut_methods: Vec<(TypePath, QualifiedName, Signature, syn::ExprPath)>,
}

#[doc = "Converts the functions parameters into polymorphic impl parameters for reference parameters"]
fn polymorphise_functions(
    impls_to_trait: &mut RequiredPolyMethods,
    results: &mut (QualifiedName, RsCodegenResult),
) {
    for ie in &mut results.1.extern_c_mod_items {
        if let ForeignItem::Fn(ForeignItemFn { vis, sig, .. }) = ie {
            let fnname: Ident = Ident::new(
                &format!("{}_poly", sig.ident.to_string()),
                Span::call_site(),
            );

            if analyze_function(&sig, &fnname, impls_to_trait, results.0.clone()) {
                *vis = parse_quote!();
                let mut fnargs = sig.inputs.clone();
                for arg in &mut fnargs {
                    if let FnArg::Typed(t) = arg {
                        conv_ref_args(t, true)
                    }
                }
                let ret = &mut sig.output;
                let params = params_from_fnargs(&fnargs);
                let oldident = &sig.ident;

                results.1.bindgen_mod_items.push(Item::Fn(parse_quote!(
                    pub fn #fnname ( #fnargs ) #ret {
                        cxxbridge :: #oldident ( #( #params, )* )
                } )));
                if let Some(fi) = results.1.materializations.first()
                    && let Use::UsedFromCxxBridgeWithAlias(a) = fi
                {
                    results.1.materializations = vec![Use::Custom(Box::new(
                        parse_quote!(pub use bindgen::root:: #fnname as #a; ),
                    ))];
                }
            }
        }
    }
}

#[doc = "Returns the function parameters for a call getting the baseclass for polymorphic parameters"]
fn params_from_fnargs(fnargs: &Punctuated<syn::FnArg, syn::token::Comma>) -> Vec<Expr> {
    fnargs
        .clone()
        .iter()
        .filter_map(|pat| match pat {
            FnArg::Receiver(_) => None,
            FnArg::Typed(t) => {
                let argname = match &*t.pat {
                    syn::Pat::Ident(patty) => &patty.ident,
                    _ => todo!(),
                };
                Some(if let Type::ImplTrait(it) = &*t.ty {
                    if let Some(syn::TypeParamBound::Trait(t)) = it.bounds.first() {
                        if t.path.segments.last().unwrap().ident.to_string() == "CppConstRef" {
                            parse_quote!( #argname .as_cpp_ref() )
                        } else {
                            parse_quote!( #argname .as_cpp_mut() )
                        }
                    } else {
                        panic!()
                    }
                } else {
                    parse_quote!( #argname )
                })
            }
        })
        .collect()
}

#[doc = "Tries to get the type inside a pinned refenece if it is one"]
fn try_get_pin_type(p: &TypePath) -> Option<&TypePath> {
    if p.path.segments.last().unwrap().ident.to_string() == "Pin"
        && let PathArguments::AngleBracketed(a) = &p.path.segments.last().unwrap().arguments
        && let Some(ty) = a.args.last()
        && let GenericArgument::Type(ty) = ty
        && let Type::Reference(re) = ty
        && let Type::Path(result) = &*re.elem
    {
        Some(result)
    } else {
        None
    }
}

#[doc = "Given a function parameter turns it into polymorphic impl parameter if it is a reference"]
fn conv_ref_args(t: &mut PatType, mutability: bool) {
    if let Type::Path(p) = &mut *t.ty {
        if let Some(ty) = try_get_pin_type(&p) {
            *t.ty = parse_quote! { impl crate::CppPinMutRef<'_, #ty> };
            if mutability && let Pat::Ident(i) = &mut *t.pat {
                i.mutability = Some(Mut::default())
            }
        }
    }
    if let Type::Reference(r) = &mut *t.ty {
        let ty = &*r.elem;
        *t.ty = parse_quote! { impl crate::CppConstRef<'_, #ty> };
    }
}
