// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.

//! Implementation of the `NumEnum` derive macro.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Data, DeriveInput, Fields};

/// Derives `TryFrom<Int>` (for most integer widths) and `From<Enum>` for an
/// enum annotated with `#[repr(integer_type)]`.
///
/// All variants must be unit variants with explicit discriminants. Returns a
/// `compile_error!` token stream (rather than panicking) when the requirements
/// are not met.
pub fn derive(input: TokenStream) -> TokenStream {
    match try_derive(input) {
        Ok(ts) => ts,
        Err(e) => e.to_compile_error(),
    }
}

fn try_derive(input: TokenStream) -> syn::Result<TokenStream> {
    let input: DeriveInput = syn::parse2(input)?;
    let name = &input.ident;

    // Parse enum variants
    let variants = match &input.data {
        Data::Enum(data) => &data.variants,
        _ => {
            return Err(syn::Error::new_spanned(
                name,
                "NumEnum can only be derived for enums",
            ));
        }
    };

    // Find the underlying integer type from #[repr(integer_type)]
    let repr_type = input
        .attrs
        .iter()
        .find(|attr| attr.path().is_ident("repr"))
        .map(|attr| attr.parse_args::<syn::Ident>())
        .transpose()?
        .ok_or_else(|| {
            syn::Error::new_spanned(name, "NumEnum requires a #[repr(integer_type)] attribute")
        })?;

    // Validate all variants are unit variants
    for variant in variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                &variant.ident,
                "NumEnum only supports unit variants",
            ));
        }
    }

    // Collect variant names and discriminants
    let variant_discriminants: Vec<_> = variants
        .iter()
        .map(|variant| {
            let variant_name = &variant.ident;
            variant
                .discriminant
                .as_ref()
                .map(|(_, expr)| (variant_name, expr))
                .ok_or_else(|| {
                    syn::Error::new_spanned(
                        variant_name,
                        format!(
                            "variant `{variant_name}` must have an explicit \
                             discriminant for NumEnum"
                        ),
                    )
                })
        })
        .collect::<syn::Result<_>>()?;

    // Generate match arms for TryFrom<Int>, e.g. `0 => Ok(Color::Red),`
    let try_from_arms = variant_discriminants
        .iter()
        .map(|(variant_name, discriminant)| {
            quote! {
                #discriminant => Ok(#name::#variant_name),
            }
        });

    // Generate impl TryFrom<repr_type> for Enum with direct match
    let try_from_repr_impl = quote! {
        impl TryFrom<#repr_type> for #name {
            type Error = ();
            fn try_from(value: #repr_type) -> Result<Self, Self::Error> {
                match value {
                    #(#try_from_arms)*
                    _ => Err(()),
                }
            }
        }
    };

    // Generate TryFrom for other integer types that delegate to the repr impl
    let repr_str = repr_type.to_string();
    let other_int_types: Vec<syn::Ident> = [
        "u8", "u16", "u32", "u64", "usize", "i8", "i16", "i32", "i64", "isize",
    ]
    .iter()
    .filter(|t| **t != repr_str)
    .map(|t| syn::Ident::new(t, Span::call_site()))
    .collect();

    let other_try_from_impls = other_int_types.iter().map(|int_type| {
        quote! {
            impl TryFrom<#int_type> for #name {
                type Error = ();

                fn try_from(value: #int_type) -> Result<Self, Self::Error> {
                    let repr_val = #repr_type::try_from(value).map_err(|_| ())?;
                    Self::try_from(repr_val)
                }
            }
        }
    });

    // Generate From<Enum> for Int implementation
    let from_arms = variant_discriminants
        .iter()
        .map(|(variant_name, discriminant)| {
            quote! {
                #name::#variant_name => #discriminant,
            }
        });

    // Generate impl From<Color> for u8 { fn from(...) { match ... } }
    let from_impl = quote! {
        impl From<#name> for #repr_type {
            fn from(value: #name) -> Self {
                match value {
                    #(#from_arms)*
                }
            }
        }
    };

    // Combine implementations
    Ok(quote! {
        #try_from_repr_impl
        #(#other_try_from_impls)*
        #from_impl
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use quote::quote;

    #[test]
    fn test_derive_num_enum_generates_try_from_and_from() {
        let input = quote! {
            #[repr(u8)]
            enum Color {
                Red = 0,
                Green = 1,
                Blue = 2,
            }
        };
        let output = derive(input).to_string();
        assert!(output.contains("TryFrom"), "expected TryFrom in output");
        assert!(output.contains("From"), "expected From in output");
        assert!(
            output.contains("Red"),
            "expected variant name Red in output"
        );
        assert!(
            output.contains("Green"),
            "expected variant name Green in output"
        );
    }

    #[test]
    fn test_derive_num_enum_generates_all_integer_type_impls() {
        let input = quote! {
            #[repr(u8)]
            enum Flag {
                A = 1,
            }
        };
        let output = derive(input).to_string();
        // 1 repr (u8) + 9 other integer types = 10 TryFrom impls total
        assert_eq!(
            output.matches("impl TryFrom").count(),
            10,
            "expected 10 TryFrom impls (one per integer type)"
        );
    }

    #[test]
    fn test_derive_num_enum_repr_type_not_duplicated_in_other_impls() {
        let input = quote! {
            #[repr(u32)]
            enum Flag {
                A = 0,
            }
        };
        let output = derive(input).to_string();
        // The repr type (u32) should appear in the direct TryFrom and From impls.
        // The 9 other types must all be present in their own TryFrom impls.
        for t in &[
            "u8", "u16", "u64", "usize", "i8", "i16", "i32", "i64", "isize",
        ] {
            assert!(output.contains(t), "expected TryFrom impl for {}", t);
        }
    }

    #[test]
    fn test_derive_num_enum_missing_repr_emits_compile_error() {
        let input = quote! {
            enum Foo {
                Bar = 0,
            }
        };
        let output = derive(input).to_string();
        assert!(
            output.contains("compile_error"),
            "expected compile_error for missing #[repr]"
        );
        assert!(
            output.contains("NumEnum requires a #[repr(integer_type)] attribute"),
            "expected full error message"
        );
    }

    #[test]
    fn test_derive_num_enum_non_unit_variant_emits_compile_error() {
        let input = quote! {
            #[repr(u8)]
            enum Foo {
                Bar(u32),
            }
        };
        let output = derive(input).to_string();
        assert!(
            output.contains("compile_error"),
            "expected compile_error for non-unit variant"
        );
        assert!(
            output.contains("NumEnum only supports unit variants"),
            "expected full error message"
        );
    }

    #[test]
    fn test_derive_num_enum_missing_discriminant_emits_compile_error() {
        let input = quote! {
            #[repr(u8)]
            enum Foo {
                Bar,
            }
        };
        let output = derive(input).to_string();
        assert!(
            output.contains("compile_error"),
            "expected compile_error for missing discriminant"
        );
        assert!(
            output.contains("must have an explicit discriminant for NumEnum"),
            "expected full error message"
        );
    }

    #[test]
    fn test_derive_num_enum_non_enum_emits_compile_error() {
        let input = quote! {
            #[repr(u8)]
            struct Foo {
                x: u32,
            }
        };
        let output = derive(input).to_string();
        assert!(
            output.contains("compile_error"),
            "expected compile_error for struct input"
        );
        assert!(
            output.contains("NumEnum can only be derived for enums"),
            "expected helpful error message"
        );
    }
}
