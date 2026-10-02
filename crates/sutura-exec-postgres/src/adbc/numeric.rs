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
//! A cell the domain's Arrow reader refuses is answered under the `tokio-postgres` path's variant
//! for the same value (`refusal`): a non-finite `float8` as `NotFinite`, a `float4` or any other
//! unmapped column as `UnsupportedType`.
//!
//! **The limits**: only a column the driver TAGS `numeric` is read this way. A domain over `NUMERIC`
//! carries the domain's own name, so it stays text here, where `tokio-postgres` refuses it as a type
//! it does not map; so does any type the driver hands back as text that `tokio-postgres` does not
//! map, and no case names one. `UnsupportedType`'s `postgres_type` here names no
//! type, because the Arrow column does not say which one it was. And a certified answer with no
//! `NUMERIC` column is handed on unread, so its `float4` or non-finite `float8` is refused by the
//! domain's reader downstream rather than as a `PostgresError`.

use sutura_domain::warehouse::arrow::of_row_set;
use sutura_domain::warehouse::{NotFinite, ResultBatches, RowSet, UnreadableCell, Value};

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
    let read = batches.to_rows().map_err(refusal)?;
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

/// A cell the domain's reader refused, as the `tokio-postgres` path refuses the same value. A
/// column that does not downcast to its own schema's type is the driver's defect, and stays one.
fn refusal(cause: UnreadableCell) -> PostgresError {
    match cause {
        UnreadableCell::NotFinite { column, cause } => PostgresError::NotFinite { column, cause },
        UnreadableCell::NotADate { column, cause } => PostgresError::NotADate { column, cause },
        UnreadableCell::UnsupportedType { column, .. } => PostgresError::UnsupportedType {
            column,
            postgres_type: "a type this adapter does not map",
        },
        UnreadableCell::Ragged(cause) => PostgresError::Shape { cause },
        cause @ UnreadableCell::Downcast { .. } => AdbcError::Unreadable(cause).into(),
    }
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

    use arrow_array::{Array as _, ArrayRef, Float32Array, Float64Array, RecordBatch, StringArray};
    use arrow_schema::{Field, Schema};
    use sutura_domain::warehouse::{NotFinite, ResultBatches, Value};

    use crate::PostgresError;
    use crate::adbc::tests::{CEILING_MS, FakeConnection, one_column, open_for};

    fn batch_of(rows: Vec<&str>, numeric: bool) -> RecordBatch {
        let column = Arc::new(StringArray::from(rows));
        let mut field = Field::new("v", column.data_type().clone(), true);
        if numeric {
            field = field.with_metadata(HashMap::from([(
                String::from("ADBC:postgresql:typname"),
                String::from("numeric"),
            )]));
        }
        RecordBatch::try_new(Arc::new(Schema::new(vec![field])), vec![column]).expect("one column is a batch")
    }

    /// `batch` as the certified path's stream hands it on.
    fn answered(batch: RecordBatch) -> ResultBatches {
        let mut connection = FakeConnection::replying(batch);
        crate::adbc::session::answer(
            &mut connection,
            "SELECT 1",
            None,
            CEILING_MS,
            open_for(Duration::from_secs(60)),
        )
        .expect("a certified stream reads")
    }

    #[test]
    fn the_certified_answer_re_reads_a_tagged_numeric_column_and_hands_any_other_on() {
        let tagged = super::batches(answered(batch_of(vec!["1"], true))).expect("a tagged column is re-read");
        assert_eq!(
            tagged.to_rows().expect("the re-read batch reads").rows(),
            [vec![Value::Integer(1)]]
        );
        let untagged = super::batches(answered(batch_of(vec!["1"], false))).expect("an untagged column is handed on");
        assert_eq!(
            untagged.to_rows().expect("the batch reads").rows(),
            [vec![Value::Text(String::from("1"))]]
        );
    }

    #[test]
    fn every_numeric_special_is_the_tokio_postgres_paths_refusal() {
        for special in ["nan", "inf", "-inf"] {
            let refused = super::numeric_cell(Value::Text(String::from(special)), "v").expect_err(special);
            assert!(
                matches!(refused, PostgresError::NotFinite { ref column, cause: NotFinite::NotANumber } if column == "v"),
                "{special}: {refused:?}"
            );
        }
    }

    #[test]
    fn a_cell_the_domain_reader_refuses_is_the_tokio_postgres_paths_variant() {
        let read = |column: ArrayRef| super::rows(&answered(one_column("v", column)));
        let float4 = read(Arc::new(Float32Array::from(vec![1.5_f32]))).expect_err("a float4 is not mapped");
        assert!(
            matches!(float4, PostgresError::UnsupportedType { ref column, .. } if column == "v"),
            "{float4:?}"
        );
        let nan = read(Arc::new(Float64Array::from(vec![f64::NAN]))).expect_err("a NaN float8 is refused");
        assert!(
            matches!(nan, PostgresError::NotFinite { ref column, .. } if column == "v"),
            "{nan:?}"
        );
    }

    #[test]
    fn a_fractional_or_wide_numeric_stays_exact_text_and_untagged_text_is_never_promoted() {
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
