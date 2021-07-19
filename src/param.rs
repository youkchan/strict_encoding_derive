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

use proc_macro2::Span;
use std::convert::TryInto;
use syn::{Error, Ident, LitInt, Path, Result};

use amplify::proc_attr::{
    ArgValue, ArgValueReq, AttrReq, LiteralClass, ParametrizedAttr, ValueClass,
};

#[derive(Clone)]
pub(crate) struct EncodingDerive {
    pub use_crate: Path,
    pub skip: bool,
    pub by_order: bool,
    pub value: Option<LitInt>,
    pub repr: Ident,
}

impl EncodingDerive {
    pub(crate) fn try_from(
        attr: &mut ParametrizedAttr,
        is_global: bool,
        is_enum: bool,
    ) -> Result<EncodingDerive> {
        let mut map = if is_global {
            map! {
                "crate" => ArgValueReq::with_default(ident!(strict_encoding))
            }
        } else {
            map! {
                "skip" => ArgValueReq::Prohibited
            }
        };

        if is_enum {
            map.insert("by_order", ArgValueReq::Prohibited);
            map.insert("by_value", ArgValueReq::Prohibited);
            if is_global {
                map.insert("repr", ArgValueReq::with_default(ident!(u8)));
            } else {
                map.insert(
                    "value",
                    ArgValueReq::Optional(ValueClass::Literal(
                        LiteralClass::Int,
                    )),
                );
            }
        }

        attr.check(AttrReq::with(map))?;

        if attr.args.contains_key("by_value")
            && attr.args.contains_key("by_order")
        {
            return Err(Error::new(
                Span::call_site(),
                "`by_value` and `by_order` attributes can't be present together",
            ));
        }

        let repr: Ident = attr
            .args
            .get("repr")
            .cloned()
            .map(|arg| arg.try_into())
            .transpose()
            .expect(
                "amplify_syn is broken: attribute `repr` required to be Ident",
            )
            .unwrap_or_else(|| ident!(u8));

        match repr.to_string().as_str() {
            "u8" | "u16" | "u32" | "u64" => {}
            _ => {
                return Err(Error::new(
                    Span::call_site(),
                    "`repr` requires integer type identifier",
                ))
            }
        }

        let use_crate = attr
            .args
            .get("crate")
            .cloned()
            .unwrap_or_else(|| ArgValue::from(ident!(strict_encoding)))
            .try_into()
            .expect("amplify_syn is broken: requirements for crate arg are not satisfied");

        let value = attr
            .args
            .get("value")
            .map(|a| a.clone().try_into().expect("amplify_syn is broken: requirements for value arg are not satisfied"));

        let skip = attr.args.get("skip").is_some();

        let by_order = !attr.args.contains_key("by_value");

        Ok(EncodingDerive {
            use_crate,
            skip,
            by_order,
            value,
            repr,
        })
    }
}
