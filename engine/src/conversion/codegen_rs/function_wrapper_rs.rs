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

use proc_macro2::TokenStream;
use syn::{Pat, Type};

use crate::conversion::analysis::fun::function_wrapper::{
    RustConversionType, TypeConversionPolicy,
};
use quote::quote;
use syn::parse_quote;

impl TypeConversionPolicy {
    pub(super) fn rust_wrapper_unconverted_type(&self) -> Type {
        match self.rust_conversion {
            RustConversionType::None => self.converted_rust_type(),
            RustConversionType::FromStr => parse_quote! { impl ToCppString },
        }
    }

    pub(super) fn rust_conversion(&self, var: Pat) -> TokenStream {
        match self.rust_conversion {
            RustConversionType::None => quote! { #var },
            RustConversionType::FromStr => quote! ( #var .into_cpp() ),
        }
    }
}
