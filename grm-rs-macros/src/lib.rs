use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, parse_macro_input, Lit};


#[proc_macro_derive(NodeModel)]
pub fn derive_node_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let struct_ident = input.ident.clone(); // the struct name (User, Post, etc.)
    let label_str = struct_ident.to_string(); // default label

    // No support for generics yet — it complicates things.
    if !input.generics.params.is_empty() {
        return syn::Error::new_spanned(
            input.generics,
            "NodeModel derive does not yet support generics",
        )
        .to_compile_error()
        .into();
    }

    // Ensure we’re deriving for a struct with named fields
    let fields = match input.data {
        Data::Struct(s) => match s.fields {
            Fields::Named(named) => named.named,
            _ => {
                return syn::Error::new_spanned(
                    struct_ident,
                    "NodeModel derive requires a struct with named fields",
                )
                .to_compile_error()
                .into();
            }
        },
        _ => {
            return syn::Error::new_spanned(
                struct_ident,
                "NodeModel can only be derived for structs",
            )
            .to_compile_error()
            .into();
        }
    };

    // Identify the id field + collect all other fields as properties
    let mut id_field_ident = None;
    let mut id_field_ty = None;

    let mut prop_idents = Vec::new();
    let mut prop_names = Vec::new();

    for field in fields {
        let ident = field.ident.unwrap();
        let ident_name = ident.to_string();

        if ident_name == "id" {
            id_field_ident = Some(ident.clone());
            id_field_ty = Some(field.ty.clone());
        } else {
            prop_idents.push(ident.clone());
            prop_names.push(ident_name);
        }
    }

    let id_ident = match id_field_ident {
        Some(i) => i,
        None => {
            return syn::Error::new_spanned(
                struct_ident,
                "NodeModel derive requires a field named `id`",
            )
            .to_compile_error()
            .into();
        }
    };

    let id_ty = id_field_ty.unwrap();

    // Generate code for: inserting props into a BTreeMap
    let to_props_inserts = prop_idents
        .iter()
        .zip(prop_names.iter())
        .map(|(ident, key)| {
            quote! {
                map.insert(
                    #key.to_string(),
                    ::serde_json::to_value(&self.#ident)
                        .unwrap_or_else(|e| {
                            panic!(
                                "NodeModel::to_properties failed to serialize field `{}`: {}",
                                #key, e
                            )
                        })
                );
            }
        });

    // Generate code for reconstructing from the map
    let from_props_fields = prop_idents
        .iter()
        .zip(prop_names.iter())
        .map(|(ident, key)| {
            quote! {
                #ident: {
                    let v = props.remove(#key)
                        .ok_or_else(|| ::grm_rs::error::GrmError::Mapping(
                            format!("missing field `{}`", #key)
                        ))?;
                    ::serde_json::from_value(v)
                        .map_err(|e| ::grm_rs::error::GrmError::Mapping(
                            format!("failed to deserialize field `{}`: {}", #key, e)
                        ))?
                }
            }
        });

    // Build the final implementation
    let expanded = quote! {
        impl ::grm_rs::NodeModel for #struct_ident {
            const LABELS: &'static [&'static str] = &[ #label_str ];

            type Id = #id_ty;

            fn id(&self) -> &Self::Id {
                &self.#id_ident
            }

            fn set_id(&mut self, id: Self::Id) {
                self.#id_ident = id;
            }

            fn to_properties(
                &self
            ) -> ::std::collections::BTreeMap<::std::string::String, ::serde_json::Value> {
                let mut map = ::std::collections::BTreeMap::new();
                #(#to_props_inserts)*
                map
            }

            fn from_properties(
                id: Self::Id,
                mut props: ::std::collections::BTreeMap<::std::string::String, ::serde_json::Value>
            ) -> ::grm_rs::error::Result<Self> {
                Ok(Self {
                    #id_ident: id,
                    #(#from_props_fields,)*
                })
            }
        }
    };

    TokenStream::from(expanded)
}


#[proc_macro_derive(RelModel, attributes(grm))]
pub fn derive_rel_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    // Struct name
    let struct_ident = input.ident.clone();

    // Parse the #[grm(...)] attributes
    let mut from_ty: Option<syn::Type> = None;
    let mut to_ty: Option<syn::Type> = None;
    let mut rel_type: Option<String> = None;

    for attr in input.attrs.iter().filter(|a| a.path().is_ident("grm")) {
        match attr.parse_args_with(|meta: syn::parse::ParseStream| {
            let mut from_val = None;
            let mut to_val = None;
            let mut ty_val = None;

            while !meta.is_empty() {
                let ident: syn::Ident = meta.parse()?;
                let _eq: syn::Token![=] = meta.parse()?;
                let lit: Lit = meta.parse()?;

                match ident.to_string().as_str() {
                    "from" => {
                        if let Lit::Str(s) = lit {
                            from_val = Some(s.parse::<syn::Type>()?);
                        }
                    }
                    "to" => {
                        if let Lit::Str(s) = lit {
                            to_val = Some(s.parse::<syn::Type>()?);
                        }
                    }
                    "ty" => {
                        if let Lit::Str(s) = lit {
                            ty_val = Some(s.value());
                        }
                    }
                    other => {
                        return Err(syn::Error::new_spanned(
                            ident,
                            format!("Unknown RelModel attribute `{}`", other),
                        ));
                    }
                }

                // Optional comma
                let _ = meta.parse::<syn::Token![,]>();
            }

            Ok((from_val, to_val, ty_val))
        }) {
            Ok((from, to, ty)) => {
                if from_ty.is_none() {
                    from_ty = from;
                }
                if to_ty.is_none() {
                    to_ty = to;
                }
                if rel_type.is_none() {
                    rel_type = ty;
                }
            }
            Err(err) => return err.to_compile_error().into(),
        }
    }

    // Make sure all params exist
    let from_ty = match from_ty {
        Some(t) => t,
        None => {
            return syn::Error::new_spanned(
                struct_ident.clone(),
                "RelModel requires #[grm(from = \"Type\")]",
            )
            .to_compile_error()
            .into();
        }
    };

    let to_ty = match to_ty {
        Some(t) => t,
        None => {
            return syn::Error::new_spanned(
                struct_ident.clone(),
                "RelModel requires #[grm(to = \"Type\")]",
            )
            .to_compile_error()
            .into();
        }
    };

    let rel_type_str = match rel_type {
        Some(t) => t,
        None => {
            return syn::Error::new_spanned(
                struct_ident.clone(),
                "RelModel requires #[grm(ty = \"REL_TYPE\")]",
            )
            .to_compile_error()
            .into();
        }
    };

    // Extract fields
    let fields = match input.data {
        Data::Struct(s) => match s.fields {
            Fields::Named(named) => named.named,
            _ => {
                return syn::Error::new_spanned(
                    struct_ident.clone(),
                    "RelModel derive requires named fields",
                )
                .to_compile_error()
                .into();
            }
        },
        _ => {
            return syn::Error::new_spanned(
                struct_ident.clone(),
                "RelModel can only be derived for structs",
            )
            .to_compile_error()
            .into();
        }
    };

    // Identify id field + other fields
    let mut id_field_ident = None;
    let mut id_field_ty = None;

    let mut prop_idents = Vec::new();
    let mut prop_names = Vec::new();

    for field in fields {
        let ident = field.ident.clone().unwrap();
        let name = ident.to_string();

        if name == "id" {
            id_field_ident = Some(ident.clone());
            id_field_ty = Some(field.ty.clone());
        } else {
            prop_idents.push(ident.clone());
            prop_names.push(name);
        }
    }

    let id_ident = match id_field_ident {
        Some(i) => i,
        None => {
            return syn::Error::new_spanned(
                struct_ident.clone(),
                "RelModel must contain a field named `id`",
            )
            .to_compile_error()
            .into();
        }
    };

    let id_ty = id_field_ty.unwrap();

    // to_properties()
    let to_props_inserts = prop_idents.iter().zip(prop_names.iter()).map(|(ident, key)| {
        quote! {
            map.insert(
                #key.to_string(),
                ::serde_json::to_value(&self.#ident)
                    .unwrap_or_else(|e| panic!(
                        "RelModel::to_properties failed to serialize `{}`: {}",
                        #key, e
                    ))
            );
        }
    });

    // from_properties()
    let from_props_fields = prop_idents.iter().zip(prop_names.iter()).map(|(ident, key)| {
        quote! {
            #ident: {
                let v = props.remove(#key)
                    .ok_or_else(|| ::grm_rs::error::GrmError::Mapping(
                        format!("missing field `{}`", #key)
                    ))?;
                ::serde_json::from_value(v)
                    .map_err(|e| ::grm_rs::error::GrmError::Mapping(
                        format!("failed to deserialize `{}`: {}", #key, e)
                    ))?
            }
        }
    });

    // Generate final impl
    let expanded = quote! {
        impl ::grm_rs::RelModel for #struct_ident {
            const TYPE: &'static str = #rel_type_str;

            type Id = #id_ty;
            type From = #from_ty;
            type To = #to_ty;

            fn id(&self) -> &Self::Id {
                &self.#id_ident
            }

            fn set_id(&mut self, id: Self::Id) {
                self.#id_ident = id;
            }

            fn to_properties(
                &self
            ) -> ::std::collections::BTreeMap<::std::string::String, ::serde_json::Value> {
                let mut map = ::std::collections::BTreeMap::new();
                #(#to_props_inserts)*
                map
            }

            fn from_properties(
                id: Self::Id,
                mut props: ::std::collections::BTreeMap<::std::string::String, ::serde_json::Value>
            ) -> ::grm_rs::error::Result<Self> {
                Ok(Self {
                    #id_ident: id,
                    #(#from_props_fields,)*
                })
            }
        }
    };

    TokenStream::from(expanded)
}

