// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.

//! Procedural macros for deriving utilities.
//!
//! This crate provides derive macros:
//!
//! - [`Display`]: Generates a [`core::fmt::Display`] implementation
//! - [`NumEnum`]: Generates bidirectional conversions between enums and integer
//!   types
//! - [`NamedEnum`]: Generates methods for accessing enum variant names
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

use lace_util_derive_impl as impl_;
use proc_macro::TokenStream;

/// Derive macro for generating bidirectional conversions between enums and
/// integer types.
///
/// This macro generates implementations of `TryFrom<Int>` and `From<Enum>` for
/// enums with a `#[repr(integer_type)]` attribute, where each variant must
/// carry an explicit integer discriminant.
///
/// # Requirements
///
/// - The enum must have a `#[repr(integer_type)]` attribute specifying an
///   integer type (e.g., `u8`, `i32`)
/// - All enum variants must be unit variants (no fields)
/// - Every variant must have an explicit discriminant (e.g., `Red = 0`)
///
/// # Generated Implementations
///
/// - `impl TryFrom<Int> for Enum`: Converts an integer to an enum variant.
///   Returns `Err(())` if the integer doesn't correspond to a valid variant.
///   Generated for: u8, u16, u32, u64, usize, i8, i16, i32, i64, isize.
/// - `impl From<Enum> for Int`: Converts an enum variant to its corresponding
///   integer value (for the repr type only).
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
/// ## Converting from arbitrary integer types
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
/// // TryFrom is implemented for all integer types
/// assert_eq!(Color::try_from(0u32), Ok(Color::Red));
/// assert_eq!(Color::try_from(1i64), Ok(Color::Green));
/// assert_eq!(Color::try_from(2usize), Ok(Color::Blue));
/// assert_eq!(Color::try_from(256u32), Err(()));  // Out of u8 range
/// assert_eq!(Color::try_from(-1i32), Err(()));   // Negative value
/// ```
///
/// # Compile-time errors
///
/// This macro will emit a compiler error (via `compile_error!`) if:
/// - The input is not an enum
/// - The enum is missing a `#[repr(integer_type)]` attribute
/// - Any variant has fields (e.g., `Foo(u32)` or `Bar { x: i32 }`)
/// - Any variant is missing an explicit discriminant
#[proc_macro_derive(NumEnum)]
pub fn derive_num_enum(input: TokenStream) -> TokenStream {
    impl_::num_enum::derive(input.into()).into()
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
/// # Compile-time errors
///
/// This macro will emit a compiler error (via `compile_error!`) if:
/// - The input is not an enum
/// - Any variant has fields (e.g., `Foo(u32)` or `Bar { x: i32 }`)
/// - The `#[name]` attribute has invalid syntax
#[proc_macro_derive(NamedEnum, attributes(name))]
pub fn derive_named_enum(input: TokenStream) -> TokenStream {
    impl_::named_enum::derive(input.into()).into()
}

/// Attribute macro to mark the application entry point function.
///
/// This macro transforms the annotated function into the application entry
/// point by applying the `#[unsafe(export_name = "lace_app_main")]` attribute.
/// # Usage
/// ```ignore
/// use lace_util_derive::entry;
/// #[entry]
/// fn main() {
///     // Application code here
/// }
/// ```
/// # Compile-time errors
///
/// This macro will emit a compiler error (via `compile_error!`) if:
/// - Any arguments are provided to the attribute
/// - The attribute is applied to input that does not parse as a function
#[proc_macro_attribute]
pub fn entry(args: TokenStream, input: TokenStream) -> TokenStream {
    impl_::entry::apply(args.into(), input.into()).into()
}

/// Derive macro for generating [`core::fmt::Display`] implementations for
/// enums and structs.
///
/// Every enum variant and every struct must have a `#[display("format
/// string")]` attribute providing a [`format!`]-compatible format string. All
/// fields are passed **positionally** to the format string: tuple-variant /
/// tuple-struct fields are bound as `_0`, `_1`, ...; named-field variants
/// and named-field structs pass fields as **named** arguments, so the format
/// string can reference them by name (e.g., `{field}`). Positional (`{}`)
/// and indexed (`{0}`) placeholders also work for all field kinds.
///
/// # Examples
///
/// ## Enums
///
/// ```
/// use lace_util_derive::Display;
///
/// #[derive(Display, Debug)]
/// enum MyError {
///     #[display("not found")]
///     NotFound,
///     #[display("I/O error: {}")]
///     Io(std::fmt::Error),
///     #[display("code {}: {}")]
///     WithCode(u32, std::fmt::Error),
/// }
///
/// assert_eq!(MyError::NotFound.to_string(), "not found");
/// assert_eq!(
///     MyError::Io(std::fmt::Error).to_string(),
///     "I/O error: an error occurred when formatting an argument"
/// );
/// assert_eq!(
///     MyError::WithCode(42, std::fmt::Error).to_string(),
///     "code 42: an error occurred when formatting an argument"
/// );
/// ```
///
/// ## Unit struct
///
/// ```
/// use lace_util_derive::Display;
///
/// #[derive(Display, Debug)]
/// #[display("something went wrong")]
/// struct MyError;
///
/// assert_eq!(MyError.to_string(), "something went wrong");
/// ```
///
/// ## Tuple struct
///
/// ```
/// use lace_util_derive::Display;
///
/// #[derive(Display)]
/// #[display("value: {}")]
/// struct Wrapper(u32);
///
/// assert_eq!(Wrapper(42).to_string(), "value: 42");
/// ```
///
/// ## Named-field struct
///
/// ```
/// use lace_util_derive::Display;
///
/// #[derive(Display)]
/// #[display("{key}: {value}")]
/// struct KeyValue {
///     key: &'static str,
///     value: u32,
/// }
///
/// assert_eq!(KeyValue { key: "answer", value: 42 }.to_string(), "answer: 42");
/// ```
///
/// # Compile-time errors
///
/// This macro will emit a compiler error (via `compile_error!`) if:
/// - It is applied to a union type
/// - Any enum variant is missing a `#[display("...")]` attribute
/// - A struct is missing a `#[display("...")]` attribute
#[proc_macro_derive(Display, attributes(display))]
pub fn derive_display(input: TokenStream) -> TokenStream {
    impl_::display::derive(input.into()).into()
}
