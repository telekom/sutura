//! An Oracle answer, read from the driver's Arrow batches: one batch per round trip.
//!
//! Each batch is charged against the materialisation budget before the next round trip is asked
//! for, and the deadline is checked before every one, so a result that will be refused is refused
//! at the batch that crosses the line. The driver hands a `NUMBER` with no fixed precision back as
//! its exact decimal text; a value of that text that parses as an `i64` is read as
//! [`Value::Integer`](sutura_domain::warehouse::Value::Integer) and every other stays exact
//! [`Value::Text`](sutura_domain::warehouse::Value::Text), the split the Postgres adapter draws
//! for `NUMERIC`. A `DATE` or `TIMESTAMP` arrives as a microsecond timestamp, a `TIMESTAMP WITH
//! TIME ZONE` as that timestamp in UTC beside the value's offset, and each is read as its date-time
//! text; a `TIMESTAMP WITH LOCAL TIME ZONE` column is refused by name. Every other type is the
//! domain reader's: a `RAW` or a `BINARY_FLOAT` column is refused.

use std::sync::Arc;

use arrow_array::cast::AsArray as _;
use arrow_array::types::{Int16Type, TimestampMicrosecondType};
use arrow_array::{Array as _, ArrayRef, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema, SchemaRef, TimeUnit};
use sutura_domain::warehouse::{Accumulating, ResultBudget, RowSet, UnannouncedBatch, UnreadableCell, Value};

use crate::datetime::{date_text, local_zone_refused};
use crate::{OracleError, execute_err_mapped};

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
    let numbers = number_columns(batches.columns().iter().map(|column| (column.name(), column.db_type())))?;
    let schema = batches.schema();
    read(batches, &schema, &numbers, stop, before_fetch)
}

/// The indexes of the `NUMBER` columns among `columns`, each a name and its type; a `TIMESTAMP
/// WITH LOCAL TIME ZONE` column is refused by name.
fn number_columns<'a>(columns: impl IntoIterator<Item = (&'a str, &'a oracledb::DbType)>) -> Result<Vec<usize>, OracleError> {
    let mut numbers = Vec::new();
    for (index, (name, db_type)) in columns.into_iter().enumerate() {
        if db_type == oracledb::DB_TYPE_TIMESTAMP_LTZ {
            return Err(local_zone_refused(name));
        }
        if db_type == oracledb::DB_TYPE_NUMBER {
            numbers.push(index);
        }
    }
    Ok(numbers)
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

