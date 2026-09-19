//! Arrow -> [`JobRows`] decode for the ADBC transport (telekom/sutura#913).
//!
//! The driver returns Arrow record batches; this pure half turns a schema +
//! batches into the adapter's own `JobRows`, reusing the same
//! [`crate::transport::FieldType`] vocabulary the wire uses, so one plan answered
//! by two transports agrees on field kinds. The value pass is one vectorised
//! [`arrow_cast::cast`] to `Utf8` per column rather than a per-cell downcast
//! ladder.
//!
//! Completeness is decided here. The wire refused a first page by comparing the
//! delivered count to the endpoint's `totalRows`; an ADBC read streams the whole
//! result, so completeness is the stream draining fully. [`job_rows`] takes the
//! drained rows plus a [`Reported`] total (the driver attaches job statistics to
//! the schema metadata, measured in the provisioned leg) and refuses a delivered
//! count that does not reach what was reported.

use std::error::Error;

use arrow_array::Array as _;
use arrow_array::{RecordBatch, StringArray};
use arrow_cast::cast;
use arrow_schema::{DataType, Schema};

use crate::transport::{Cell, Field, FieldType, JobRows};

/// Why a result set could not be decoded.
#[derive(Debug, PartialEq, Eq)]
pub enum Decode {
    /// A column whose Arrow type this adapter does not map.
    UnmappedColumn(String),
    /// A batch that disagrees with the schema it was announced under — a stream
    /// arriving over a C ABI from a foreign driver is exactly the case to refuse
    /// rather than trust.
    Shape { fields: usize, columns: usize },
    /// The stream was not complete: a reported total the delivered rows do not
    /// reach.
    Incomplete { delivered: usize, reported: usize },
}

impl core::fmt::Display for Decode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnmappedColumn(name) => write!(f, "unmapped result column `{name}`"),
            Self::Shape { fields, columns } => {
                write!(f, "result batch has {columns} columns for a {fields}-field schema")
            }
            Self::Incomplete { delivered, reported } => {
                write!(f, "incomplete answer: delivered {delivered} of {reported} reported rows")
            }
        }
    }
}

impl Error for Decode {}

/// The row count a stream reports, when it reports one at all.
///
/// `None` is an honest absence, never a defaulted `0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reported {
    Unreported,
    Total(usize),
}

/// Maps one Arrow column type into the adapter's closed field vocabulary, the
/// same six the wire accepts: `INT64`, `FLOAT64`, `NUMERIC`, `BOOL`, `STRING`,
/// `DATE`.
fn field_type(name: &str, dt: &DataType) -> FieldType {
    match dt {
        DataType::Int64 => FieldType::Int64,
        DataType::Float64 => FieldType::Float64,
        DataType::Decimal128(_, _) | DataType::Decimal256(_, _) => FieldType::Numeric,
        DataType::Boolean => FieldType::Bool,
        DataType::Utf8 | DataType::Utf8View | DataType::LargeUtf8 => FieldType::String,
        DataType::Date32 | DataType::Date64 => FieldType::Date,
        other => FieldType::Unmapped(format!("{other} ({name})")),
    }
}

