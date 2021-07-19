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
    ImplGenerics, Index, LitStr, Result, TypeGenerics, WhereClause,
};

use amplify::proc_attr::ParametrizedAttr;

use crate::param::EncodingDerive;
use crate::ATTR_NAME;

pub(crate) fn decode_derive(input: DeriveInput) -> Result<TokenStream2> {
    let (impl_generics, ty_generics, where_clause) =
        input.generics.split_for_impl();
    let ident_name = &input.ident;

    let global_param = ParametrizedAttr::with(ATTR_NAME, &input.attrs)?;

    match input.data {
        Data::Struct(data) => decode_struct_impl(
            data,
            ident_name,
            global_param,
            impl_generics,
            ty_generics,
            where_clause,
        ),
        Data::Enum(data) => decode_enum_impl(
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
            "Deriving StrictDecode is not supported in unions",
        )),
    }
}

fn decode_struct_impl(
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
            decode_fields_impl(&fields.named, global_param, false)?
        }
        Fields::Unnamed(ref fields) => {
            decode_fields_impl(&fields.unnamed, global_param, false)?
        }
        Fields::Unit => quote! {},
    };

    let import = encoding.use_crate;

    Ok(quote! {
        #[allow(unused_qualifications)]
        impl #impl_generics #import::StrictDecode for #ident_name #ty_generics #where_clause {
            #[inline]
            fn strict_decode<D: ::std::io::Read>(mut d: D) -> Result<Self, #import::Error> {
                use #import::StrictDecode;
                Ok(#ident_name { #inner_impl })
            }
        }
    })
}

fn decode_enum_impl(
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

        let field_impl = match variant.fields {
            Fields::Named(ref fields) => {
                decode_fields_impl(&fields.named, local_param, true)?
            }
            Fields::Unnamed(ref fields) => {
                decode_fields_impl(&fields.unnamed, local_param, true)?
            }
            Fields::Unit => TokenStream2::new(),
        };

        let ident = &variant.ident;
        let value = match (encoding.value, encoding.by_order) {
            (Some(val), _) => val.to_token_stream(),
            (None, true) => Index::from(order as usize).to_token_stream(),
            (None, false) => quote! { Self::#ident as #repr },
        };

        inner_impl.append_all(quote_spanned! { variant.span() =>
            x if x == #value => {
                Self::#ident {
                    #field_impl
                }
            }
        });
    }

    let import = encoding.use_crate;
    let enum_name = LitStr::new(&ident_name.to_string(), Span::call_site());

    Ok(quote! {
        #[allow(unused_qualifications)]
        impl #impl_generics #import::StrictDecode for #ident_name #ty_generics #where_clause {
            fn strict_decode<D: ::std::io::Read>(mut d: D) -> Result<Self, #import::Error> {
                use #import::StrictDecode;
                Ok(match #repr::strict_decode(&mut d)? {
                    #inner_impl
                    unknown => Err(#import::Error::EnumValueNotKnown(#enum_name, unknown as usize))?
                })
            }
        }
    })
}

fn decode_fields_impl<'a>(
    fields: impl IntoIterator<Item = &'a Field>,
    mut parent_param: ParametrizedAttr,
    is_enum: bool,
) -> Result<TokenStream2> {
    let mut stream = TokenStream2::new();

    parent_param.args.remove("crate");
    let parent_attr =
        EncodingDerive::try_from(&mut parent_param.clone(), false, is_enum)?;
    let import = parent_attr.use_crate;

    for (index, field) in fields.into_iter().enumerate() {
        let mut local_param = ParametrizedAttr::with(ATTR_NAME, &field.attrs)?;

        // First, test individual attribute
        let _ = EncodingDerive::try_from(&mut local_param, false, is_enum)?;
        // Second, combine global and local together
        let mut combined = parent_param.clone().merged(local_param)?;
        let encoding = EncodingDerive::try_from(&mut combined, false, is_enum)?;

        let name = field
            .ident
            .as_ref()
            .map(Ident::to_token_stream)
            .unwrap_or_else(|| Index::from(index).to_token_stream());

        if encoding.skip {
            stream.append_all(quote_spanned! { field.span() =>
                #name: Default::default(),
            });
        } else {
            stream.append_all(quote_spanned! { field.span() =>
                #name: #import::StrictDecode::strict_decode(&mut d)?,
            });
        }
    }

    Ok(stream)
}