/// `schema` with every date-typed column read as its text.
fn dated(schema: &SchemaRef) -> SchemaRef {
    let fields: Vec<Field> = schema
        .fields()
        .iter()
        .map(|field| {
            if is_timestamp(field.data_type()) || is_zoned(field.data_type()) {
                field.as_ref().clone().with_data_type(DataType::Utf8)
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

/// A `TIMESTAMP WITH TIME ZONE` column as the driver hands it: the UTC instant beside the offset.
fn is_zoned(data_type: &DataType) -> bool {
    let DataType::Struct(fields) = data_type else {
        return false;
    };
    let mut types = fields.iter().map(|field| field.data_type());
    matches!(
        (types.next(), types.next(), types.next()),
        (
            Some(DataType::Timestamp(TimeUnit::Microsecond, _)),
            Some(DataType::Int16),
            None
        )
    )
}

/// `batch` under `schema`, each date-typed column as its text.
fn dated_batch(schema: &SchemaRef, batch: &RecordBatch) -> Result<RecordBatch, OracleError> {
    let columns = batch
        .columns()
        .iter()
        .map(|column| {
            let texts: StringArray = if is_timestamp(column.data_type()) {
                column
                    .as_primitive::<TimestampMicrosecondType>()
                    .iter()
                    .map(|micros| micros.map(|micros| date_text(micros, None)).transpose())
                    .collect::<Result<_, _>>()?
            } else if is_zoned(column.data_type()) {
                let parts = column.as_struct();
                let instants = parts.column(0).as_primitive::<TimestampMicrosecondType>();
                let offsets = parts.column(1).as_primitive::<Int16Type>();
                (0..parts.len())
                    .map(|row| {
                        parts
                            .is_valid(row)
                            .then(|| date_text(instants.value(row), Some(offsets.value(row))))
                            .transpose()
                    })
                    .collect::<Result<_, _>>()?
            } else {
                return Ok(Arc::clone(column));
            };
            Ok(Arc::new(texts) as ArrayRef)
        })
        .collect::<Result<Vec<_>, OracleError>>()?;
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

    use arrow_array::{Array as _, ArrayRef, Int16Array, RecordBatch, StringArray, StructArray, TimestampMicrosecondArray};
    use arrow_schema::{DataType, Field, Fields, Schema, SchemaRef, TimeUnit};
    use sutura_domain::warehouse::{ResultBudget, RowSet, Value};

    use super::{Stop, number_columns, read};
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
    fn every_batch_of_an_answer_is_read() {
        let (answer, pulled) = read_counting(
            vec![text_batch(&["1", "2"]), text_batch(&["3"])],
            &[],
            stop(1 << 20, None),
            || Ok(()),
        );
        let rows = answer.expect("three rows fit");
        assert_eq!(rows.rows(), &[text("1"), text("2"), text("3")]);
        assert_eq!(pulled, 2);
    }

    #[test]
    fn only_a_number_column_is_read_as_an_integer() {
        let columns = [
            ("N", oracledb::DB_TYPE_NUMBER),
            ("CODE", oracledb::DB_TYPE_VARCHAR),
            ("M", oracledb::DB_TYPE_NUMBER),
        ];
        let numbers = number_columns(columns).expect("no column is refused");
        assert_eq!(numbers, [0, 2], "a VARCHAR2 holding '007' would read as 7");
    }

    #[test]
    fn a_local_time_zone_column_is_refused_by_name() {
        let columns = [("N", oracledb::DB_TYPE_NUMBER), ("SEEN", oracledb::DB_TYPE_TIMESTAMP_LTZ)];
        let refused = number_columns(columns);
        assert!(
            matches!(&refused, Err(OracleError::UnsupportedType { column, .. }) if column == "SEEN"),
            "{refused:?}"
        );
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

    fn timestamp_batch(micros: Vec<Option<i64>>) -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "D",
            DataType::Timestamp(TimeUnit::Microsecond, None),
            true,
        )]));
        let column: ArrayRef = Arc::new(TimestampMicrosecondArray::from(micros));
        RecordBatch::try_new(schema, vec![column]).expect("one timestamp column")
    }

    fn texts(batch: RecordBatch) -> Vec<Vec<Value>> {
        let (answer, _) = read_counting(vec![batch], &[], stop(1 << 20, None), || Ok(()));
        answer.expect("the dates read").rows().to_vec()
    }

    fn text(raw: &str) -> Vec<Value> {
        vec![Value::Text(String::from(raw))]
    }

    #[test]
    fn a_date_with_a_time_of_day_keeps_it_in_its_batch() {
        // 2026-10-11T13:45:30.
        let rows = texts(timestamp_batch(vec![Some(1_791_726_330_000_000), None]));
        assert_eq!(rows, [text("2026-10-11T13:45:30"), vec![Value::Null]]);
    }

    #[test]
    fn only_midnight_with_no_fraction_reads_as_the_day() {
        // 2026-10-11T00:00:00 and one microsecond past it.
        let rows = texts(timestamp_batch(vec![
            Some(1_791_676_800_000_000),
            Some(1_791_676_800_000_001),
        ]));
        assert_eq!(rows, [text("2026-10-11"), text("2026-10-11T00:00:00.000001")]);
    }

    #[test]
    fn a_time_of_day_before_1970_lands_on_its_day() {
        let rows = texts(timestamp_batch(vec![Some(-43_200_000_000), Some(-1)]));
        assert_eq!(rows, [text("1969-12-31T12:00:00"), text("1969-12-31T23:59:59.999999")]);
    }

    #[test]
    fn a_zoned_timestamp_keeps_its_own_offset() {
        let instants = TimestampMicrosecondArray::from(vec![Some(1_761_933_135_456_000), None]).with_timezone("UTC");
        let fields = Fields::from(vec![
            Field::new("timestamp", instants.data_type().clone(), true),
            Field::new("offset_minutes", DataType::Int16, true),
        ]);
        let offsets = Int16Array::from(vec![Some(-360), None]);
        let nulls = offsets.nulls().cloned();
        let zoned = StructArray::new(fields, vec![Arc::new(instants), Arc::new(offsets)], nulls);
        let schema = Arc::new(Schema::new(vec![Field::new("T", zoned.data_type().clone(), true)]));
        let batch = RecordBatch::try_new(schema, vec![Arc::new(zoned)]).expect("one zoned column");
        // The driver's reading of `2025-10-31 11:52:15.456 -06:00`: the UTC instant beside the offset.
        assert_eq!(texts(batch), [text("2025-10-31T11:52:15.456-06:00"), vec![Value::Null]]);
    }
}
