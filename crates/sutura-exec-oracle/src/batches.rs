//! An Oracle answer, read from the driver's Arrow batches: one batch per round trip.
//!
//! Each batch is charged against the materialisation budget before the next round trip is asked
//! for, and the deadline is checked before every one, so a result that will be refused is refused
//! at the batch that crosses the line. The driver hands a `NUMBER` with no fixed precision back as
//! its exact decimal text; a value of that text that parses as an `i64` is read as
//! [`Value::Integer`](sutura_domain::warehouse::Value::Integer) and every other stays exact
//! [`Value::Text`](sutura_domain::warehouse::Value::Text), the split the Postgres adapter draws
//! for `NUMERIC`. A `DATE` or `TIMESTAMP` arrives as a microsecond timestamp and is read as
//! its day. Every other type is the domain reader's: a `RAW` or a `BINARY_FLOAT` column is refused.

use std::sync::Arc;

use arrow_array::cast::AsArray as _;
use arrow_array::types::{Date32Type, TimestampMicrosecondType};
use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::{DataType, Field, Schema, SchemaRef, TimeUnit};
use sutura_domain::warehouse::{Accumulating, ResultBudget, RowSet, UnannouncedBatch, UnreadableCell, Value};

use crate::{OracleError, execute_err_mapped};

const MICROS_PER_DAY: i64 = 86_400_000_000;

/// Runs `sql` on `connection` and reads its answer - see the module header.
pub(crate) fn query(
    connection: &oracledb::Connection,
    sql: &str,
    params: &[&dyn oracledb::ToDbValue],
    stop: Stop,
    before_fetch: impl FnMut() -> Result<(), OracleError>,
) -> Result<RowSet, OracleError> {
    let sets = [params];
    let batches = connection
        .query_arrow_batches(sql, sets.as_slice())
        .map_err(execute_err_mapped)?;
    let schema = batches.schema();
    let numbers: Vec<usize> = batches
        .columns()
        .iter()
        .enumerate()
        .filter(|&(_, column)| column.db_type() == oracledb::DB_TYPE_NUMBER)
        .map(|(index, _)| index)
        .collect();
    read(batches, &schema, &numbers, stop, before_fetch)
}

/// Where a read stops: past the byte budget (refused), or at `most_rows` (the answer witness).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Stop {
    pub(crate) budget: ResultBudget,
    pub(crate) most_rows: Option<usize>,
}

/// Reads `batches`, announced under `announced`, into rows. `before_fetch` runs before each batch is
/// pulled; `numbers` are the columns that hold a `NUMBER`.
pub(crate) fn read(
    mut batches: impl Iterator<Item = Result<RecordBatch, oracledb::Error>>,
    announced: &SchemaRef,
    numbers: &[usize],
    stop: Stop,
    mut before_fetch: impl FnMut() -> Result<(), OracleError>,
) -> Result<RowSet, OracleError> {
    let schema = dated(announced);
    let mut accumulating = Accumulating::announcing(Arc::clone(&schema), usize::MAX, stop.budget);
    loop {
        let left = stop.most_rows.map(|most| most.saturating_sub(accumulating.delivered()));
        if left == Some(0) {
            break;
        }
        before_fetch()?;
        let Some(batch) = batches.next() else {
            break;
        };
        let mut batch = batch.map_err(execute_err_mapped)?;
        if let Some(left) = left {
            batch = batch.slice(0, batch.num_rows().min(left));
        }
        accumulating.push(dated_batch(&schema, &batch)?).map_err(unannounced)?;
    }
    let read = accumulating.finish().to_rows().map_err(unreadable)?;
    exact_numbers(read, numbers)
}

/// `schema` with every microsecond timestamp read as a day.
fn dated(schema: &SchemaRef) -> SchemaRef {
    let fields: Vec<Field> = schema
        .fields()
        .iter()
        .map(|field| {
            if is_timestamp(field.data_type()) {
                field.as_ref().clone().with_data_type(DataType::Date32)
            } else {
                field.as_ref().clone()
            }
        })
        .collect();
    Arc::new(Schema::new_with_metadata(fields, schema.metadata().clone()))
}

