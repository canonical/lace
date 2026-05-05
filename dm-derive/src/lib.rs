use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

/// Attribute macro that marks a trait as a queryable device interface.
///
/// Generates an `unsafe impl dm::Interface for dyn TraitName` with `to_erased`
/// and `from_erased` methods that transmute between `&dyn TraitName` and
/// `*const dyn Erased`. This is sound because all `dyn` pointer types have the same size.
///
/// Usage:
/// ```ignore
/// #[dm::interface]
/// trait InputDevice: std::fmt::Debug {}
/// ```
#[proc_macro_attribute]
pub fn interface(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_trait: syn::ItemTrait =
        syn::parse(item).expect("#[interface] can only be applied to traits");
    let name = &item_trait.ident;

    let expanded = quote! {
        #item_trait

        unsafe impl dm::Interface for dyn #name {
            unsafe fn to_erased(&self) -> *const dyn dm::Erased {
                ::std::mem::transmute(self)
            }

            unsafe fn from_erased<'a>(raw: *const dyn dm::Erased) -> &'a (dyn #name + 'static) {
                ::std::mem::transmute(raw)
            }
        }
    };

    TokenStream::from(expanded)
}

/// Derive macro that generates a `Device` impl with `query_raw` support.
///
/// The `#[interfaces(...)]` attribute lists the traits this type implements.
/// The generated `query_raw` method checks the requested `TypeId` against each
/// listed trait and returns a type-erased pointer via `Interface::to_erased`.
///
/// Usage:
/// ```ignore
/// #[derive(Device)]
/// #[interfaces(InputDevice, OutputDevice)]
/// struct SerialPort {}
/// ```
#[proc_macro_derive(Device, attributes(interfaces))]
pub fn derive_device(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    // Collect trait paths from #[interfaces(...)]
    let mut trait_paths = Vec::new();
    for attr in &input.attrs {
        if attr.path().is_ident("interfaces") {
            attr.parse_nested_meta(|meta| {
                trait_paths.push(meta.path);
                Ok(())
            })
            .expect("#[interfaces(...)] must contain comma-separated trait names");
        }
    }

    // Generate the match arms: one per trait
    let arms = trait_paths.iter().map(|path| {
        quote! {
            if id == ::std::any::TypeId::of::<dyn #path>() {
                return Some(unsafe { dm::Interface::to_erased(self as &dyn #path) });
            }
        }
    });

    let expanded = quote! {
        impl dm::Device for #name {
            fn query_raw(&self, id: ::std::any::TypeId) -> Option<*const dyn dm::Erased> {
                #(#arms)*
                None
            }
        }
    };

    TokenStream::from(expanded)
}
