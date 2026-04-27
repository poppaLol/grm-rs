mod helpers;

use helpers::generate_property_helpers;
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    Data, DeriveInput, Error, Field, Fields, Ident, Lit, Token, Type, parse, parse_macro_input,
};

struct ModelField {
    ident: Ident,
    ty: Type,
    name: String,
}

fn ensure_no_generics(input: &DeriveInput, derive_name: &str) -> Result<(), Error> {
    if input.generics.params.is_empty() {
        Ok(())
    } else {
        Err(Error::new_spanned(
            &input.generics,
            format!("{derive_name} derive does not yet support generics"),
        ))
    }
}

fn named_struct_fields(input: &DeriveInput, derive_name: &str) -> Result<Vec<ModelField>, Error> {
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => &named.named,
            _ => {
                return Err(Error::new_spanned(
                    &input.ident,
                    format!("{derive_name} derive requires a struct with named fields"),
                ));
            }
        },
        _ => {
            return Err(Error::new_spanned(
                &input.ident,
                format!("{derive_name} can only be derived for structs"),
            ));
        }
    };

    Ok(fields.iter().map(model_field).collect())
}

fn model_field(field: &Field) -> ModelField {
    let ident = field
        .ident
        .clone()
        .expect("named_struct_fields only accepts named fields");

    ModelField {
        name: ident.to_string(),
        ident,
        ty: field.ty.clone(),
    }
}

fn take_required_field(
    fields: &mut Vec<ModelField>,
    field_name: &str,
    derive_name: &str,
) -> Result<ModelField, Error> {
    let index = fields
        .iter()
        .position(|field| field.name == field_name)
        .ok_or_else(|| {
            Error::new_spanned(
                &field_name,
                format!("{derive_name} must contain a field named `{field_name}`"),
            )
        })?;

    Ok(fields.remove(index))
}

fn property_serializers(props: &[ModelField], trait_name: &str) -> Vec<TokenStream2> {
    props
        .iter()
        .map(|field| {
            let ident = &field.ident;
            let key = &field.name;

            quote! {
                map.insert(
                    #key.to_string(),
                    ::serde_json::to_value(&self.#ident)
                        .unwrap_or_else(|e| {
                            panic!(
                                concat!(#trait_name, "::to_properties failed to serialize field `{}`: {}"),
                                #key,
                                e
                            )
                        })
                );
            }
        })
        .collect()
}

fn property_deserializers(props: &[ModelField]) -> Vec<TokenStream2> {
    props
        .iter()
        .map(|field| {
            let ident = &field.ident;
            let key = &field.name;

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
        })
        .collect()
}

fn parse_rel_model_attrs(input: &DeriveInput) -> Result<(Type, Type, String), Error> {
    let mut from_ty: Option<Type> = None;
    let mut to_ty: Option<Type> = None;
    let mut rel_type: Option<String> = None;

    for attr in input
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("grm"))
    {
        let (from, to, ty) = attr.parse_args_with(|meta: parse::ParseStream| {
            let mut from_val = None;
            let mut to_val = None;
            let mut ty_val = None;

            while !meta.is_empty() {
                let ident: Ident = meta.parse()?;
                let _eq: Token![=] = meta.parse()?;
                let lit: Lit = meta.parse()?;

                match ident.to_string().as_str() {
                    "from" => {
                        if let Lit::Str(s) = lit {
                            from_val = Some(s.parse::<Type>()?);
                        }
                    }
                    "to" => {
                        if let Lit::Str(s) = lit {
                            to_val = Some(s.parse::<Type>()?);
                        }
                    }
                    "ty" => {
                        if let Lit::Str(s) = lit {
                            ty_val = Some(s.value());
                        }
                    }
                    other => {
                        return Err(Error::new_spanned(
                            ident,
                            format!("Unknown RelModel attribute `{other}`"),
                        ));
                    }
                }

                let _ = meta.parse::<Token![,]>();
            }

            Ok((from_val, to_val, ty_val))
        })?;

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

    let struct_ident = &input.ident;

    Ok((
        from_ty.ok_or_else(|| {
            Error::new_spanned(struct_ident, "RelModel requires #[grm(from = \"Type\")]")
        })?,
        to_ty.ok_or_else(|| {
            Error::new_spanned(struct_ident, "RelModel requires #[grm(to = \"Type\")]")
        })?,
        rel_type.ok_or_else(|| {
            Error::new_spanned(struct_ident, "RelModel requires #[grm(ty = \"REL_TYPE\")]")
        })?,
    ))
}

#[proc_macro_derive(NodeModel, attributes(grm))]
pub fn derive_node_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let prop_helpers = generate_property_helpers(&input);
    let struct_ident = input.ident.clone();
    let label_str = struct_ident.to_string();

    if let Err(err) = ensure_no_generics(&input, "NodeModel") {
        return err.to_compile_error().into();
    }

    let mut fields = match named_struct_fields(&input, "NodeModel") {
        Ok(fields) => fields,
        Err(err) => return err.to_compile_error().into(),
    };

    let id_field = match take_required_field(&mut fields, "id", "NodeModel derive") {
        Ok(field) => field,
        Err(err) => return err.to_compile_error().into(),
    };

    let id_ident = id_field.ident;
    let id_ty = id_field.ty;
    let to_props_inserts = property_serializers(&fields, "NodeModel");
    let from_props_fields = property_deserializers(&fields);

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
        #prop_helpers
    };

    TokenStream::from(expanded)
}

#[proc_macro_derive(RelModel, attributes(grm))]
pub fn derive_rel_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let prop_helpers = generate_property_helpers(&input);
    let struct_ident = input.ident.clone();

    if let Err(err) = ensure_no_generics(&input, "RelModel") {
        return err.to_compile_error().into();
    }

    let (from_ty, to_ty, rel_type_str) = match parse_rel_model_attrs(&input) {
        Ok(attrs) => attrs,
        Err(err) => return err.to_compile_error().into(),
    };

    let mut fields = match named_struct_fields(&input, "RelModel") {
        Ok(fields) => fields,
        Err(err) => return err.to_compile_error().into(),
    };

    let id_field = match take_required_field(&mut fields, "id", "RelModel derive") {
        Ok(field) => field,
        Err(err) => return err.to_compile_error().into(),
    };
    let from_field = match take_required_field(&mut fields, "from", "RelModel derive") {
        Ok(field) => field,
        Err(err) => return err.to_compile_error().into(),
    };
    let to_field = match take_required_field(&mut fields, "to", "RelModel derive") {
        Ok(field) => field,
        Err(err) => return err.to_compile_error().into(),
    };

    let id_ident = id_field.ident;
    let id_ty = id_field.ty;
    let from_ident = from_field.ident;
    let to_ident = to_field.ident;
    let to_props_inserts = property_serializers(&fields, "RelModel");
    let from_props_fields = property_deserializers(&fields);

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

            fn from_parts(
                id: Self::Id,
                from: <Self::From as ::grm_rs::NodeModel>::Id,
                to: <Self::To as ::grm_rs::NodeModel>::Id,
                mut props: ::std::collections::BTreeMap<::std::string::String, ::serde_json::Value>
            ) -> ::grm_rs::error::Result<Self> {
                Ok(Self {
                    #id_ident: id,
                    #from_ident: from,
                    #to_ident: to,
                    #(#from_props_fields,)*
                })
            }
        }

        #prop_helpers
    };

    TokenStream::from(expanded)
}
