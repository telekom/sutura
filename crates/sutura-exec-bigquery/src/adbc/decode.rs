//! Arrow → [`JobRows`] decode for the ADBC transport (telekom/sutura#913).
//!
//! The driver returns Arrow record batches; this pure half turns a schema +
//! batches into the adapter's own `JobRows`, reusing the same
//! [`crate::transport::FieldType`] vocabulary and the shared `crate::rowset::rows`
//! mapping the wire uses - one plan answered by two transports must produce one
//! number.
//!
//! Completeness is decided here. The wire refused a first page by comparing the
//! delivered count to the endpoint's `totalRows`; an ADBC read streams the whole
//! result, so completeness is the stream draining fully. [`job_rows`] takes the
//! drained rows plus a [`Reported`] total (the driver attaches its own job
//! statistics to the schema metadata, measured in the provisioned leg) and
//! refuses a delivered count that does not reach what was reported.

use std::error::Error;

use arrow_array::{Array, BooleanArray, Float64Array, Int64Array, RecordBatch, StringArray, StringViewArray};
use arrow_schema::{DataType, Schema};

use crate::transport::{Cell, Field, FieldType, JobRows};

/// Why a result set could not be decoded.
#[derive(Debug, PartialEq, Eq)]
pub enum Decode {
    /// A column whose Arrow type this adapter does not map.
    UnmappedColumn(String),
    /// The stream was not complete: a reported total the delivered rows do not
    /// reach.
    Incomplete { delivered: usize, reported: usize },
}

impl core::fmt::Display for Decode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnmappedColumn(name) => write!(f, "unmapped result column `{name}`"),
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

/// Maps one Arrow column type into the adapter's closed field vocabulary.
fn field_type(name: &str, dt: &DataType) -> FieldType {
    match dt {
        DataType::Int64 => FieldType::Int64,
        DataType::Float64 => FieldType::Float64,
        DataType::Boolean => FieldType::Bool,
        DataType::Utf8 | DataType::Utf8View | DataType::LargeUtf8 => FieldType::String,
        other => FieldType::Unmapped(format!("{other} ({name})")),
    }
}

/// One column's cells, as text or null.
///
/// A downcast that slips past `field_type`'s agreement is refused as an unmapped
/// column rather than answered null.
fn column(name: &str, array: &dyn Array) -> Result<Vec<Cell>, Decode> {
    let mut out = Vec::with_capacity(array.len());
    for row in 0..array.len() {
        let cell = if array.is_null(row) {
            Cell::Null
        } else {
            Cell::Text(match array.data_type() {
                DataType::Int64 => array
                    .as_any()
                    .downcast_ref::<Int64Array>()
                    .ok_or_else(|| Decode::UnmappedColumn(name.to_owned()))?
                    .value(row)
                    .to_string(),
                DataType::Float64 => array
                    .as_any()
                    .downcast_ref::<Float64Array>()
                    .ok_or_else(|| Decode::UnmappedColumn(name.to_owned()))?
                    .value(row)
                    .to_string(),
                DataType::Boolean => array
                    .as_any()
                    .downcast_ref::<BooleanArray>()
                    .ok_or_else(|| Decode::UnmappedColumn(name.to_owned()))?
                    .value(row)
                    .to_string(),
                DataType::Utf8 | DataType::LargeUtf8 => array
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .ok_or_else(|| Decode::UnmappedColumn(name.to_owned()))?
                    .value(row)
                    .to_owned(),
                DataType::Utf8View => array
                    .as_any()
                    .downcast_ref::<StringViewArray>()
                    .ok_or_else(|| Decode::UnmappedColumn(name.to_owned()))?
                    .value(row)
                    .to_owned(),
                _ => return Err(Decode::UnmappedColumn(name.to_owned())),
            })
        };
        out.push(cell);
    }
    Ok(out)
}

/// Decodes drained batches into a [`JobRows`], refusing an unmapped column or an
/// incomplete stream.
///
/// The schema-wide type pass runs before the value mapping, so a result with no
/// rows still refuses an unmapped column.
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
        let cols: Vec<Vec<Cell>> = schema
            .fields()
            .iter()
            .zip(batch.columns())
            .map(|(f, col)| column(f.name(), &**col))
            .collect::<Result<_, _>>()?;
        for r in 0..batch.num_rows() {
            let row = cols
                .iter()
                .map(|c| {
                    // `r` is bounded by `batch.num_rows()`, which is exactly
                    // `cols.len()` by construction; the `get` is defensive.
                    c.get(r).cloned().unwrap_or(Cell::Null)
                })
                .collect();
            rows.push(row);
        }
        delivered += batch.num_rows();
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
    use arrow_array::builder::{BooleanBuilder, Int64Builder, StringBuilder};
    use std::sync::Arc;

    fn ints(v: &[i64]) -> Arc<Int64Array> {
        let mut b = Int64Builder::new();
        for x in v {
            b.append_value(*x);
        }
        Arc::new(b.finish())
    }

    fn strings(v: &[Option<&str>]) -> Arc<StringArray> {
        let mut b = StringBuilder::new();
        for s in v {
            match s {
                Some(s) => b.append_value(*s),
                None => b.append_null(),
            }
        }
        Arc::new(b.finish())
    }

    fn ints_nullable(v: &[Option<i64>]) -> Arc<Int64Array> {
        let mut b = Int64Builder::new();
        for x in v {
            match x {
                Some(x) => b.append_value(*x),
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
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), vec![ints_nullable(&[Some(7), None])]).unwrap();
        let rows = job_rows(&schema, &[batch], Reported::Unreported).unwrap();
        assert_eq!(rows.rows()[1][0], Cell::Null);
    }

    #[test]
    fn a_null_cell_is_null_in_a_mapped_bool_column() {
        let schema = Schema::new(vec![arrow_schema::Field::new("ok", DataType::Boolean, true)]);
        let mut b = BooleanBuilder::new();
        b.append_value(true);
        b.append_null();
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), vec![Arc::new(b.finish())]).unwrap();
        let rows = job_rows(&schema, &[batch], Reported::Unreported).unwrap();
        assert_eq!(rows.rows()[1][0], Cell::Null);
    }
}
