//! `#[locora_plugin]`: turns a type implementing `locora_sdk::Plugin` into an
//! exported WebAssembly plugin component.

use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Ident, ItemStruct, LitStr, Token, bracketed};

struct Args {
    name: Option<LitStr>,
    permissions: Vec<Ident>,
}

impl Parse for Args {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut args = Args {
            name: None,
            permissions: Vec::new(),
        };
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            match key.to_string().as_str() {
                "name" => args.name = Some(input.parse()?),
                "permissions" => {
                    let content;
                    bracketed!(content in input);
                    let list = Punctuated::<Ident, Token![,]>::parse_terminated(&content)?;
                    args.permissions = list.into_iter().collect();
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown option `{other}`; expected `name` or `permissions`"),
                    ));
                }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(args)
    }
}

const KNOWN_PERMISSIONS: &[&str] = &[
    "storage",
    "palaces_read",
    "palaces_write",
    "notify",
    "ui_highlight",
];

/// Export a plugin.
///
/// ```ignore
/// #[locora_plugin(name = "Lernzeit", permissions = [storage, palaces_read])]
/// struct Lernzeit;
/// ```
///
/// The plugin id and version come from `Cargo.toml`. `name` defaults to the id.
#[proc_macro_attribute]
pub fn locora_plugin(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(attr as Args);
    let item = syn::parse_macro_input!(item as ItemStruct);
    let ty = &item.ident;

    for perm in &args.permissions {
        if !KNOWN_PERMISSIONS.contains(&perm.to_string().as_str()) {
            return syn::Error::new(
                perm.span(),
                format!(
                    "unknown permission `{perm}`; available: {}",
                    KNOWN_PERMISSIONS.join(", ")
                ),
            )
            .to_compile_error()
            .into();
        }
    }
    let permissions = args
        .permissions
        .iter()
        .map(|p| p.to_string().replacen('_', ":", 1));
    let name = match &args.name {
        Some(name) => quote!(#name),
        None => quote!(env!("CARGO_PKG_NAME")),
    };

    quote! {
        #item

        #[doc(hidden)]
        struct __LocoraPluginExport;

        impl ::locora_sdk::__bindings::Guest for __LocoraPluginExport {
            fn manifest() -> ::locora_sdk::__bindings::locora::plugin::types::Manifest {
                ::locora_sdk::__rt::manifest(
                    env!("CARGO_PKG_NAME"),
                    #name,
                    env!("CARGO_PKG_VERSION"),
                    &[#(#permissions),*],
                )
            }

            fn activate() -> ::core::result::Result<(), ::std::string::String> {
                ::locora_sdk::__rt::activate::<#ty>()
            }

            fn deactivate() {
                ::locora_sdk::__rt::deactivate::<#ty>()
            }

            fn on_event(
                topic: ::std::string::String,
                payload: ::std::vec::Vec<u8>,
            ) -> ::core::result::Result<(), ::std::string::String> {
                ::locora_sdk::__rt::dispatch(&topic, &payload)
            }

            fn on_filter(
                hook: ::std::string::String,
                value: ::std::vec::Vec<u8>,
            ) -> ::core::result::Result<::std::vec::Vec<u8>, ::std::string::String> {
                ::locora_sdk::__rt::filter(&hook, &value)
            }

            fn on_call(
                service: ::std::string::String,
                request: ::std::vec::Vec<u8>,
            ) -> ::core::result::Result<::std::vec::Vec<u8>, ::std::string::String> {
                ::locora_sdk::__rt::call(&service, &request)
            }
        }

        ::locora_sdk::__bindings::__export_plugin!(__LocoraPluginExport);
    }
    .into()
}
