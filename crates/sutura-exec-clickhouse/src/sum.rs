//! The generated sum, as the tuple `sutura_sql` renders it as.
//!
//! The integer-or-original sum (`sutura_sql::generate`'s `sum_for_clickhouse`) answers
//! `tuple(<type name>, <integer>, <decimal>, <float>)`. The type name is a per-query constant - `Int128`
//! for a widened integer sum, otherwise the server's own `toTypeName` of the sum - and names the one
//! slot that holds the total; the other two are `NULL`. A [`SumType`] is what the name says the total
//! is, and [`SumType::slot`] is where the adapter's batch normaliser takes it from.
//!
//! **A decimal's slot is `Decimal(76, 38)`, whatever the column's scale**, so [`at_scale`] puts the
//! column's scale back: `11.5000..` (38 digits) under `Decimal(38, 2)` is `11.50`. A column of a scale
//! above 38 would have lost digits in the server's conversion, so it is refused by name, as is every
//! other type this adapter does not read as a sum (`UInt128`, `Int256`).

use arrow_array::types::Decimal256Type;
use arrow_array::{Array, Decimal256Array};
use arrow_buffer::i256;
use arrow_schema::DataType;

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

/// Why a decimal slot could not be read at its column's scale.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SlotScale {
    /// The slot is not a `Decimal256` of at least the column's scale.
    NotADecimalSlot,
    /// A total has a non-zero digit past the column's scale, which putting the scale back would drop.
    PastScale,
}

/// The totals in a decimal `slot` at `scale` fractional digits. The slot is the exact sum widened to
/// its own scale, so the digits dropped are zeros; a non-zero one is refused rather than rounded.
pub(crate) fn at_scale(slot: &dyn Array, scale: usize) -> Result<Decimal256Array, SlotScale> {
    let DataType::Decimal256(precision, slot_scale) = *slot.data_type() else {
        return Err(SlotScale::NotADecimalSlot);
    };
    let dropped = u32::try_from(slot_scale)
        .ok()
        .zip(u32::try_from(scale).ok())
        .and_then(|(slot_scale, scale)| slot_scale.checked_sub(scale))
        .ok_or(SlotScale::NotADecimalSlot)?;
    let declared = i8::try_from(scale).map_err(|_cause| SlotScale::NotADecimalSlot)?;
    let factor = i256::from_i128(10).checked_pow(dropped).ok_or(SlotScale::NotADecimalSlot)?;
    let totals = slot
        .as_any()
        .downcast_ref::<Decimal256Array>()
        .ok_or(SlotScale::NotADecimalSlot)?;
    totals
        .try_unary::<_, Decimal256Type, _>(|total| match total.checked_rem(factor) {
            Some(rest) if rest == i256::ZERO => total.checked_div(factor).ok_or(SlotScale::PastScale),
            _ => Err(SlotScale::PastScale),
        })?
        .with_precision_and_scale(precision, declared)
        .map_err(|_cause| SlotScale::NotADecimalSlot)
}

#[cfg(test)]
mod tests;
