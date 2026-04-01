// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.

//! Implementation of the `Display` derive macro.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Data, DeriveInput, Fields, Variant};

/// Derives [`core::fmt::Display`] for an enum or struct.
///
/// Every enum variant and every struct must carry a `#[display("...")]`
/// attribute whose value is a [`format!`]-compatible string literal. All
/// fields are passed positionally (for unnamed fields) or by name (for named
/// fields) to the format string. Returns a `compile_error!` token stream
/// (rather than panicking) when a variant or the struct itself is missing the
/// attribute, so errors point at the offending span.
pub fn derive(input: TokenStream) -> TokenStream {
    match try_derive(input) {
        Ok(ts) => ts,
        Err(e) => e.to_compile_error(),
    }
}

fn try_derive(input: TokenStream) -> syn::Result<TokenStream> {
    let input: DeriveInput = syn::parse2(input)?;
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let body = match &input.data {
        Data::Enum(data) => {
            let arms = data
                .variants
                .iter()
                .map(|variant| generate_arm(name, variant))
                .collect::<syn::Result<Vec<_>>>()?;
            quote! {
                match self {
                    #(#arms)*
                }
            }
        }
        Data::Struct(data) => generate_struct_body(name, &input.attrs, &data.fields)?,
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                name,
                "Display can only be derived for enums and structs",
            ));
        }
    };

    Ok(quote! {
        impl #impl_generics core::fmt::Display for #name #ty_generics #where_clause {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                #body
            }
        }
    })
}

fn generate_arm(enum_name: &syn::Ident, variant: &Variant) -> syn::Result<TokenStream> {
    let variant_name = &variant.ident;

    // Every variant must have a #[display("...")] attribute.
    let fmt_str = variant
        .attrs
        .iter()
        .find(|attr| attr.path().is_ident("display"))
        .map(|attr| attr.parse_args::<syn::LitStr>())
        .transpose()?
        .ok_or_else(|| {
            syn::Error::new_spanned(
                variant_name,
                format!(
                    "variant `{variant_name}` is missing a required \
                     #[display(\"...\")] attribute"
                ),
            )
        })?;

    Ok(match &variant.fields {
        Fields::Unit => {
            quote! {
                #enum_name::#variant_name => write!(f, #fmt_str),
            }
        }
        Fields::Unnamed(fields) => {
            let bindings: Vec<_> = (0..fields.unnamed.len())
                .map(|i| syn::Ident::new(&format!("_{i}"), Span::call_site()))
                .collect();
            quote! {
                #enum_name::#variant_name(#(#bindings),*) => write!(f, #fmt_str, #(#bindings),*),
            }
        }
        Fields::Named(fields) => {
            let bindings: Vec<_> = fields
                .named
                .iter()
                .map(|field| field.ident.as_ref().unwrap())
                .collect();
            quote! {
                #enum_name::#variant_name { #(#bindings),* } => write!(f, #fmt_str, #(#bindings = #bindings),*),
            }
        }
    })
}

