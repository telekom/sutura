//! The generated sum, as the tuple `sutura_sql` renders it as.
//!
//! The integer-or-original sum (`sutura_sql::generate`'s `sum_for_clickhouse`) answers
//! `tuple(<type name>, <integer>, <decimal>, <float>)`. The type name is a per-query constant - `Int128`
//! for a widened integer sum, otherwise the server's own `toTypeName` of the sum - and names the one
//! slot that holds the total; the other two are `NULL`. A [`SumType`] is what the name says the total
//! is, and [`SumType::slot`] is where the adapter's batch normaliser takes it from.
//!
//! **A decimal's slot is `Decimal(76, 38)`, whatever the column's scale.** A column of a scale above
//! 38 would have lost digits in the server's conversion, so it is refused by name, as is every other
//! type this adapter does not read as a sum (`UInt128`, `Int256`). **The limit:** the slot is read as
//! the driver typed it, so the column's own scale is parsed here and not applied - the exact text a
//! decimal total answers on this path carries the slot's 38 fractional digits.

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
}

#[cfg(test)]
mod tests;
