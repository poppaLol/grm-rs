use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::DeriveInput;
use syn::Field;

/// Is this field the GRM "ID field"?
///
/// Rules:
///  - If it has `#[grm(id)]` → YES
///  - Else if its name is literally `id` → YES (backwards compat)
///  - Otherwise → NO
pub fn is_grm_id_field(field: &Field) -> bool {
    // Explicit marker wins: #[grm(id)]
    for attr in &field.attrs {
        if !attr.path().is_ident("grm") {
            continue;
        }

        // syn 2.x: use parse_nested_meta to walk #[grm(...)]
        let mut is_id = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("id") {
                is_id = true;
            }
            Ok(())
        });

        if is_id {
            return true;
        }
    }

    // Fallback: a field literally named `id`
    if let Some(ident) = &field.ident
        && ident == "id"
    {
        return true;
    }

    false
}

pub fn generate_property_helpers(input: &DeriveInput) -> TokenStream2 {
    let struct_ident = &input.ident;
    let mut methods = Vec::new();

    let data = match &input.data {
        syn::Data::Struct(data) => data,
        _ => {
            return quote! {
                compile_error!("NodeModel can only be derived for structs");
            };
        }
    };

    let fields = match &data.fields {
        syn::Fields::Named(named) => &named.named,
        _ => {
            // For tuple/unit structs we simply don't generate property helpers.
            return TokenStream2::new();
        }
    };

    for field in fields {
        // Skip the ID field (GRM semantics)
        if is_grm_id_field(field) {
            continue;
        }

        let field_ident = match &field.ident {
            Some(ident) => ident,
            None => continue,
        };

        let field_name_str = field_ident.to_string();
        let field_ty = &field.ty;
        let method_name = format_ident!("{}_prop", field_name_str);

        methods.push(quote! {
            pub fn #method_name() -> ::grm_rs::dsl::Property<#struct_ident, #field_ty> {
                ::grm_rs::dsl::Property::<#struct_ident, #field_ty>::new(#field_name_str)
            }
        });
    }

    if methods.is_empty() {
        TokenStream2::new()
    } else {
        quote! {
            impl #struct_ident {
                #(#methods)*
            }
        }
    }
}