fn is_timestamp(data_type: &DataType) -> bool {
    *data_type == DataType::Timestamp(TimeUnit::Microsecond, None)
}

/// `batch` under `schema`, each timestamp column cast to the day it falls on.
fn dated_batch(schema: &SchemaRef, batch: &RecordBatch) -> Result<RecordBatch, OracleError> {
    let columns: Vec<ArrayRef> = batch
        .columns()
        .iter()
        .map(|column| {
            if is_timestamp(column.data_type()) {
                // An `i64` of microseconds divided by a day's is at most 106 751 991, inside `i32`.
                let days = column
                    .as_primitive::<TimestampMicrosecondType>()
                    .unary::<_, Date32Type>(|micros| i32::try_from(micros.div_euclid(MICROS_PER_DAY)).unwrap_or(i32::MAX));
                Arc::new(days) as ArrayRef
            } else {
                Arc::clone(column)
            }
        })
        .collect();
    RecordBatch::try_new(Arc::clone(schema), columns).map_err(|cause| OracleError::Batch { cause })
}

fn unannounced(cause: UnannouncedBatch) -> OracleError {
    match cause {
        UnannouncedBatch::OverBudget { most_bytes } => OracleError::OverBudget { most_bytes },
        cause @ (UnannouncedBatch::Width { .. } | UnannouncedBatch::Mislabelled { .. } | UnannouncedBatch::OverBound { .. }) => {
            OracleError::Unannounced { cause }
        }
    }
}

/// A cell the domain's reader refused, under this adapter's own variant for the value.
fn unreadable(cause: UnreadableCell) -> OracleError {
    match cause {
        UnreadableCell::NotFinite { column, cause } => OracleError::NotFinite { column, cause },
        UnreadableCell::NotADate { column, .. } => OracleError::UnsupportedType {
            column,
            oracle_type: "a date this build cannot represent",
        },
        UnreadableCell::UnsupportedType { column, .. } => OracleError::UnsupportedType {
            column,
            oracle_type: "a type with no exact domain value (RAW, BLOB or BINARY_FLOAT)",
        },
        UnreadableCell::Ragged(cause) => OracleError::Shape { cause },
        cause @ UnreadableCell::Downcast { .. } => OracleError::Unreadable { cause },
    }
}

