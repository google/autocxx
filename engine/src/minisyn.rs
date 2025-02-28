// Copyright 2023 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Newtype wrappers for `syn` types implementing a different
//! `Debug` implementation that results in more concise output.

use std::fmt::Display;

use proc_macro2::TokenStream;
use quote::ToTokens;
use syn::punctuated::{Pair, Punctuated};

macro_rules! minisyn_no_parse {
    ($syntype:ident) => {
        /// Equivalent to the identically-named `syn` type except
        /// that its `Debug` implementation is more concise.
        #[derive(Clone, Hash, Eq, PartialEq)]
        pub struct $syntype(pub ::syn::$syntype);
        impl std::fmt::Debug for $syntype {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
                write!(f, "{}", self.0.to_token_stream().to_string())
            }
        }
        impl ToTokens for $syntype
        where
            ::syn::$syntype: ToTokens,
        {
            fn to_tokens(&self, tokens: &mut TokenStream) {
                self.0.to_tokens(tokens)
            }

            fn to_token_stream(&self) -> TokenStream {
                self.0.to_token_stream()
            }
            fn into_token_stream(self) -> TokenStream
            where
                Self: Sized,
            {
                self.0.into_token_stream()
            }
        }
        impl std::ops::Deref for $syntype {
            type Target = ::syn::$syntype;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
        impl std::ops::DerefMut for $syntype {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }
        impl std::convert::From<::syn::$syntype> for $syntype {
            fn from(inner: ::syn::$syntype) -> Self {
                Self(inner)
            }
        }
        impl std::convert::From<$syntype> for syn::$syntype {
            fn from(inner: $syntype) -> Self {
                inner.0
            }
        }
    };
}

macro_rules! minisyn {
    ($syntype:ident) => {
        minisyn_no_parse!($syntype);

        impl syn::parse::Parse for $syntype {
            fn parse(input: syn::parse::ParseStream<'_>) -> syn::parse::Result<Self> {
                syn::parse::Parse::parse(input).map(Self)
            }
        }
    };
}

minisyn!(ItemMod);
minisyn_no_parse!(Attribute);
minisyn_no_parse!(AssocConst);
minisyn_no_parse!(AssocType);
minisyn!(Expr);
minisyn!(ExprAssign);
minisyn!(ExprAwait);
minisyn!(ExprBinary);
minisyn!(ExprBlock);
minisyn!(ExprBreak);
minisyn!(ExprConst);
minisyn!(ExprCast);
minisyn!(ExprField);
minisyn_no_parse!(ExprGroup);
minisyn!(ExprLet);
minisyn!(ExprParen);
minisyn!(ExprReference);
minisyn!(ExprTry);
minisyn!(ExprUnary);
minisyn_no_parse!(Field);
minisyn_no_parse!(Fields);
minisyn!(ForeignItem);
minisyn!(FnArg);
minisyn!(GenericArgument);
minisyn!(GenericParam);
minisyn!(Ident);
minisyn!(ImplItem);
minisyn!(Item);
minisyn!(ItemConst);
minisyn!(ItemEnum);
minisyn!(ItemForeignMod);
minisyn!(ItemStruct);
minisyn!(ItemType);
minisyn!(ItemUse);
minisyn!(LitBool);
minisyn!(LitInt);
minisyn!(Macro);
minisyn_no_parse!(Pat);
minisyn_no_parse!(PatType);
minisyn_no_parse!(PatReference);
minisyn_no_parse!(PatSlice);
minisyn_no_parse!(PatTuple);
minisyn!(Path);
minisyn_no_parse!(PathArguments);
minisyn!(PathSegment);
minisyn!(Receiver);
minisyn!(ReturnType);
minisyn!(Signature);
minisyn!(Stmt);
minisyn!(TraitItem);
minisyn!(Type);
minisyn!(TypeArray);
minisyn!(TypeGroup);
minisyn!(TypeParamBound);
minisyn!(TypeParen);
minisyn!(TypePath);
minisyn!(TypePtr);
minisyn!(TypeReference);
minisyn!(TypeSlice);
minisyn!(Visibility);

/// Converts a `syn::Punctuated` from being full of `syn` types to being
/// full of `minisyn` types or vice-versa.
pub(crate) fn minisynize_punctuated<T1, T2, S>(input: Punctuated<T1, S>) -> Punctuated<T2, S>
where
    T1: Into<T2>,
{
    input
        .into_pairs()
        .map(|p| match p {
            Pair::Punctuated(t, p) => Pair::Punctuated(t.into(), p),
            Pair::End(t) => Pair::End(t.into()),
        })
        .collect()
}

/// Converts a `Vec` from being full of `syn` types to being
/// full of `minisyn` types or vice-versa.
pub(crate) fn minisynize_vec<T1, T2>(input: Vec<T1>) -> Vec<T2>
where
    T1: Into<T2>,
{
    input.into_iter().map(Into::into).collect()
}

impl Ident {
    pub(crate) fn new(string: &str, span: proc_macro2::Span) -> Self {
        Self(syn::Ident::new(string, span))
    }
}

impl Display for Ident {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.to_string())
    }
}

impl<T> PartialEq<T> for Ident
where
    T: AsRef<str> + ?Sized,
{
    fn eq(&self, rhs: &T) -> bool {
        self.0.eq(rhs)
    }
}
