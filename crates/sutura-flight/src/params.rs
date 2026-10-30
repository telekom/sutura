//! Decode the one parameter row a `DoExchange` request carries.
//!
//! **Column position is the identity.** The batch carries no certified schema yet - certification
//! happens after [`crate::receive_request`] hands the raw [`LegWire`](sutura_domain::plan::LegWire)
//! and this batch to `LegWire::parse` - so a field NAME here is peer-controlled text with nothing to
//! check it against. [`decode`] never reads one.
//!
//! **Closed refusals.** Every [`ParamsError`] names a column POSITION and, where relevant, which
//! requirement failed - never the value, the field name or the Arrow type a peer sent, any of which
//! could be read back from a log line or an error a caller sees.

use arrow_array::RecordBatch;
use arrow_array::cast::AsArray as _;
use arrow_array::types::Date32Type;
use arrow_schema::DataType;
use sutura_domain::calendar::Date;
use sutura_domain::warehouse::ParamValue;

/// Why a parameter batch could not become typed parameter values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ParamsError {
    /// The protocol carries exactly one parameter row - a lookup leg with no range parameters still
    /// sends one row with zero columns, never zero rows.
    #[error("the remote leg parameter batch must carry exactly one row")]
    RowCount,
    /// A parameter value is absent. Every column must be presented, so there is no value this
    /// domain's typed `ParamValue` could ever hold for "no value".
    #[error("the remote leg parameter at column {at} is null")]
    Null {
        /// The column's position, never its field name.
        at: usize,
    },
    /// This domain's `ParamValue` has exactly two shapes a parameter may take - `Utf8` and
    /// `Date32` - and this column's Arrow type is neither.
    #[error("the remote leg parameter at column {at} has an unsupported Arrow type")]
    UnsupportedType {
        /// The column's position, never its field name.
        at: usize,
    },
    /// A `Date32` day count outside the years this domain's `Date` represents.
    #[error("the remote leg parameter at column {at} is a date outside the supported range")]
    DateOutOfRange {
        /// The column's position, never its field name.
        at: usize,
    },
}

/// Decode a `DoExchange` request's parameter batch into the positional values `LegWire::parse`
/// needs, refusing anything the two [`ParamValue`] shapes cannot carry.
///
/// Exactly one row is required, by position: `batch.num_rows() != 1` is [`ParamsError::RowCount`]
/// before any column is read. Zero columns over that one row decodes to an empty `Vec` - the shape
/// a lookup leg with no range parameters sends.
pub fn decode(batch: &RecordBatch) -> Result<Vec<ParamValue>, ParamsError> {
    if batch.num_rows() != 1 {
        return Err(ParamsError::RowCount);
    }
    (0..batch.num_columns()).map(|at| decode_one(batch, at)).collect()
}

