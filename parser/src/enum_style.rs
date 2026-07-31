use proc_macro2::{Ident, Span};
use quote::{quote, ToTokens};
use indexmap::set::IndexSet as HashSet;
use indexmap::map::IndexMap as HashMap;
use syn::parse::Parse;

#[derive(Debug, Hash, PartialEq, Eq)]
pub enum EnumStyle {
    BitfieldEnum,
    NewtypeEnum,
    // NewtypeGlobalEnum,
    RustifiedEnum,
    RustifiedNonExhaustiveEnum,
    // ConstifiedEnumModule,
    // ConstifiedEnum,
}

#[derive(Debug, Default)]
pub struct EnumStyleMap(pub HashMap<EnumStyle, Vec<String>>);

impl std::hash::Hash for EnumStyleMap {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for (k, v) in &self.0 {
            k.hash(state);
            v.hash(state);
        }
    }
}

impl EnumStyleMap {
    pub fn get_enum_names(&self) -> HashSet<&String> {
        self.0.values().flat_map(|v| v.iter()).collect()
    }
}

impl EnumStyle {
    fn from_str(s: &str) -> Option<Self> {
        use EnumStyle::*;
        Some(match s {
            "BitfieldEnum" => BitfieldEnum,
            "NewtypeEnum" => NewtypeEnum,
            // "NewtypeGlobalEnum" => NewtypeGlobalEnum,
            "RustifiedEnum" => RustifiedEnum,
            "RustifiedNonExhaustiveEnum" => RustifiedNonExhaustiveEnum,
            // "ConstifiedEnumModule" => ConstifiedEnumModule,
            // "ConstifiedEnum" => ConstifiedEnum,
            _ => return None,
        })
    }
}

impl Parse for EnumStyle {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let style_ident: Ident = input.parse()?;
        let style = style_ident.to_string();
        EnumStyle::from_str(&style).ok_or(syn::Error::new(
            style_ident.span(),
            format!("unknown enum style `{}`", style),
        ))
    }
}

impl ToTokens for EnumStyle {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        use EnumStyle::*;
        let variant_ident = match self {
            BitfieldEnum => "BitfieldEnum",
            NewtypeEnum => "NewtypeEnum",
            // NewtypeGlobalEnum => "NewtypeGlobalEnum",
            RustifiedEnum => "RustifiedEnum",
            RustifiedNonExhaustiveEnum => "RustifiedNonExhaustiveEnum",
            // ConstifiedEnumModule => "ConstifiedEnumModule",
            // ConstifiedEnum => "ConstifiedEnum",
        };
        let var = Ident::new(variant_ident, Span::call_site());
        tokens.extend(quote! { #var });
    }
}
