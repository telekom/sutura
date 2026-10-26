//! The generated sum, as the tuple `sutura_sql` renders it as.
//!
//! The integer-or-original sum (`sutura_sql::generate`'s `sum_for_clickhouse`) answers
//! `tuple(<type name>, <integer>, <decimal>, <float>)`. The type name is a per-query constant - `Int128`
//! for a widened integer sum, otherwise the server's own `toTypeName` of the sum - and names the one
//! slot that holds the total; the other two are `NULL`. A [`SumType`] is what the name says the total
//! is, and [`read`] takes the total from the slot the name points at.
//!
//! **A decimal's slot is `Decimal(76, 38)`, whatever the column's scale**, so the scale comes from the
//! type name and is put back: `3.000..` (38 digits) under `Decimal(38, 2)` is the exact text `3.00`. A
//! column of a scale above 38 would have lost digits in the server's conversion, so it is refused by
//! name, as is every other type this adapter does not read as a sum (`UInt128`, `Int256`).

use sutura_domain::warehouse::{NotFinite, Real, Value};

/// The `ClickHouse` type of the generated sum tuple, as the server names it in a result's header.
pub(crate) const TUPLE: &str = "Tuple(String, Nullable(Decimal(76, 0)), Nullable(Decimal(76, 38)), Nullable(Float64))";

/// The widest scale a sum's decimal slot keeps.
const DECIMAL_SLOT_SCALE: usize = 38;

/// What a sum's type name says its total is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SumType {
    /// A widened integer total: decimal digits that may not fit an `i64`.
    Integer,
    /// A float total, rendered `inf` or `nan` where it is not finite.
    Float,
    /// A decimal total with this many fractional digits.
    Decimal { scale: usize },
}

/// Why a sum could not be read.
#[derive(Debug, PartialEq)]
pub(crate) enum SumTextError {
    /// The tuple, its type name or its total is not what the generated sum answers.
    Malformed,
    /// A float total that is not a finite number.
    NotFinite(NotFinite),
}

impl SumType {
    /// The type a `toTypeName` answer names, or `None` for any type this adapter does not read as a sum.
    pub(crate) fn parse(name: &str) -> Option<Self> {
        let name = name
            .strip_prefix("Nullable(")
            .and_then(|rest| rest.strip_suffix(')'))
            .unwrap_or(name);
        match name {
            "Int128" => Some(Self::Integer),
            "Float32" | "Float64" => Some(Self::Float),
            _ => {
                let arguments = name.strip_prefix("Decimal(")?.strip_suffix(')')?;
                let (_precision, scale) = arguments.split_once(',')?;
                let scale = scale.trim().parse().ok().filter(|scale| *scale <= DECIMAL_SLOT_SCALE)?;
                Some(Self::Decimal { scale })
            }
        }
    }

    /// Where the total sits in the tuple, after the type name.
    pub(crate) const fn slot(self) -> usize {
        match self {
            Self::Integer => 1,
            Self::Decimal { .. } => 2,
            Self::Float => 3,
        }
    }

    /// The total as a domain value: an integer that fits an `i64` is [`Value::Integer`] and a wider one
    /// its exact digits; a float is [`Value::Real`]; a decimal is its exact text at its scale, widened to
    /// [`Value::Integer`] only where the scale is zero and the value fits an `i64`.
    fn value(self, text: &str) -> Result<Value, SumTextError> {
        match self {
            Self::Integer => {
                let digits = text.parse::<i128>().map_err(|_cause| SumTextError::Malformed)?;
                Ok(i64::try_from(digits).map_or_else(|_| Value::Text(digits.to_string()), Value::Integer))
            }
            Self::Float => {
                let number = text.parse::<f64>().map_err(|_cause| SumTextError::Malformed)?;
                Real::parse(number).map(Value::Real).map_err(SumTextError::NotFinite)
            }
            Self::Decimal { scale } => {
                let exact = at_scale(text, scale).ok_or(SumTextError::Malformed)?;
                if scale == 0 {
                    Ok(exact.parse::<i64>().map_or_else(|_cause| Value::Text(exact), Value::Integer))
                } else {
                    Ok(Value::Text(exact))
                }
            }
        }
    }
}

/// The sum a `Tuple(String, Nullable(Decimal(76, 0)), ..)` cell holds: `NULL` where the total is, a
/// domain value where it is not, and a refusal for a type name this adapter does not read as a sum.
pub(crate) fn read(cell: serde_json::Value) -> Result<Value, SumTextError> {
    let serde_json::Value::Array(slots) = cell else {
        return Err(SumTextError::Malformed);
    };
    let mut slots = slots.into_iter();
    let Some(serde_json::Value::String(name)) = slots.next() else {
        return Err(SumTextError::Malformed);
    };
    let sum_type = SumType::parse(&name).ok_or(SumTextError::Malformed)?;
    match slots.nth(sum_type.slot() - 1) {
        Some(serde_json::Value::Null) => Ok(Value::Null),
        Some(serde_json::Value::String(text)) => sum_type.value(&text),
        Some(serde_json::Value::Number(number)) => sum_type.value(&number.to_string()),
        _ => Err(SumTextError::Malformed),
    }
}

/// `text` with exactly `scale` fractional digits, or `None` where it is not a decimal that fits them.
/// A slot is written at its own, wider scale, so the digits past `scale` must be zeros to be dropped.
fn at_scale(text: &str, scale: usize) -> Option<String> {
    let (sign, unsigned) = text.strip_prefix('-').map_or(("", text), |rest| ("-", rest));
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    if whole.is_empty() || !digits(whole) || !digits(fraction) {
        return None;
    }
    let (kept, dropped) = fraction.split_at(fraction.len().min(scale));
    if dropped.bytes().any(|byte| byte != b'0') {
        return None;
    }
    let padding = "0".repeat(scale - kept.len());
    Some(if scale == 0 {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{kept}{padding}")
    })
}

#[cfg(test)]
mod tests;