fn decode_one(batch: &RecordBatch, at: usize) -> Result<ParamValue, ParamsError> {
    let column = batch.column(at);
    if column.is_null(0) {
        return Err(ParamsError::Null { at });
    }
    match column.data_type() {
        DataType::Utf8 => Ok(ParamValue::Text(String::from(column.as_string::<i32>().value(0)))),
        DataType::Date32 => Date::from_days_since_epoch(column.as_primitive::<Date32Type>().value(0))
            .map(ParamValue::Date)
            .map_err(|_cause| ParamsError::DateOutOfRange { at }),
        _ => Err(ParamsError::UnsupportedType { at }),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::{Array, Date32Array, Int64Array, RecordBatch, StringArray};
    use arrow_schema::{DataType, Field, Schema};
    use sutura_domain::calendar::Date;
    use sutura_domain::warehouse::ParamValue;

    use super::{ParamsError, decode};

    // `ParamValue::render` (`crates/sutura-domain/src/warehouse.rs`) is already the exhaustive,
    // no-wildcard match over `ParamValue`'s variants that a third shape must extend to keep
    // compiling - this module does not need a second one.

    type NamedColumn<'a> = (&'a str, DataType, Arc<dyn Array>);

    fn one_row(fields: Vec<NamedColumn<'_>>) -> RecordBatch {
        let schema = Arc::new(Schema::new(
            fields
                .iter()
                .map(|(name, data_type, _)| Field::new(*name, data_type.clone(), true))
                .collect::<Vec<_>>(),
        ));
        RecordBatch::try_new(schema, fields.into_iter().map(|(_, _, array)| array).collect()).expect("one row, matching types")
    }

    #[test]
    fn a_utf8_column_decodes_to_text() {
        let batch = one_row(vec![("x", DataType::Utf8, Arc::new(StringArray::from(vec!["berlin"])))]);
        assert_eq!(decode(&batch), Ok(vec![ParamValue::Text(String::from("berlin"))]));
    }

    #[test]
    fn a_date32_column_decodes_to_date() {
        let date = Date::from_days_since_epoch(19_723).expect("a representable date");
        let batch = one_row(vec![("x", DataType::Date32, Arc::new(Date32Array::from(vec![19_723])))]);
        assert_eq!(decode(&batch), Ok(vec![ParamValue::Date(date)]));
    }

    #[test]
    fn an_unsupported_arrow_type_is_refused_by_position() {
        let batch = one_row(vec![("n", DataType::Int64, Arc::new(Int64Array::from(vec![1])))]);
        assert_eq!(decode(&batch), Err(ParamsError::UnsupportedType { at: 0 }));
    }

    #[test]
    fn an_out_of_range_date_is_refused_by_position() {
        let batch = one_row(vec![
            ("x", DataType::Utf8, Arc::new(StringArray::from(vec!["ok"]))),
            ("d", DataType::Date32, Arc::new(Date32Array::from(vec![i32::MAX]))),
        ]);
        assert_eq!(decode(&batch), Err(ParamsError::DateOutOfRange { at: 1 }));
    }

    #[test]
    fn a_null_value_is_refused_by_position() {
        let batch = one_row(vec![("x", DataType::Utf8, Arc::new(StringArray::from(vec![None::<&str>])))]);
        assert_eq!(decode(&batch), Err(ParamsError::Null { at: 0 }));
    }

    #[test]
    fn zero_rows_is_refused() {
        let schema = Arc::new(Schema::new(vec![Field::new("x", DataType::Utf8, false)]));
        let batch = RecordBatch::try_new(schema, vec![Arc::new(StringArray::from(Vec::<&str>::new()))]).expect("zero rows");
        assert_eq!(decode(&batch), Err(ParamsError::RowCount));
    }

    #[test]
    fn two_rows_is_refused() {
        let batch = one_row(vec![("x", DataType::Utf8, Arc::new(StringArray::from(vec!["a", "b"])))]);
        assert_eq!(decode(&batch), Err(ParamsError::RowCount));
    }

    #[test]
    fn zero_columns_with_one_row_decodes_to_an_empty_vec() {
        let schema = Arc::new(Schema::empty());
        let batch = RecordBatch::try_new_with_options(
            schema,
            Vec::new(),
            &arrow_array::RecordBatchOptions::new().with_row_count(Some(1)),
        )
        .expect("zero columns, one row");
        assert_eq!(decode(&batch), Ok(Vec::new()));
    }

    /// Column POSITION is the identity: a field named for the wrong column must not relabel it.
    /// The names sort in the reverse of their positions, so a decoder ordering by name differs.
    #[test]
    fn column_position_not_field_name_is_the_identity() {
        let date = Date::from_days_since_epoch(100).expect("a representable date");
        let batch = one_row(vec![
            ("z_looks_like_a_date", DataType::Utf8, Arc::new(StringArray::from(vec!["ok"]))),
            ("a_looks_like_text", DataType::Date32, Arc::new(Date32Array::from(vec![100]))),
        ]);
        assert_eq!(
            decode(&batch),
            Ok(vec![ParamValue::Text(String::from("ok")), ParamValue::Date(date)])
        );
    }
}
