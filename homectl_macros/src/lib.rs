#![recursion_limit="128"]
extern crate proc_macro;
use crate::proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, Data, Meta, Fields, Type, Lit, MetaNameValue,NestedMeta};

extern crate proc_macro2;
use proc_macro2::{Ident, Span};

use std::collections::HashMap;

/// Extracts attribute property values from a Meta array
fn extract_prop(meta: &[Meta], attr: &str, prop: &str) -> Vec<String> {
    meta.iter()
        .filter_map(|meta| match meta {
            Meta::List(metalist) => {
                if metalist.ident == attr {
                    Some(&metalist.nested)
                } else {
                    None
                }
            },
            _ => None,
        })
        .flatten()
        .filter_map(|meta| match meta {
            NestedMeta::Meta(Meta::NameValue(MetaNameValue {
                ident,
                lit: Lit::Str(s),
                ..
            })) => {
                if ident == prop {
                    Some(s.value())
                } else {
                    None
                }
            },
            _ => None,
        })
        .collect()
}

#[proc_macro_derive(Commandable)]
pub fn dev_derive(input: TokenStream) -> TokenStream {
    // Command traits that must be implemented for all variants
    const DEFAULT_CMDS: [&str; 1] = ["SmartDeviceCommands"];
    let input = syn::parse_macro_input!(input as DeriveInput);

    let name = &input.ident;

    // Make sure we're dealing with an enum
    let vars = match &input.data {
        Data::Enum(v) => &v.variants,
        _ => panic!("Commandable can only be derived by enums")
    };

    let mut var_paths = Vec::new();
    let mut dev_paths = Vec::new();
    let mut var_cmds = HashMap::new();

    for var in vars {
        // Get device struct path...
        match &var.fields {
            Fields::Unnamed(u) => {
                if u.unnamed.len() != 1 {
                    panic!("Variant of {} must be 1-tuples", name);
                } else {
                    match &u.unnamed[0].ty {
                        Type::Path(p) => dev_paths.push(p),
                        _ => panic!("Variant field is not a path")
                    }
                }
            },
            _ => {
                panic!(
                    "{} must be composed of 1-tuple variants exclusively",
                    name
                )
            },
        }

        let var_name = &var.ident;
        // then the variant path...
        var_paths.push(quote!{ #name::#var_name });

        // and the command traits we need to support...
        let meta: Vec<Meta> = var.attrs
            .iter()
            .filter_map(|a| a.interpret_meta())
            .collect();
        let mut commands = extract_prop(&meta, "homectl", "cmd").iter()
            .map(|s| Ident::new(s, Span::call_site())).collect::<Vec<Ident>>();
        // including the default ones
        for c in &DEFAULT_CMDS {
            commands.push(Ident::new(c, Span::call_site()));
        }
        var_cmds.insert(var_paths.last().unwrap().to_string(), commands);
    }

    let display = {
        let var_paths = var_paths.clone();
        quote! {
            impl ::std::fmt::Display for #name {
                fn fmt(
                    &self,
                    f: &mut ::std::fmt::Formatter
                ) -> ::std::fmt::Result {
                    match self {
                        #(#var_paths(d) => d.fmt(f),)*
                    }
                }
            }
        }
    };

    let discover = {
        let var_paths = var_paths.clone();
        let dev_paths = dev_paths.clone();
        quote! {
            fn discover() ->::std::io::Result<
                ::std::option::Option<::std::vec::Vec<#name>>
            > {
                use ::std::vec::Vec;
                let mut ret: Vec<#name> = Vec::new();

                #(if let Some(devs) = <#dev_paths>::discover()? {
                    ret.append(
                        &mut devs.into_iter()
                            .map(|d| #var_paths(d))
                            .collect()
                    );
                })*

                if ret.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(ret))
                }
            }
        }
    };

    let from_address = {
        let var_paths = var_paths.clone();
        quote! {
            fn from_address(addr: &::std::net::IpAddr) -> ::std::io::Result<
                ::std::option::Option<#name>
            > {
                #(if let Some(dev) = <#dev_paths>::from_address(&addr)? {
                    Ok(Some(#var_paths(dev)))
                } else)* {
                    Ok(None)
                }
            }
        }
    };


    // Assemble exec arms for each device type
    let mut exec_arms = Vec::new();
    for vp in &var_paths {
        let cmds = var_cmds.get(&vp.to_string()).unwrap();
        exec_arms.push(quote! {
            #vp(dev) => {
                // We ignore CommandNotSupported errors since the next command
                // trait might be able to handle it.
                #(match (dev as &mut dyn #cmds).exec(command) {
                    Err(Error::CommandNotSupported) => (),
                    res                             => return res,
                })*
            }
        });
    }

    let exec = {
        quote! {
            fn exec(&mut self, command: &Command) -> ExecResult {
                match self {
                    #(#exec_arms),*
                }
                
                // None of the command traits were able to handle the supplied
                // command
                Err(Error::CommandNotSupported)
            }
        }
    };

    let description = {
        quote! {
            fn description(&self) -> String {
                match self {
                    #(#var_paths(d) => {
                        d.name() + " @ " + &d.address().to_string()
                    },)*
                }
            }
        }
    };

    TokenStream::from(quote! {
        impl Commandable for #name {
            #discover
            #from_address
            #exec
            #description
        }
        #display
    })
}
