// Copyright 2023 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Newtype wrappers for `syn` types implementing a different
//! `Debug` implementation that results in more concise output.

use proc_macro2::TokenStream;
use quote::ToTokens;

pub use syn::{parse, parse_quote, punctuated, token, Ident};

macro_rules! minisyn {
    ($syntype:ident) => {
        /// Newtype wrapper for `syn::$syntype` which has a more concise
        /// `Debug` representation.
        #[derive(Clone)]
        pub (crate) struct $syntype(pub(crate) ::syn::$syntype);
        impl std::fmt::Debug for $syntype {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
                write!(f, "{}", self.0.to_token_stream().to_string())
            }
        }
        impl ToTokens for $syntype {
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
        impl syn::parse::Parse for $syntype {
            fn parse(input: syn::parse::ParseStream<'_>) -> syn::parse::Result<Self> {
                syn::parse::Parse::parse(input).map(Self)
            }
        }
        impl std::ops::Deref for $syntype {
            type Target = ::syn::$syntype;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
    };
}

minisyn!(ItemMod);
minisyn!(Binding);
minisyn!(Expr);
minisyn!(ExprAssign);
minisyn!(ExprAssignOp);
minisyn!(ExprAwait);
minisyn!(ExprBinary);
minisyn!(ExprBox);
minisyn!(ExprBreak);
minisyn!(ExprCast);
minisyn!(ExprField);
// minisyn!(ExprGroup);
minisyn!(ExprLet);
minisyn!(ExprParen);
minisyn!(ExprReference);
minisyn!(ExprTry);
minisyn!(ExprType);
minisyn!(ExprUnary);
minisyn!(FnArg);
minisyn!(ImplItem);
minisyn!(Item);
minisyn!(ItemConst);
minisyn!(ItemEnum);
minisyn!(ItemStruct);
minisyn!(ItemType);
minisyn!(ItemUse);
minisyn!(Macro);
minisyn!(Pat);
// minisyn!(PatBox);
// minisyn!(PatType);
// minisyn!(PatReference);
// minisyn!(PatSlice);
// minisyn!(PatTuple);
minisyn!(Path);
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

#[derive(Clone)]
pub(crate) struct Punctuated<T, P>(pub(crate) syn::punctuated::Punctuated<T, P>);
#[derive(Clone)]
pub(crate) struct Attribute(pub(crate) syn::Attribute);