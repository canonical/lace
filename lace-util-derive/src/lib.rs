// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only

//! Procedural macros for deriving enum utilities.
//!
//! This crate provides two derive macros for enums:
//!
//! - [`NumEnum`]: Generates bidirectional conversions between enums and integer
//!   types
//! - [`NamedEnum`]: Generates methods for accessing variant names
//!
//! # Examples
//!
//! ```
//! use lace_util_derive::{NumEnum, NamedEnum};
//!
//! #[derive(NumEnum, NamedEnum, Debug, PartialEq)]
//! #[repr(u8)]
//! enum Status {
//!     #[name(short = "ok", long = "Success")]
//!     Ok = 1,
//!     #[name(short = "fail", long = "Failure")]
//!     Fail = 2,
//! }
//!
//! // NumEnum provides integer conversions
//! assert_eq!(Status::try_from(1u8), std::result::Result::Ok(Status::Ok));
//! assert_eq!(u8::from(Status::Fail), 2);
//!
//! // NamedEnum provides name accessors
//! assert_eq!(Status::Ok.short_name(), "ok");
//! assert_eq!(Status::Ok.long_name(), "Success");
//! assert_eq!(Status::try_from_short_name("fail"), Some(Status::Fail));
//! ```

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields};

/// Derive macro for generating bidirectional conversions between enums and
/// integer types.
///
/// This macro generates implementations of `TryFrom<Int>` and `From<Enum>` for
/// enums with a `#[repr(type)]` attribute, where variants are assigned
/// sequential integer values starting from 0.
///
/// # Requirements
///
/// - The enum must have a `#[repr(type)]` attribute specifying an integer type
///   (e.g., `u8`, `i32`)
/// - All enum variants must be unit variants (no fields)
///
/// # Generated Implementations
///
/// - `impl TryFrom<Int> for Enum`: Converts an integer to an enum variant.
///   Returns `Err(())` if the integer doesn't correspond to a valid variant.
/// - `impl From<Enum> for Int`: Converts an enum variant to its corresponding
///   integer value.
///
/// # Examples
///
/// ## Basic usage with u8
///
/// ```
/// use lace_util_derive::NumEnum;
///
/// #[derive(NumEnum, Debug, PartialEq)]
/// #[repr(u8)]
/// enum Color {
///     Red = 0,
///     Green = 1,
///     Blue = 2,
/// }
///
/// // Convert from integer to enum
/// assert_eq!(Color::try_from(0u8), Ok(Color::Red));
/// assert_eq!(Color::try_from(1u8), Ok(Color::Green));
/// assert_eq!(Color::try_from(2u8), Ok(Color::Blue));
/// assert_eq!(Color::try_from(3u8), Err(()));
///
/// // Convert from enum to integer
/// assert_eq!(u8::from(Color::Red), 0);
/// assert_eq!(u8::from(Color::Green), 1);
/// assert_eq!(u8::from(Color::Blue), 2);
/// ```
///
/// ## Using different integer types
///
/// ```
/// use lace_util_derive::NumEnum;
///
/// #[derive(NumEnum, Debug, PartialEq)]
/// #[repr(u16)]
/// enum Level {
///     Low = 1,
///     Medium = 5,
///     High = 1024,
/// }
///
/// assert_eq!(Level::try_from(1u16), Ok(Level::Low));
/// assert_eq!(u16::from(Level::Medium), 5);
/// assert_eq!(u16::from(Level::High), 1024);
/// ```
///
/// # Panics
///
/// This macro will panic at compile time if:
/// - The enum doesn't have a `#[repr(type)]` attribute
/// - Any variant has fields (e.g., `Foo(u32)` or `Bar { x: i32 }`)
#[proc_macro_derive(NumEnum)]
pub fn derive_num_enum(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    // Find the underlying integer type from #[repr(type)]
    let repr_type = input
        .attrs
        .iter()
        .find(|attr| attr.path().is_ident("repr"))
        .map(|attr| {
            attr.parse_args::<syn::Ident>().expect(
                "Expected #[repr(integer_type)] attribute with a valid \
                 integer type (e.g., u8, i32)",
            )
        })
        .expect("Expected #[repr(type)] attribute on enum");

    // Parse enum variants
    let variants = match &input.data {
        Data::Enum(data) => &data.variants,
        _ => panic!("NumEnum can only be derived for enums"),
    };

    // Validate all variants are unit variants
    for variant in variants {
        if !matches!(variant.fields, Fields::Unit) {
            panic!("NumEnum only supports unit variants");
        }
    }

    // Collect variant names and discriminants
    let variant_discriminants: Vec<_> = variants
        .iter()
        .map(|variant| {
            let variant_name = &variant.ident;
            let Some(discriminant) = variant.discriminant.as_ref().map(|(_, expr)| expr) else {
                panic!(
                    "Variant `{}` must have an explicit discriminant for NumEnum",
                    variant_name
                )
            };
            (variant_name, discriminant)
        })
        .collect();

    // Generate match arms for TryFrom<Int>, e.g. `0 => Ok(Color::Red),`
    let try_from_arms = variant_discriminants
        .iter()
        .map(|(variant_name, discriminant)| {
            quote! {
                #discriminant => Ok(#name::#variant_name),
            }
        });

    // Generate impl TryFrom<u8> for Color { fn try_from(...) { match ... } }
    let try_from_impl = quote! {
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
    let expanded = quote! {
        #try_from_impl
        #from_impl
    };

    TokenStream::from(expanded)
}

/// Derive macro for generating name accessor methods for enum variants
///
/// This macro generates methods for accessing short and long names of enum
/// variants, as well as a method for looking up variants by their short name.
///
/// # Attributes
///
/// Variants can be annotated with the `#[name(short = "...", long = "...")]`
/// attribute to specify custom names. If not provided, the variant's identifier
/// is used as both the short and long name.
///
/// # Generated Methods
///
/// - `pub fn short_name(&self) -> &'static str`: Returns the short name of the
///   variant
/// - `pub fn long_name(&self) -> &'static str`: Returns the long name of the
///   variant
/// - `pub fn try_from_short_name(name: &str) -> Option<Self>`: Looks up a
///   variant by its short name
///
/// # Requirements
///
/// - All enum variants must be unit variants (no fields)
///
/// # Examples
///
/// ## With custom names
///
/// ```
/// use lace_util_derive::NamedEnum;
///
/// #[derive(NamedEnum, Debug, PartialEq)]
/// enum Priority {
///     #[name(short = "low", long = "Low Priority")]
///     Low,
///     #[name(short = "med", long = "Medium Priority")]
///     Medium,
///     #[name(short = "high", long = "High Priority")]
///     High,
/// }
///
/// // Access short names
/// assert_eq!(Priority::Low.short_name(), "low");
/// assert_eq!(Priority::Medium.short_name(), "med");
/// assert_eq!(Priority::High.short_name(), "high");
///
/// // Access long names
/// assert_eq!(Priority::Low.long_name(), "Low Priority");
/// assert_eq!(Priority::Medium.long_name(), "Medium Priority");
/// assert_eq!(Priority::High.long_name(), "High Priority");
///
/// // Lookup by short name
/// assert_eq!(Priority::try_from_short_name("low"), Some(Priority::Low));
/// assert_eq!(Priority::try_from_short_name("med"), Some(Priority::Medium));
/// assert_eq!(Priority::try_from_short_name("high"), Some(Priority::High));
/// assert_eq!(Priority::try_from_short_name("invalid"), None);
/// // long name doesn't match
/// assert_eq!(Priority::try_from_short_name("Low Priority"), None);
/// ```
///
/// ## Without custom names (using defaults)
///
/// ```
/// use lace_util_derive::NamedEnum;
///
/// #[derive(NamedEnum, Debug, PartialEq)]
/// enum Animal {
///     Cat,
///     Dog,
///     Bird,
/// }
///
/// // Without #[name] attributes, variant identifiers are used
/// assert_eq!(Animal::Cat.short_name(), "Cat");
/// assert_eq!(Animal::Cat.long_name(), "Cat");
/// assert_eq!(Animal::Dog.short_name(), "Dog");
/// assert_eq!(Animal::try_from_short_name("Cat"), Some(Animal::Cat));
/// assert_eq!(Animal::try_from_short_name("Dog"), Some(Animal::Dog));
/// assert_eq!(Animal::try_from_short_name("Bird"), Some(Animal::Bird));
/// assert_eq!(Animal::try_from_short_name("cat"), None); // case sensitive
/// ```
///
/// # Panics
///
/// This macro will panic at compile time if:
/// - Any variant has fields (e.g., `Foo(u32)` or `Bar { x: i32 }`)
/// - The `#[name]` attribute has invalid syntax
#[proc_macro_derive(NamedEnum, attributes(name))]
pub fn derive_named_enum(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
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
                    "Failed to parse name attribute: expected format #[name(short = \"...\", long = \"...\")]",
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
    let expanded = quote! {
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
    };

    TokenStream::from(expanded)
}
