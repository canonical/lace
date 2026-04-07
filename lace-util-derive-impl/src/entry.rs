// SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
// Copyright (C) 2026, Canonical Ltd.

//! Implementation of the `entry` attribute macro.

use proc_macro2::TokenStream;
use quote::quote;
use syn::ItemFn;

/// Rewrites the annotated function with
/// `#[unsafe(export_name = "lace_app_main")]`.
///
/// Returns a `compile_error!` token stream (rather than panicking) when
/// arguments are passed to the attribute or the input cannot be parsed as a
/// function.
pub fn apply(args: TokenStream, input: TokenStream) -> TokenStream {
    match try_apply(args, input) {
        Ok(ts) => ts,
        Err(e) => e.to_compile_error(),
    }
}

fn try_apply(args: TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new_spanned(
            args,
            "#[entry] does not accept any arguments",
        ));
    }
    let func: ItemFn = syn::parse2(input)?;
    Ok(quote! {
        #[unsafe(export_name = "lace_app_main")]
        #func
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use proc_macro2::TokenStream;
    use quote::quote;

    #[test]
    fn test_entry_apply_adds_export_name() {
        let args = TokenStream::new();
        let input = quote! { fn main() {} };
        let output = apply(args, input).to_string();
        assert!(
            output.contains("export_name"),
            "expected export_name attribute in output"
        );
        assert!(
            output.contains("lace_app_main"),
            "expected lace_app_main as the export name"
        );
    }

    #[test]
    fn test_entry_apply_preserves_function_body() {
        let args = TokenStream::new();
        let input = quote! { fn my_entry() {} };
        let output = apply(args, input).to_string();
        assert!(
            output.contains("fn my_entry"),
            "expected original function to be preserved in output"
        );
    }

    #[test]
    fn test_entry_apply_with_args_emits_compile_error() {
        let args = quote! { some_arg };
        let input = quote! { fn main() {} };
        let output = apply(args, input).to_string();
        assert!(
            output.contains("compile_error"),
            "expected compile_error when arguments are provided"
        );
        assert!(
            output.contains("#[entry] does not accept any arguments"),
            "expected full error message"
        );
    }

    #[test]
    fn test_entry_apply_non_function_emits_compile_error() {
        let args = TokenStream::new();
        let input = quote! { struct Foo {} };
        let output = apply(args, input).to_string();
        assert!(
            output.contains("compile_error"),
            "expected compile_error when input is not a function"
        );
    }
}
