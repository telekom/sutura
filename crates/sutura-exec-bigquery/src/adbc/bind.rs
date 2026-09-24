//! The plan's values, as the one Arrow batch this driver binds them from.
//!
//! **Every question this tool surface can ask carries parameters**, so a transport that could not
//! bind them could not answer anything: `sutura_domain::query::Question` makes its `range` a
//! mandatory field, `sutura_sql::Dialect::BigQuery` renders `PlaceholderStyle::Question`, and the
//! adapter's own cell asserts the canonical plan comes out as two `?` and two values. A refusal
//! here was therefore not a narrow gap - it was the whole surface.
//!
//! # The shape, read off the pinned driver rather than assumed
//!
//! `adbc_core::Statement::bind` takes ONE [`RecordBatch`]. In the pinned `go/v1.13.0` driver,
//! `statement.Bind` wraps it in a reader and `record_reader.go`'s `newRecordReader` then loops
//! `boundParameters.Next()`, and inside each batch `queryRecordWithSchemaCallback` loops
//! `for i := range int(rec.NumRows())` - **running the whole query once per ROW** and appending each
//! result to the same stream.
//!
//! **So a batch of two rows is two executions of one question, concatenated into one answer.** That
//! is a wrong number under a certified metric name, which is why [`ONE_QUESTION`] is a named
//! constant, why [`parameter_batch`] builds exactly that many rows, and why
//! `a_parameter_batch_carries_exactly_one_row` exists rather than a sentence saying so.
//!
//! Column NAMES are not the contract and the driver says so: `getQueryParameter` attaches
//! `values.ColumnName(i)` only when the parameter mode is `named`, and this driver's statements
//! default to `positional` (`go/connection.go`'s `NewStatement`). Arrow requires a field name
//! regardless, so the ordinal is written into one - a reader of a captured batch sees the position
//! rather than an invented column.
//!
//! # Which Arrow type each value becomes, and why that decides the `GoogleSQL` type
//!
//! The driver maps the Arrow type to a `StandardSQLDataType` before it sends anything
//! (`arrowDataTypeToTypeKind`), so the Arrow type chosen HERE is what `BigQuery` type-checks the
//! query against:
//!
//! | `ParamValue` | Arrow | `GoogleSQL` |
//! | --- | --- | --- |
//! | `Text` | `Utf8` | `STRING` |
//! | `Date` | `Date32` | `DATE` |
//!
//! `Date32` and not a string, which is the one choice with a wrong alternative: a date sent as
//! `STRING` compares against a `DATE` column only through an implicit coercion `GoogleSQL` does not
//! perform, so a range filter would either fail the query or - worse, on a string column - compare
//! lexically. The domain's [`Date`](sutura_domain::calendar::Date) already holds days since the
//! epoch, which is exactly `Date32`'s encoding, so no formatting happens on this path at all.

use std::sync::Arc;

