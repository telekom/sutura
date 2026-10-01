//! `NUMERIC` read the way the `tokio-postgres` path reads it.
//!
//! The pinned driver hands `NUMERIC` back as Arrow `Utf8` whatever the scale, tagged
//! `ADBC:postgresql:typname = numeric` (`postgres_type.h:246,358`), and writes the digits with
//! exactly `dscale` places after a point and `nan`/`inf`/`-inf` for the specials
//! (`copy/reader.h:299-412`). `crate::numeric::numeric_cell` answers a scale-zero value that fits an
//! `i64` as [`Value::Integer`], any other finite one as its exact text, and refuses the specials -
//! so text with no point that parses is the integer, the rest stays text, and the specials are the
//! same refusal. The cells in `super::parity` hold the pairing per case.
//!
//! **The limit**: only a column the driver TAGS `numeric` is read this way. A domain over `NUMERIC`
//! carries the domain's own name, so it stays text here, where `tokio-postgres` refuses it as a type
//! it does not map.

use sutura_domain::warehouse::arrow::of_row_set;
use sutura_domain::warehouse::{NotFinite, ResultBatches, RowSet, Value};

use super::AdbcError;
use crate::PostgresError;

/// The field metadata key the driver names a column's PostgreSQL type under.
const TYPNAME: &str = "ADBC:postgresql:typname";

/// The batches, with every `NUMERIC` cell read as [`rows`] reads it. Passed through untouched when
/// the result has no `NUMERIC` column, which is the common case and costs nothing.
pub(super) fn batches(batches: ResultBatches) -> Result<ResultBatches, PostgresError> {
    if numeric_columns(&batches).is_empty() {
        return Ok(batches);
    }
    of_row_set(&rows(&batches)?).map_err(|cause| PostgresError::Shape { cause })
}

/// The result as domain rows, every `NUMERIC` cell read as `tokio-postgres` reads it.
pub(super) fn rows(batches: &ResultBatches) -> Result<RowSet, PostgresError> {
    let numeric = numeric_columns(batches);
    let read = batches.to_rows().map_err(AdbcError::Unreadable)?;
    if numeric.is_empty() {
        return Ok(read);
    }
    let (columns, mut rows) = read.into_parts();
    for row in &mut rows {
        for &index in &numeric {
            if let (Some(cell), Some(label)) = (row.get_mut(index), columns.get(index)) {
                *cell = numeric_cell(core::mem::replace(cell, Value::Null), label)?;
            }
        }
    }
    RowSet::new(columns, rows).map_err(|cause| PostgresError::Shape { cause })
}

fn numeric_columns(batches: &ResultBatches) -> Vec<usize> {
    batches
        .schema()
        .fields()
        .iter()
        .enumerate()
        .filter(|(_, field)| field.metadata().get(TYPNAME).is_some_and(|name| name == "numeric"))
        .map(|(index, _)| index)
        .collect()
}

fn numeric_cell(cell: Value, label: &str) -> Result<Value, PostgresError> {
    let Value::Text(text) = cell else {
        return Ok(cell);
    };
    if matches!(text.as_str(), "nan" | "inf" | "-inf") {
        return Err(PostgresError::NotFinite {
            column: String::from(label),
            cause: NotFinite::NotANumber,
        });
    }
    Ok(text.parse::<i64>().map_or_else(|_| Value::Text(text), Value::Integer))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    use arrow_array::{Array as _, RecordBatch, StringArray};
    use arrow_schema::{Field, Schema};
    use sutura_domain::warehouse::Value;

    use crate::adbc::tests::{CEILING_MS, FakeConnection, open_for};

    #[test]
    fn a_fractional_or_wide_numeric_stays_exact_text_and_untagged_text_is_never_promoted() {
        let batch_of = |rows: Vec<&str>, numeric: bool| {
            let column = Arc::new(StringArray::from(rows));
            let mut field = Field::new("v", column.data_type().clone(), true);
            if numeric {
                field = field.with_metadata(HashMap::from([(
                    String::from("ADBC:postgresql:typname"),
                    String::from("numeric"),
                )]));
            }
            RecordBatch::try_new(Arc::new(Schema::new(vec![field])), vec![column]).expect("one column is a batch")
        };
        let mut tagged = FakeConnection::replying(batch_of(vec!["1.50", "123456789012345678901234567890"], true));
        let batches = crate::adbc::session::raw(&mut tagged, "SELECT 1", CEILING_MS, open_for(Duration::from_secs(60)))
            .expect("a raw stream reads");
        let rows = super::rows(&batches).expect("tagged numeric cells are read");
        assert_eq!(
            rows.rows(),
            [
                vec![Value::Text(String::from("1.50"))],
                vec![Value::Text(String::from("123456789012345678901234567890"))]
            ]
        );
        let mut untagged = FakeConnection::replying(batch_of(vec!["1"], false));
        let batches = crate::adbc::session::raw(&mut untagged, "SELECT 1", CEILING_MS, open_for(Duration::from_secs(60)))
            .expect("a raw stream reads");
        let rows = super::rows(&batches).expect("untagged text cells are read");
        assert_eq!(rows.rows(), [vec![Value::Text(String::from("1"))]]);
    }
}