/// Decodes drained batches into a [`JobRows`], refusing an unmapped column, a
/// batch narrower than the schema, or an incomplete stream.
///
/// The schema-wide type pass runs before any value work — a result with no rows
/// still refuses an unmapped column — and each batch's column count is checked
/// against the schema before its cells are read (fail closed, not short).
pub fn job_rows(schema: &Schema, batches: &[RecordBatch], reported: Reported) -> Result<JobRows, Decode> {
    let mut fields = Vec::with_capacity(schema.fields().len());
    for f in schema.fields() {
        let ft = field_type(f.name(), f.data_type());
        if matches!(ft, FieldType::Unmapped(_)) {
            return Err(Decode::UnmappedColumn(f.name().to_owned()));
        }
        fields.push(Field::of(f.name().to_owned(), ft));
    }

    let mut rows: Vec<Vec<Cell>> = Vec::new();
    let mut delivered = 0usize;
    for batch in batches {
        if batch.num_columns() != schema.fields().len() {
            return Err(Decode::Shape {
                fields: schema.fields().len(),
                columns: batch.num_columns(),
            });
        }
        let n = batch.num_rows();
        for r in 0..n {
            let mut cells = Vec::with_capacity(batch.num_columns());
            for col in batch.columns() {
                let str_arr =
                    cast(&**col, &DataType::Utf8).map_err(|e| Decode::UnmappedColumn(format!("column cast failed: {e}")))?;
                let s = str_arr
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .ok_or_else(|| Decode::UnmappedColumn("not a string after cast".to_owned()))?;
                cells.push(if s.is_null(r) {
                    Cell::Null
                } else {
                    Cell::Text(s.value(r).to_owned())
                });
            }
            rows.push(cells);
        }
        delivered += n;
    }

    if let Reported::Total(total) = reported
        && delivered != total
    {
        return Err(Decode::Incomplete {
            delivered,
            reported: total,
        });
    }

    Ok(JobRows::of(fields, rows, delivered))
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::builder::{Int64Builder, StringBuilder};
    use std::sync::Arc;

    fn ints(v: &[i64]) -> Arc<arrow_array::Int64Array> {
        let mut b = Int64Builder::new();
        for x in v {
            b.append_value(*x);
        }
        Arc::new(b.finish())
    }

    fn strings(v: &[Option<&str>]) -> Arc<arrow_array::StringArray> {
        let mut b = StringBuilder::new();
        for s in v {
            match s {
                Some(s) => b.append_value(s),
                None => b.append_null(),
            }
        }
        Arc::new(b.finish())
    }

    #[test]
    fn maps_columns_and_rows() {
        let schema = Schema::new(vec![
            arrow_schema::Field::new("id", DataType::Int64, false),
            arrow_schema::Field::new("name", DataType::Utf8, false),
        ]);
        let batch = RecordBatch::try_new(Arc::new(schema), vec![ints(&[1, 2]), strings(&[Some("a"), Some("b")])]).unwrap();
        let rows = job_rows(batch.schema().as_ref(), &[batch], Reported::Total(2)).unwrap();
        assert_eq!(rows.fields().len(), 2);
        assert_eq!(rows.rows().len(), 2);
        assert_eq!(rows.total_rows(), 2);
        assert_eq!(rows.rows()[0][0], Cell::Text("1".into()));
        assert_eq!(rows.rows()[0][1], Cell::Text("a".into()));
    }

    #[test]
    fn refuses_an_unmapped_column_even_with_no_rows() {
        let schema = Schema::new(vec![arrow_schema::Field::new("geo", DataType::Binary, false)]);
        let res = job_rows(&schema, &[], Reported::Unreported);
        assert!(matches!(res, Err(Decode::UnmappedColumn(_))));
    }

    #[test]
    fn refuses_a_batch_narrower_than_the_schema() {
        // Two fields announced, but the batch (its own 1-field schema, valid on
        // its own) carries one column — the public job_rows API can be handed a
        // schema that does not match the batches it is given.
        let announced = Schema::new(vec![
            arrow_schema::Field::new("id", DataType::Int64, false),
            arrow_schema::Field::new("name", DataType::Utf8, false),
        ]);
        let batch_schema = Schema::new(vec![arrow_schema::Field::new("id", DataType::Int64, false)]);
        let batch = RecordBatch::try_new(Arc::new(batch_schema), vec![ints(&[1, 2])]).unwrap();
        let res = job_rows(&announced, &[batch], Reported::Unreported);
        assert!(matches!(res, Err(Decode::Shape { fields: 2, columns: 1 })));
    }

    #[test]
    fn refuses_an_incomplete_stream() {
        let schema = Schema::new(vec![arrow_schema::Field::new("id", DataType::Int64, false)]);
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), vec![ints(&[1, 2, 3])]).unwrap();
        let res = job_rows(&schema, &[batch], Reported::Total(5));
        assert!(matches!(
            res,
            Err(Decode::Incomplete {
                delivered: 3,
                reported: 5
            })
        ));
    }

    #[test]
    fn a_null_cell_is_null_not_a_number() {
        let schema = Schema::new(vec![arrow_schema::Field::new("id", DataType::Int64, true)]);
        let mut b = Int64Builder::new();
        b.append_value(7);
        b.append_null();
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), vec![Arc::new(b.finish())]).unwrap();
        let rows = job_rows(&schema, &[batch], Reported::Unreported).unwrap();
        assert_eq!(rows.rows()[1][0], Cell::Null);
    }

    #[test]
    fn a_date32_column_renders_iso() {
        // 0 days after the epoch = 1970-01-01, ISO.
        let schema = Schema::new(vec![arrow_schema::Field::new("d", DataType::Date32, true)]);
        let arr = arrow_array::Date32Array::from(vec![Some(0)]);
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), vec![Arc::new(arr)]).unwrap();
        let rows = job_rows(&schema, &[batch], Reported::Unreported).unwrap();
        assert_eq!(*rows.fields()[0].kind(), crate::transport::FieldType::Date);
        assert_eq!(rows.rows()[0][0], Cell::Text("1970-01-01".into()));
    }

    #[test]
    fn a_decimal128_column_renders_exact_scale() {
        // 12345 with scale 2 => 123.45, exact digits.
        use arrow_array::array::Decimal128Array;
        let schema = Schema::new(vec![arrow_schema::Field::new("amount", DataType::Decimal128(9, 2), true)]);
        let arr = Decimal128Array::from(vec![Some(12345i128)])
            .with_precision_and_scale(9, 2)
            .unwrap();
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), vec![Arc::new(arr)]).unwrap();
        let rows = job_rows(&schema, &[batch], Reported::Unreported).unwrap();
        assert_eq!(*rows.fields()[0].kind(), crate::transport::FieldType::Numeric);
        assert_eq!(rows.rows()[0][0], Cell::Text("123.45".into()));
    }
}