use arrow_array::{ArrayRef, Date32Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use sutura_domain::warehouse::ParamValue;

use super::AdbcError;

/// How many rows one question's parameters occupy.
///
/// **One, and the driver is why it is a constant rather than a literal.** See this module's header:
/// the driver executes the query once per row of the bound batch, so any other value here answers a
/// question more than once and concatenates the results.
const ONE_QUESTION: usize = 1;

/// The Arrow field name a parameter at `position` is carried under.
///
/// Positional parameters have no names - the driver reads the column name only in `named` mode - so
/// this exists for Arrow's own requirement and for whoever reads a captured batch. The ordinal is
/// the only honest thing to put in it, because a parameter's identity in a plan IS its position
/// (`crate::transport::ParameterMode`).
fn field_name(position: usize) -> String {
    format!("p{position}")
}

/// One question's values, as the batch this driver binds from - or `None` where there are none.
///
/// **`None` rather than an empty batch, and the driver decides that too:** `newRecordReader` takes
/// the `runPlainQuery` path when nothing is bound, and an empty-schema batch with one row would
/// instead take the parameter path and set `query.Parameters` to an empty slice. The two are not the
/// same request. Every boot-path call - `verify_anchor`, the identity read, a fixture load - carries
/// no values and must take the plain path.
///
/// # Errors
///
/// [`AdbcError::Parameters`] where Arrow refuses the batch. Unreachable as written - the schema and
/// the columns are built from one walk of one slice, so their lengths and types agree by
/// construction - and answered for rather than unwrapped, because `unwrap_used` is denied and a
/// panic here would be process death under `panic = "abort"`.
pub(super) fn parameter_batch(params: &[ParamValue]) -> Result<Option<RecordBatch>, AdbcError> {
    if params.is_empty() {
        return Ok(None);
    }
    let mut fields = Vec::with_capacity(params.len());
    let mut columns: Vec<ArrayRef> = Vec::with_capacity(params.len());
    for (position, value) in params.iter().enumerate() {
        // One exhaustive match with no wildcard, so a third `ParamValue` is a compile error at this
        // line rather than a value silently rendered as text.
        let (kind, column): (DataType, ArrayRef) = match *value {
            ParamValue::Text(ref text) => (DataType::Utf8, Arc::new(StringArray::from(vec![text.as_str(); ONE_QUESTION]))),
            ParamValue::Date(date) => (
                DataType::Date32,
                Arc::new(Date32Array::from(vec![date.days_since_epoch(); ONE_QUESTION])),
            ),
        };
        // Not nullable: a plan's parameter list carries values, never absences - `ParamValue` has no
        // null shape - so a nullable field would describe a column this path cannot produce.
        fields.push(Field::new(field_name(position), kind, false));
        columns.push(column);
    }
    RecordBatch::try_new(Arc::new(Schema::new(fields)), columns)
        .map(Some)
        .map_err(|cause| AdbcError::Parameters { cause })
}

#[cfg(test)]
mod tests {
    use arrow_array::Array as _;
    use sutura_domain::calendar::Date;
    use sutura_domain::warehouse::ParamValue;

    use super::{ONE_QUESTION, parameter_batch};

    fn day(iso: &str) -> Date {
        Date::parse(iso).expect("a test date is a date")
    }

    /// The values of the canonical two-parameter plan: a range's two ends.
    fn a_range() -> Vec<ParamValue> {
        vec![ParamValue::Date(day("2026-06-01")), ParamValue::Date(day("2026-07-01"))]
    }

    #[test]
    fn a_question_with_no_values_binds_nothing_at_all() {
        // The boot path, and `None` is not an empty batch: the driver takes a different code path
        // for each, and an empty-schema batch would send an empty parameter list rather than none.
        assert!(parameter_batch(&[]).expect("no values is not a failure").is_none());
    }

    #[test]
    fn a_parameter_batch_carries_exactly_one_row() {
        // **The property that stops one question being answered twice.** The pinned driver runs the
        // query once per ROW of the bound batch and concatenates the results, so a second row is a
        // doubled answer under a certified metric name - not an error anything would report.
        let batch = parameter_batch(&a_range())
            .expect("a range is bindable")
            .expect("a range has values");
        assert_eq!(batch.num_rows(), ONE_QUESTION);
        assert_eq!(batch.num_rows(), 1);
        for column in batch.columns() {
            assert_eq!(column.len(), 1);
        }
    }

    #[test]
    fn the_values_arrive_in_the_order_the_statements_placeholders_take_them() {
        // **The order IS the contract** - `ParameterMode::Positional` means the nth value pairs with
        // the nth `?`, and the driver attaches no names in that mode. A transport that reordered
        // this would send a different query and get a plausible wrong answer.
        let batch = parameter_batch(&a_range())
            .expect("a range is bindable")
            .expect("a range has values");
        assert_eq!(batch.num_columns(), 2);
        assert_eq!(
            batch
                .schema()
                .fields()
                .iter()
                .map(|field| field.name().clone())
                .collect::<Vec<String>>(),
            vec![String::from("p0"), String::from("p1")]
        );
        let ends: Vec<i32> = batch
            .columns()
            .iter()
            .map(|column| {
                column
                    .as_any()
                    .downcast_ref::<arrow_array::Date32Array>()
                    .expect("a date parameter is a Date32")
                    .value(0)
            })
            .collect();
        assert_eq!(
            ends,
            vec![day("2026-06-01").days_since_epoch(), day("2026-07-01").days_since_epoch()]
        );
    }

    #[test]
    fn a_date_is_bound_as_a_date_and_never_as_text() {
        // **The one type choice with a wrong alternative.** The driver derives the `GoogleSQL` type
        // from the ARROW type, so a date sent as `Utf8` arrives as `STRING` - which against a `DATE`
        // column either fails the query or, on a text column, compares lexically. Neither is an
        // answer. `Date32` is also the domain's own encoding, so nothing is formatted on this path.
        let batch = parameter_batch(&[ParamValue::Date(day("2026-06-01"))])
            .expect("a date is bindable")
            .expect("a date is a value");
        assert_eq!(batch.schema().field(0).data_type(), &arrow_schema::DataType::Date32);
        assert!(!batch.schema().field(0).is_nullable());
    }

    #[test]
    fn text_is_bound_as_text_and_carries_the_exact_bytes_it_was_given() {
        // A key filter's value, and it travels in a COLUMN rather than in the statement - which is
        // the no-injection property at the one boundary this adapter could break it at. A quote and
        // a backslash are carried, not escaped, because nothing here is rendering SQL.
        let hostile = String::from("north'; DROP TABLE x --\\");
        let batch = parameter_batch(&[ParamValue::Text(hostile.clone())])
            .expect("text is bindable")
            .expect("text is a value");
        assert_eq!(batch.schema().field(0).data_type(), &arrow_schema::DataType::Utf8);
        assert_eq!(
            batch
                .column(0)
                .as_any()
                .downcast_ref::<arrow_array::StringArray>()
                .expect("a text parameter is a Utf8 array")
                .value(0),
            hostile
        );
    }

    #[test]
    fn a_mixed_parameter_list_keeps_one_type_per_position() {
        // Two kinds in one question - a key filter beside a range - because a transport that bound
        // the first value's type for every column would send a query BigQuery type-checks against
        // the wrong types, and the two-date case above could not see it.
        let batch = parameter_batch(&[ParamValue::Text(String::from("north")), ParamValue::Date(day("2026-06-01"))])
            .expect("a mixed list is bindable")
            .expect("a mixed list has values");
        assert_eq!(
            batch
                .schema()
                .fields()
                .iter()
                .map(|field| field.data_type().clone())
                .collect::<Vec<arrow_schema::DataType>>(),
            vec![arrow_schema::DataType::Utf8, arrow_schema::DataType::Date32]
        );
    }
}
