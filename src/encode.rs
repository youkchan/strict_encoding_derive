// LNP/BP client-side-validation library implementing respective LNPBP
// specifications & standards (LNPBP-7, 8, 9, 42)
//
// Written in 2019-2021 by
//     Dr. Maxim Orlovsky <orlovsky@pandoracore.com>
//
// To the extent possible under law, the author(s) have dedicated all
// copyright and related and neighboring rights to this software to
// the public domain worldwide. This software is distributed without
// any warranty.
//
// You should have received a copy of the Apache 2.0 License along with this
// software. If not, see <https://opensource.org/licenses/Apache-2.0>.

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{ToTokens, TokenStreamExt};
use syn::spanned::Spanned;
use syn::{
    Data, DataEnum, DataStruct, DeriveInput, Error, Field, Fields, Ident,
    ImplGenerics, Index, Result, TypeGenerics, WhereClause,
};

use amplify::proc_attr::ParametrizedAttr;

use crate::param::EncodingDerive;
use crate::ATTR_NAME;

pub(crate) fn encode_derive(input: DeriveInput) -> Result<TokenStream2> {
    let (impl_generics, ty_generics, where_clause) =
        input.generics.split_for_impl();
    let ident_name = &input.ident;

    let global_param = ParametrizedAttr::with(ATTR_NAME, &input.attrs)?;

    match input.data {
        Data::Struct(data) => encode_struct_impl(
            data,
            ident_name,
            global_param,
            impl_generics,
            ty_generics,
            where_clause,
        ),
        Data::Enum(data) => encode_enum_impl(
            data,
            ident_name,
            global_param,
            impl_generics,
            ty_generics,
            where_clause,
        ),
        //strict_encode_inner_enum(&input, &data),
        Data::Union(_) => Err(Error::new_spanned(
            &input,
            "Deriving StrictEncode is not supported in unions",
        )),
    }
}

fn encode_struct_impl(
    data: DataStruct,
    ident_name: &Ident,
    mut global_param: ParametrizedAttr,
    impl_generics: ImplGenerics,
    ty_generics: TypeGenerics,
    where_clause: Option<&WhereClause>,
) -> Result<TokenStream2> {
    let encoding = EncodingDerive::try_from(&mut global_param, true, false)?;

    let inner_impl = match data.fields {
        Fields::Named(ref fields) => {
            encode_fields_impl(&fields.named, global_param, false)?
        }
        Fields::Unnamed(ref fields) => {
            encode_fields_impl(&fields.unnamed, global_param, false)?
        }
        Fields::Unit => quote! { Ok(0) },
    };

    let import = encoding.use_crate;

    Ok(quote! {
        #[allow(unused_qualifications)]
        impl #impl_generics #import::StrictEncode for #ident_name #ty_generics #where_clause {
            fn strict_encode<E: ::std::io::Write>(&self, mut e: E) -> Result<usize, #import::Error> {
                use #import::StrictEncode;
                let mut len = 0;
                let data = self;
                #inner_impl
                Ok(len)
            }
        }
    })
}

fn encode_enum_impl(
    data: DataEnum,
    ident_name: &Ident,
    mut global_param: ParametrizedAttr,
    impl_generics: ImplGenerics,
    ty_generics: TypeGenerics,
    where_clause: Option<&WhereClause>,
) -> Result<TokenStream2> {
    let encoding = EncodingDerive::try_from(&mut global_param, true, true)?;
    let repr = encoding.repr;

    let mut inner_impl = TokenStream2::new();

    for (order, variant) in data.variants.iter().enumerate() {
        let mut local_param =
            ParametrizedAttr::with(ATTR_NAME, &variant.attrs)?;

        // First, test individual attribute
        let _ = EncodingDerive::try_from(&mut local_param, false, true)?;
        // Second, combine global and local together
        let mut combined = global_param.clone().merged(local_param.clone())?;
        combined.args.remove("repr");
        combined.args.remove("crate");
        let encoding = EncodingDerive::try_from(&mut combined, false, true)?;

        if encoding.skip {
            continue;
        }

        let captures = variant
            .fields
            .iter()
            .enumerate()
            .map(|(i, f)| {
                f.ident.as_ref().map(Ident::to_token_stream).unwrap_or_else(
                    || {
                        Ident::new(&format!("_{}", i), Span::call_site())
                            .to_token_stream()
                    },
                )
            })
            .collect::<Vec<_>>();

        let (field_impl, bra_captures_ket) = match variant.fields {
            Fields::Named(ref fields) => (
                encode_fields_impl(&fields.named, local_param, true)?,
                quote! { { #( #captures ),* } },
            ),
            Fields::Unnamed(ref fields) => (
                encode_fields_impl(&fields.unnamed, local_param, true)?,
                quote! { ( #( #captures ),* ) },
            ),
            Fields::Unit => (TokenStream2::new(), TokenStream2::new()),
        };

        let captures = match captures.len() {
            0 => quote! {},
            _ => quote! { let data = ( #( #captures ),* , ); },
        };

        let ident = &variant.ident;
        let value = match (encoding.value, encoding.by_order) {
            (Some(val), _) => val.to_token_stream(),
            (None, true) => Index::from(order as usize).to_token_stream(),
            (None, false) => quote! { Self::#ident },
        };

        inner_impl.append_all(quote_spanned! { variant.span() =>
            Self::#ident #bra_captures_ket => {
                len += (#value as #repr).strict_encode(&mut e)?;
                #captures
                #field_impl
            }
        });
    }

    let import = encoding.use_crate;

    Ok(quote! {
        #[allow(unused_qualifications)]
        impl #impl_generics #import::StrictEncode for #ident_name #ty_generics #where_clause {
            #[inline]
            fn strict_encode<E: ::std::io::Write>(&self, mut e: E) -> Result<usize, #import::Error> {
                use #import::StrictEncode;
                let mut len = 0;
                match self {
                    #inner_impl
                }
                Ok(len)
            }
        }
    })
}

fn encode_fields_impl<'a>(
    fields: impl IntoIterator<Item = &'a Field>,
    parent_param: ParametrizedAttr,
    is_enum: bool,
) -> Result<TokenStream2> {
    let mut stream = TokenStream2::new();

    for (index, field) in fields.into_iter().enumerate() {
        let mut local_param = ParametrizedAttr::with(ATTR_NAME, &field.attrs)?;

        // First, test individual attribute
        let _ = EncodingDerive::try_from(&mut local_param, false, is_enum)?;
        // Second, combine global and local together
        let mut combined = parent_param.clone().merged(local_param)?;
        combined.args.remove("crate");
        let encoding = EncodingDerive::try_from(&mut combined, false, is_enum)?;

        if encoding.skip {
            continue;
        }

        let index = Index::from(index).to_token_stream();
        let name = if is_enum {
            index
        } else {
            field
                .ident
                .as_ref()
                .map(Ident::to_token_stream)
                .unwrap_or(index)
        };
        stream.append_all(quote_spanned! { field.span() =>
            len += data.#name.strict_encode(&mut e)?;
        })
    }

    Ok(stream)
}
