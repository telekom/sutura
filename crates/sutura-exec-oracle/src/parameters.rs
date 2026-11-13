//! The native values bound to Oracle statements.

use sutura_domain::warehouse::ParamValue;

use crate::datetime::oracle_date;

/// `oracledb::ToDbValue`'s supertrait `ToBuf` lives in a private module of that crate, so no type
/// outside it may IMPLEMENT `ToDbValue` - measured by trying it and reading `E0277`. This enum
/// borrows the driver's OWN implementations (`String`, `OracleTimestamp`) instead of adding a third.
pub(crate) enum OracleParam {
    Text(String),
    Date(oracledb::OracleTimestamp),
}

impl OracleParam {
    pub(crate) fn as_dyn(&self) -> &dyn oracledb::ToDbValue {
        match *self {
            Self::Text(ref v) => v,
            Self::Date(ref d) => d,
        }
    }
}

pub(crate) fn bind(params: &[ParamValue]) -> Vec<OracleParam> {
    params
        .iter()
        .map(|param| match *param {
            ParamValue::Text(ref v) => OracleParam::Text(v.clone()),
            ParamValue::Date(d) => OracleParam::Date(oracle_date(d)),
        })
        .collect()
}
