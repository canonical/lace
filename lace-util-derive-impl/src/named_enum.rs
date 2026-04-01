// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.

//! Implementation of the `NamedEnum` derive macro.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

/// Derives name accessor methods for an enum.
///
/// Generates `short_name(&self) -> &'static str`, `long_name(&self) ->
/// &'static str`, and `try_from_short_name(&str) -> Option<Self>`. Names are
/// sourced from `#[name(short = "...", long = "...")]` attributes; the variant
/// identifier is used as a fallback for either or both. All variants must be
/// unit variants.
pub fn derive(input: TokenStream) -> TokenStream {
    let input: DeriveInput = syn::parse2(input).expect("failed to parse derive input");
    let name = &input.ident;

    // Parse enum variants
    let variants = match &input.data {
        Data::Enum(data) => &data.variants,
        _ => panic!("NamedEnum can only be derived for enums"),
    };

    let mut variant_data = Vec::new();

    for variant in variants {
        if !matches!(variant.fields, Fields::Unit) {
            panic!("NamedEnum only supports unit variants");
        }

        let variant_name = &variant.ident;
        let mut short_name = None;
        let mut long_name = None;

        // Parse #[name(short = "...", long = "...")] attribute
        for attr in &variant.attrs {
            if attr.path().is_ident("name") {
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("short") {
                        let value = meta.value()?;
                        let s: syn::LitStr = value.parse()?;
                        short_name = Some(s.value());
                    } else if meta.path.is_ident("long") {
                        let value = meta.value()?;
                        let s: syn::LitStr = value.parse()?;
                        long_name = Some(s.value());
                    }
                    Ok(())
                })
                .expect(
                    "Failed to parse name attribute: expected format \
                     #[name(short = \"...\", long = \"...\")]",
                );
            }
        }

        let short = short_name.unwrap_or(variant_name.to_string());
        let long = long_name.unwrap_or(variant_name.to_string());

        variant_data.push((variant_name, short, long));
    }

    // Generate short_name method arms
    let short_name_arms = variant_data.iter().map(|(variant_name, short, _)| {
        quote! {
            #name::#variant_name => #short,
        }
    });

    // Generate long_name method arms
    let long_name_arms = variant_data.iter().map(|(variant_name, _, long)| {
        quote! {
            #name::#variant_name => #long,
        }
    });

    // Generate try_from_short_name method arms
    let try_from_short_name_arms = variant_data.iter().map(|(variant_name, short, _)| {
        quote! {
            #short => Some(#name::#variant_name),
        }
    });

    // Generate a single impl block with all three methods
    quote! {
        impl #name {
            pub fn short_name(&self) -> &'static str {
                match self {
                    #(#short_name_arms)*
                }
            }

            pub fn long_name(&self) -> &'static str {
                match self {
                    #(#long_name_arms)*
                }
            }

            pub fn try_from_short_name(name: &str) -> Option<Self> {
                match name {
                    #(#try_from_short_name_arms)*
                    _ => None,
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use quote::quote;

    #[test]
    fn test_derive_named_enum_with_name_attrs() {
        let input = quote! {
            enum Priority {
                #[name(short = "lo", long = "Low Priority")]
                Low,
                #[name(short = "hi", long = "High Priority")]
                High,
            }
        };
        let output = derive(input).to_string();
        assert!(output.contains("\"lo\""), "expected short name for Low");
        assert!(
            output.contains("\"Low Priority\""),
            "expected long name for Low"
        );
        assert!(output.contains("\"hi\""), "expected short name for High");
        assert!(
            output.contains("\"High Priority\""),
            "expected long name for High"
        );
    }

    #[test]
    fn test_derive_named_enum_defaults_to_identifier() {
        let input = quote! {
            enum Animal {
                Cat,
                Dog,
            }
        };
        let output = derive(input).to_string();
        // Without #[name] attrs, variant identifier strings are used as both
        // short and long names.
        assert!(
            output.contains("\"Cat\""),
            "expected identifier Cat as name"
        );
        assert!(
            output.contains("\"Dog\""),
            "expected identifier Dog as name"
        );
    }

    #[test]
    fn test_derive_named_enum_generates_all_methods() {
        let input = quote! {
            enum Foo {
                Bar,
            }
        };
        let output = derive(input).to_string();
        assert!(output.contains("short_name"), "expected short_name method");
        assert!(output.contains("long_name"), "expected long_name method");
        assert!(
            output.contains("try_from_short_name"),
            "expected try_from_short_name method"
        );
    }

    #[test]
    fn test_derive_named_enum_partial_name_attr() {
        // Only short= provided; long= should fall back to the variant identifier.
        let input = quote! {
            enum Foo {
                #[name(short = "b")]
                Bar,
            }
        };
        let output = derive(input).to_string();
        assert!(output.contains("\"b\""), "expected custom short name");
        assert!(
            output.contains("\"Bar\""),
            "expected identifier as long name fallback"
        );
    }

    #[test]
    #[should_panic(expected = "NamedEnum only supports unit variants")]
    fn test_derive_named_enum_non_unit_variant_panics() {
        let input = quote! {
            enum Foo {
                Bar(u32),
            }
        };
        derive(input);
    }
}