/// Each `NUMBER` cell whose exact text parses as an `i64`, as that integer.
fn exact_numbers(read: RowSet, numbers: &[usize]) -> Result<RowSet, OracleError> {
    if numbers.is_empty() {
        return Ok(read);
    }
    let (columns, mut rows) = read.into_parts();
    for row in &mut rows {
        for &index in numbers {
            if let Some(cell) = row.get_mut(index)
                && let Value::Text(ref text) = *cell
                && let Ok(integer) = text.parse::<i64>()
            {
                *cell = Value::Integer(integer);
            }
        }
    }
    RowSet::new(columns, rows).map_err(|cause| OracleError::Shape { cause })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::Arc;

    use arrow_array::{ArrayRef, RecordBatch, StringArray, TimestampMicrosecondArray};
    use arrow_schema::{DataType, Field, Schema, SchemaRef, TimeUnit};
    use sutura_domain::warehouse::{ResultBudget, RowSet, Value};

    use super::{Stop, read};
    use crate::OracleError;

    fn text_batch(values: &[&str]) -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![Field::new("N", DataType::Utf8, true)]));
        let column: ArrayRef = Arc::new(StringArray::from(values.to_vec()));
        RecordBatch::try_new(schema, vec![column]).expect("one text column")
    }

    fn stop(bytes: usize, most_rows: Option<usize>) -> Stop {
        Stop {
            budget: ResultBudget::of_bytes(core::num::NonZeroUsize::new(bytes).expect("a test budget is positive")),
            most_rows,
        }
    }

    type Answer = Result<RowSet, OracleError>;

    /// Reads `batches` as the driver would hand them over, counting how many were pulled.
    fn read_counting(
        batches: Vec<RecordBatch>,
        numbers: &[usize],
        stop: Stop,
        before_fetch: impl FnMut() -> Result<(), OracleError>,
    ) -> (Answer, usize) {
        let schema: SchemaRef = batches.first().expect("a test has one batch").schema();
        let pulled = Cell::new(0);
        let fetched = batches.into_iter().inspect(|_| pulled.set(pulled.get() + 1)).map(Ok);
        let answer = read(fetched, &schema, numbers, stop, before_fetch);
        (answer, pulled.get())
    }

    #[test]
    fn a_result_over_the_byte_budget_is_refused_before_the_second_fetch() {
        let wide = "x".repeat(4096);
        let (answer, pulled) = read_counting(vec![text_batch(&[&wide]), text_batch(&["y"])], &[], stop(1024, None), || {
            Ok(())
        });
        assert!(
            matches!(answer, Err(OracleError::OverBudget { most_bytes: 1024 })),
            "{answer:?}"
        );
        assert_eq!(pulled, 1, "the second round trip was asked for after the budget was spent");
    }

    #[test]
    fn a_spent_deadline_between_fetches_stops_the_read() {
        let checks = Cell::new(0);
        let (answer, pulled) = read_counting(vec![text_batch(&["1"]), text_batch(&["2"])], &[], stop(1 << 20, None), || {
            checks.set(checks.get() + 1);
            if checks.get() > 1 {
                Err(OracleError::DeadlineSpent)
            } else {
                Ok(())
            }
        });
        assert!(matches!(answer, Err(OracleError::DeadlineSpent)), "{answer:?}");
        assert_eq!(pulled, 1);
    }

    #[test]
    fn a_read_stops_at_the_answer_witness() {
        let (answer, pulled) = read_counting(
            vec![text_batch(&["1", "2", "3"]), text_batch(&["4"])],
            &[0],
            stop(1 << 20, Some(2)),
            || Ok(()),
        );
        let rows = answer.expect("two rows fit");
        assert_eq!(rows.rows(), &[vec![Value::Integer(1)], vec![Value::Integer(2)]]);
        assert_eq!(pulled, 1);
    }

    #[test]
    fn a_number_of_seventeen_or_more_digits_stays_exact() {
        let wide = "12345678901234567890123456789012345678";
        let (answer, _) = read_counting(
            vec![text_batch(&["9007199254740993", "0.1", wide])],
            &[0],
            stop(1 << 20, None),
            || Ok(()),
        );
        let rows = answer.expect("the numbers read");
        assert_eq!(
            rows.rows(),
            &[
                // 2^53 + 1 = 9007199254740993, the first integer an `f64` cannot hold.
                vec![Value::Integer(0x0020_0000_0000_0001)],
                vec![Value::Text(String::from("0.1"))],
                vec![Value::Text(String::from(wide))],
            ]
        );
    }

    #[test]
    fn a_timestamp_reads_as_its_day() {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "D",
            DataType::Timestamp(TimeUnit::Microsecond, None),
            true,
        )]));
        // 2026-10-11T23:59:59.999999 and 1969-12-31T12:00:00.
        let micros = TimestampMicrosecondArray::from(vec![Some(1_791_763_199_999_999), Some(-43_200_000_000), None]);
        let batch = RecordBatch::try_new(schema, vec![Arc::new(micros)]).expect("one timestamp column");
        let (answer, _) = read_counting(vec![batch], &[], stop(1 << 20, None), || Ok(()));
        let rows = answer.expect("the days read");
        assert_eq!(
            rows.rows(),
            &[
                vec![Value::Text(String::from("2026-10-11"))],
                vec![Value::Text(String::from("1969-12-31"))],
                vec![Value::Null],
            ]
        );
    }
}
