//! A leg's values as the one parameter batch a `DoExchange` carries.
//!
//! One row; column `i` is placeholder `i`. The receiver decodes by POSITION and never reads a field
//! name, so the names here (`p0`, `p1`, ..) carry nothing. A value is never null: a placeholder
//! always binds one.

use std::sync::Arc;

use arrow_array::{ArrayRef, Date32Array, RecordBatch, RecordBatchOptions, StringArray};
use arrow_schema::{ArrowError, DataType, Field, Schema};
use sutura_domain::warehouse::ParamValue;

/// The batch for `params`, in placeholder order.
pub(crate) fn batch(params: &[ParamValue]) -> Result<RecordBatch, ArrowError> {
    let mut fields = Vec::with_capacity(params.len());
    let mut columns: Vec<ArrayRef> = Vec::with_capacity(params.len());
    for (at, value) in params.iter().enumerate() {
        let (data_type, column): (DataType, ArrayRef) = match *value {
            ParamValue::Text(ref text) => (DataType::Utf8, Arc::new(StringArray::from(vec![text.as_str()]))),
            ParamValue::Date(date) => (DataType::Date32, Arc::new(Date32Array::from(vec![date.days_since_epoch()]))),
        };
        fields.push(Field::new(format!("p{at}"), data_type, false));
        columns.push(column);
    }
    RecordBatch::try_new_with_options(
        Arc::new(Schema::new(fields)),
        columns,
        &RecordBatchOptions::new().with_row_count(Some(1)),
    )
}

#[cfg(test)]
mod tests {
    use arrow_array::cast::AsArray as _;
    use arrow_array::types::Date32Type;
    use arrow_schema::DataType;
    use sutura_domain::calendar::Date;
    use sutura_domain::warehouse::ParamValue;

    use super::batch;

    #[test]
    fn each_value_is_one_column_of_one_row_in_placeholder_order() {
        let june = Date::parse("2026-06-01").expect("a date");
        let built = batch(&[ParamValue::Date(june), ParamValue::Text(String::from("paid"))]).expect("a batch");
        assert_eq!(built.num_rows(), 1);
        assert_eq!(built.column(0).data_type(), &DataType::Date32);
        assert_eq!(built.column(0).as_primitive::<Date32Type>().value(0), june.days_since_epoch());
        assert_eq!(built.column(1).as_string::<i32>().value(0), "paid");
        assert!(built.columns().iter().all(|column| column.null_count() == 0));
        assert_eq!(
            batch(&[]).expect("an empty batch").num_rows(),
            1,
            "no values still sends one row"
        );
    }
}