/// Generate the body of a `Display::fmt` implementation for a struct.
///
/// The struct itself must carry a `#[display("...")]` attribute. For named
/// fields, `self.field` bindings are created so the format string can
/// reference them by name. For tuple structs, positional `self.N` bindings
/// are created.
fn generate_struct_body(
    name: &syn::Ident,
    attrs: &[syn::Attribute],
    fields: &Fields,
) -> syn::Result<TokenStream> {
    let fmt_str = attrs
        .iter()
        .find(|attr| attr.path().is_ident("display"))
        .map(|attr| attr.parse_args::<syn::LitStr>())
        .transpose()?
        .ok_or_else(|| {
            syn::Error::new_spanned(
                name,
                format!(
                    "struct `{name}` is missing a required \
                     #[display(\"...\")] attribute"
                ),
            )
        })?;

    Ok(match fields {
        Fields::Unit => {
            quote! { write!(f, #fmt_str) }
        }
        Fields::Unnamed(fields) => {
            let bindings: Vec<_> = (0..fields.unnamed.len()).map(syn::Index::from).collect();
            quote! { write!(f, #fmt_str, #(self.#bindings),*) }
        }
        Fields::Named(fields) => {
            let bindings: Vec<_> = fields
                .named
                .iter()
                .map(|field| field.ident.as_ref().unwrap())
                .collect();
            quote! { write!(f, #fmt_str, #(#bindings = self.#bindings),*) }
        }
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use quote::quote;

    #[test]
    fn test_derive_display_unit_variant() {
        let input = quote! {
            enum Foo {
                #[display("not found")]
                NotFound,
            }
        };
        let output = derive(input).to_string();
        assert!(
            output.contains("impl core :: fmt :: Display"),
            "expected Display impl in output"
        );
        assert!(
            output.contains("\"not found\""),
            "expected format string in output"
        );
    }

    #[test]
    fn test_derive_display_unnamed_single_field() {
        let input = quote! {
            enum Foo {
                #[display("error: {}")]
                Err(u32),
            }
        };
        let output = derive(input).to_string();
        assert!(output.contains("_0"), "expected _0 binding for tuple field");
        assert!(output.contains("\"error: {}\""), "expected format string");
    }

    #[test]
    fn test_derive_display_unnamed_multiple_fields() {
        let input = quote! {
            enum Foo {
                #[display("{}: {}")]
                Both(u32, u32),
            }
        };
        let output = derive(input).to_string();
        assert!(output.contains("_0"), "expected _0 binding");
        assert!(output.contains("_1"), "expected _1 binding");
    }

    #[test]
    fn test_derive_display_named_fields() {
        let input = quote! {
            enum Foo {
                #[display("code: {code}")]
                Variant { code: u32 },
            }
        };
        let output = derive(input).to_string();
        assert!(
            output.contains("code = code"),
            "expected named argument `code = code` in output"
        );
    }

    #[test]
    fn test_derive_display_multiple_variants() {
        let input = quote! {
            enum MyError {
                #[display("not found")]
                NotFound,
                #[display("error: {}")]
                Wrapped(u32),
            }
        };
        let output = derive(input).to_string();
        assert!(
            output.contains("\"not found\""),
            "expected first format string"
        );
        assert!(
            output.contains("\"error: {}\""),
            "expected second format string"
        );
        assert!(output.contains("_0"), "expected binding for tuple variant");
    }

    #[test]
    fn test_derive_display_generic_enum() {
        let input = quote! {
            enum Wrapper<T: core::fmt::Display> {
                #[display("wrapped: {}")]
                Wrapped(T),
            }
        };
        let output = derive(input).to_string();
        assert!(
            !output.contains("compile_error"),
            "generic enum should not produce an error"
        );
        // The generic parameter and where-clause must appear in the impl header.
        assert!(output.contains("impl"), "expected impl block");
        assert!(
            output.contains("< T"),
            "expected generic parameter T in impl"
        );
    }

    #[test]
    fn test_derive_display_missing_attribute_emits_compile_error() {
        let input = quote! {
            enum Foo {
                Bar,
            }
        };
        let output = derive(input).to_string();
        assert!(
            output.contains("compile_error"),
            "expected compile_error for missing #[display] attribute"
        );
        assert!(
            output.contains("missing a required"),
            "expected helpful error message"
        );
    }

    #[test]
    fn test_derive_display_union_emits_compile_error() {
        let input = quote! {
            union Foo {
                a: u32,
                b: f32,
            }
        };
        let output = derive(input).to_string();
        assert!(
            output.contains("compile_error"),
            "expected compile_error for union input"
        );
    }

    #[test]
    fn test_derive_display_unit_struct() {
        let input = quote! {
            #[display("something went wrong")]
            struct MyError;
        };
        let output = derive(input).to_string();
        assert!(
            output.contains("impl core :: fmt :: Display"),
            "expected Display impl in output"
        );
        assert!(
            output.contains("\"something went wrong\""),
            "expected format string in output"
        );
        assert!(
            !output.contains("compile_error"),
            "unit struct should not produce an error"
        );
    }

    #[test]
    fn test_derive_display_tuple_struct() {
        let input = quote! {
            #[display("value: {}")]
            struct Wrapper(u32);
        };
        let output = derive(input).to_string();
        assert!(
            output.contains("\"value: {}\""),
            "expected format string in output"
        );
        assert!(
            !output.contains("compile_error"),
            "tuple struct should not produce an error"
        );
    }

    #[test]
    fn test_derive_display_named_struct() {
        let input = quote! {
            #[display("x={x}, y={y}")]
            struct Point {
                x: i32,
                y: i32,
            }
        };
        let output = derive(input).to_string();
        assert!(
            output.contains("\"x={x}, y={y}\""),
            "expected format string in output"
        );
        assert!(
            !output.contains("compile_error"),
            "named struct should not produce an error"
        );
    }

    #[test]
    fn test_derive_display_struct_missing_attribute_emits_compile_error() {
        let input = quote! {
            struct Foo;
        };
        let output = derive(input).to_string();
        assert!(
            output.contains("compile_error"),
            "expected compile_error for struct missing #[display] attribute"
        );
        assert!(
            output.contains("missing a required"),
            "expected helpful error message"
        );
    }
}
