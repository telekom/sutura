//! A fixture CSV as one Arrow batch, typed the way [`super::declared_columns`] declared its table.
//!
//! The batch goes to the server through the driver's bulk ingest, so each column arrives as the
//! Arrow type the server converts into the declared one: `Int64`, `UInt64`, `Float64`, `Boolean`,
//! `Date32`, `Utf8`, and `Decimal128(38, scale)` for the conformance corpus's exact decimals.

use std::sync::Arc;

use arrow_array::{
    ArrayRef, BooleanArray, Date32Array, Decimal128Array, Float64Array, Int64Array, RecordBatch, StringArray, UInt64Array,
};
use arrow_schema::{ArrowError, Field, Schema};
use sutura_domain::calendar::Date;
use sutura_domain::warehouse::csv::{self, Column, FixtureType};

use super::{BatchError, Columns, nullable_columns};

/// The precision of every exact decimal column, which is the widest `ClickHouse` `Decimal128` holds.
const DECIMAL_PRECISION: u8 = 38;

/// The rows of `text` under its header, as one batch typed under `load`.
///
/// A column [`super::nullable_columns`] names has a nullable field, and an empty cell in it is
/// `NULL`. An empty cell anywhere else is refused earlier, by the load that declared no such column.
pub(super) fn of(text: &str, load: Columns) -> Result<RecordBatch, BatchError> {
    let columns = csv::infer(text).map_err(BatchError::Schema)?;
    let nullable = nullable_columns(text, columns.len(), load);
    let mut cells: Vec<Vec<&str>> = columns.iter().map(|_| Vec::new()).collect();
    for (index, line) in text.lines().enumerate().skip(1) {
        if line.is_empty() {
            return Err(BatchError::BlankRow {
                line: index.saturating_add(1),
            });
        }
        for (column, cell) in cells.iter_mut().zip(line.split(',')) {
            column.push(cell);
        }
    }
    let mut fields = Vec::with_capacity(columns.len());
    let mut arrays = Vec::with_capacity(columns.len());
    for (index, (column, cells)) in columns.iter().zip(&cells).enumerate() {
        let array = array(column, load, cells)?;
        fields.push(Field::new(
            column.name().as_str(),
            array.data_type().clone(),
            nullable.contains(&index),
        ));
        arrays.push(array);
    }
    RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays).map_err(BatchError::Assembly)
}

fn array(column: &Column, load: Columns, cells: &[&str]) -> Result<ArrayRef, BatchError> {
    Ok(match column.kind() {
        FixtureType::Boolean => Arc::new(BooleanArray::from(parsed(column, cells, |cell| {
            cell.to_ascii_lowercase().parse::<bool>().ok()
        })?)),
        FixtureType::Integer => Arc::new(Int64Array::from(parsed(column, cells, |cell| cell.parse::<i64>().ok())?)),
        FixtureType::WideInteger => Arc::new(UInt64Array::from(parsed(column, cells, |cell| cell.parse::<u64>().ok())?)),
        FixtureType::Decimal { scale } if load == Columns::Conformance => {
            let declared = i8::try_from(scale).map_err(|_cause| {
                BatchError::Assembly(ArrowError::InvalidArgumentError(format!(
                    "a decimal scale of {scale} does not fit a Decimal128"
                )))
            })?;
            let exact = Decimal128Array::from(parsed(column, cells, |cell| unscaled(cell, scale))?)
                .with_precision_and_scale(DECIMAL_PRECISION, declared)
                .map_err(BatchError::Assembly)?;
            Arc::new(exact)
        }
        FixtureType::Decimal { .. } | FixtureType::Real => {
            Arc::new(Float64Array::from(parsed(column, cells, |cell| cell.parse::<f64>().ok())?))
        }
        FixtureType::Date => Arc::new(Date32Array::from(parsed(column, cells, |cell| {
            Date::parse(cell).ok().map(Date::days_since_epoch)
        })?)),
        FixtureType::Text => Arc::new(StringArray::from(parsed(column, cells, |cell| Some(String::from(cell)))?)),
    })
}

/// A column's cells, each `None` where empty.
type Parsed<T> = Result<Vec<Option<T>>, BatchError>;

/// Each cell of `column` through `one`, an empty cell as `None`, and a cell `one` refuses as the
/// line it is on.
fn parsed<T>(column: &Column, cells: &[&str], one: impl Fn(&str) -> Option<T>) -> Parsed<T> {
    cells
        .iter()
        .enumerate()
        .map(|(row, cell)| {
            if cell.is_empty() {
                return Ok(None);
            }
            one(cell).map(Some).ok_or_else(|| BatchError::Cell {
                line: row.saturating_add(2),
                column: String::from(column.name().as_str()),
            })
        })
        .collect()
}

/// `cell` as the integer a `Decimal128` of `scale` holds: its digits with the point removed and
/// zeros appended up to the scale. `None` where it has more fractional digits than the scale.
fn unscaled(cell: &str, scale: u8) -> Option<i128> {
    let (negative, digits) = cell
        .strip_prefix('-')
        .map_or_else(|| (false, cell.strip_prefix('+').unwrap_or(cell)), |rest| (true, rest));
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    let fraction = fraction.trim_end_matches('0');
    let padding = usize::from(scale).checked_sub(fraction.len())?;
    let magnitude = format!("{whole}{fraction}{}", "0".repeat(padding)).parse::<i128>().ok()?;
    if negative { magnitude.checked_neg() } else { Some(magnitude) }
}

#[cfg(test)]
mod tests {
    use super::unscaled;

    #[test]
    fn a_decimal_is_its_digits_at_the_columns_scale() {
        assert_eq!(unscaled("12.5", 2), Some(1250));
        assert_eq!(unscaled("-0.07", 2), Some(-7));
        assert_eq!(unscaled("3", 2), Some(300));
        assert_eq!(unscaled("1.500", 1), Some(15));
        assert_eq!(unscaled("1.234", 2), None);
    }
}
