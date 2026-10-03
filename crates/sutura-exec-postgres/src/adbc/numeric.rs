//! `NUMERIC` read exactly, and the domain reader's refusals under this adapter's own variants.
//!
//! The pinned driver hands `NUMERIC` back as Arrow `Utf8` whatever the scale, tagged
//! `ADBC:postgresql:typname = numeric` (`postgres_type.h:246,358`), and writes the digits with
//! exactly `dscale` places after a point and `nan`/`inf`/`-inf` for the specials
//! (`copy/reader.h:299-412`). So text with no point that fits an `i64` is [`Value::Integer`], any
//! other finite value stays its exact text - no `f64` anywhere - and the specials are refused as
//! `NotFinite`. `tests/types.rs` holds each case through the real driver.
//!
//! A cell the domain's Arrow reader refuses is answered under this adapter's own variant for the
//! value (`refusal`): a non-finite `float8` as `NotFinite`, a `float4` or any other unmapped column
//! as `UnsupportedType`.
//!
//! **The limits**: only a column the driver TAGS `numeric` is read this way. A domain over `NUMERIC`
//! carries the domain's own name, so it stays text here; so does any other type the driver hands
//! back as text. `UnsupportedType`'s `postgres_type` here names no
//! type, because the Arrow column does not say which one it was. And a certified answer with no
//! `NUMERIC` column is handed on unread, so its `float4` or non-finite `float8` is refused by the
//! domain's reader downstream rather than as a `PostgresError`.

use std::sync::Arc;

use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::{FieldRef, Schema};
use sutura_domain::warehouse::arrow::arrow_column;
use sutura_domain::warehouse::{Accumulating, NotFinite, ResultBatches, ResultBudget, RowSet, UnreadableCell, Value};

use super::AdbcError;
use super::session::MOST_RESULT_BYTES;
use crate::PostgresError;

/// The field metadata key the driver names a column's PostgreSQL type under.
const TYPNAME: &str = "ADBC:postgresql:typname";

/// The batches, with every `NUMERIC` column replaced by its cells read as [`rows`] reads them -
/// `Int64` where every cell is an integer, exact text otherwise (`arrow_column`'s inference) - and
/// every OTHER column handed on as the driver typed it. Passed through untouched when the result
/// has no `NUMERIC` column, which is the common case and costs nothing.
///
/// **Only the `NUMERIC` columns are rebuilt**, because a federated combine joins two legs' buckets
/// by type: a leg whose `SUM(int8)` is `NUMERIC` must still carry its `DATE` bucket as `Date32`,
/// the type the other leg's arrives in (measured: rebuilding the whole batch from rows turned it
/// into text, and the combine refused `coalesce(Date32, Utf8)`).
pub(super) fn batches(batches: ResultBatches) -> Result<ResultBatches, PostgresError> {
    let numeric = numeric_columns(&batches);
    if numeric.is_empty() {
        return Ok(batches);
    }
    let read = rows(&batches)?;
    let rebuilt: Vec<(usize, ArrayRef)> = numeric
        .iter()
        .map(|&index| {
            let cells: Vec<Value> = read
                .rows()
                .iter()
                .map(|row| row.get(index).cloned().unwrap_or(Value::Null))
                .collect();
            (index, arrow_column(&cells).1)
        })
        .collect();
    let replaced = |index: usize| rebuilt.iter().find(|&&(at, _)| at == index).map(|(_, array)| array);
    let announced = batches.schema();
    let fields: Vec<FieldRef> = announced
        .fields()
        .iter()
        .enumerate()
        .map(|(index, field)| {
            replaced(index).map_or_else(
                || Arc::clone(field),
                |array| Arc::new(field.as_ref().clone().with_data_type(array.data_type().clone())),
            )
        })
        .collect();
    let schema = Arc::new(Schema::new_with_metadata(fields, announced.metadata().clone()));
    let mut accumulating = Accumulating::announcing(Arc::clone(&schema), usize::MAX, ResultBudget::of_bytes(MOST_RESULT_BYTES));
    let mut offset = 0;
    for batch in batches.batches() {
        let length = batch.num_rows();
        let columns = (0..batch.num_columns())
            .map(|index| replaced(index).map_or_else(|| Arc::clone(batch.column(index)), |array| array.slice(offset, length)))
            .collect();
        let batch = RecordBatch::try_new(Arc::clone(&schema), columns).map_err(AdbcError::Batch)?;
        accumulating.push(batch).map_err(AdbcError::Unannounced)?;
        offset += length;
    }
    Ok(accumulating.finish())
}

/// The result as domain rows, every `NUMERIC` cell read exactly.
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

/// A cell the domain's reader refused, under this adapter's own variant for the value. A
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
    fn a_numeric_column_is_rebuilt_and_every_other_column_keeps_the_drivers_type() {
        use arrow_array::Date32Array;

        let numeric = batch_of(vec!["3"], true);
        let day = Arc::new(Date32Array::from(vec![20_605])) as ArrayRef;
        let fields = vec![
            Field::new("period", day.data_type().clone(), true),
            numeric.schema().field(0).clone(),
        ];
        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), vec![day, Arc::clone(numeric.column(0))])
            .expect("two columns are a batch");
        let read = super::batches(answered(batch)).expect("a tagged column is re-read");
        let schema = read.schema();
        assert_eq!(schema.field(0).data_type(), &arrow_schema::DataType::Date32, "{schema:?}");
        assert_eq!(schema.field(1).data_type(), &arrow_schema::DataType::Int64, "{schema:?}");
    }

    #[test]
    fn every_numeric_special_is_refused_as_not_finite() {
        for special in ["nan", "inf", "-inf"] {
            let refused = super::numeric_cell(Value::Text(String::from(special)), "v").expect_err(special);
            assert!(
                matches!(refused, PostgresError::NotFinite { ref column, cause: NotFinite::NotANumber } if column == "v"),
                "{special}: {refused:?}"
            );
        }
    }

    #[test]
    fn a_cell_the_domain_reader_refuses_is_the_adapters_own_variant() {
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
