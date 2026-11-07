//! The one schema every partition of a split answers, where its columns have one -
//! `github.com/telekom/sutura#828`.
//!
//! A row-speaking answer is typed by its cells (`sutura_domain::warehouse::arrow::of_rows`): an
//! empty or all-`NULL` column reads as `Int64`, and a `NUMBER` column whose cells are not all
//! integers reads as text. Two partitions of one leg can then answer two schemas. A granted split
//! needs one, so a column is typed by what the database declares it, never by its cells, and a leg
//! with a column no declaration fixes is read on the planning node.
//!
//! **The limit:** only `NUMBER` of precision 1 to 18 and scale 0, `BINARY_DOUBLE` and the four
//! character types are fixed. A `DATE`, a `TIMESTAMP`, a `NUMBER` of any other precision or scale,
//! and every other type leave the split with no one schema.

use std::sync::Arc;

use arrow_array::{ArrayRef, Float64Array, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use sutura_domain::warehouse::{Accumulating, MalformedRowSet, ResultBatches, ResultBudget, RowSet, Value};

use crate::OracleError;

/// The most decimal digits of which every value fits an `i64`.
const MOST_INTEGER_DIGITS: u8 = 18;

/// The character types, each read as its text.
const TEXTS: [&oracledb::DbType; 4] = [
    oracledb::DB_TYPE_VARCHAR,
    oracledb::DB_TYPE_NVARCHAR,
    oracledb::DB_TYPE_CHAR,
    oracledb::DB_TYPE_NCHAR,
];

/// The Arrow type every partition answers a column the database declares as `db_type`,
/// `precision` and `scale` with, or `None` where its cells decide the type.
pub(crate) fn fixed(db_type: &oracledb::DbType, precision: u8, scale: i8) -> Option<DataType> {
    if db_type == oracledb::DB_TYPE_NUMBER {
        return (scale == 0 && (1..=MOST_INTEGER_DIGITS).contains(&precision)).then_some(DataType::Int64);
    }
    if db_type == oracledb::DB_TYPE_BINARY_DOUBLE {
        return Some(DataType::Float64);
    }
    TEXTS.contains(&db_type).then_some(DataType::Utf8)
}

/// The schema of `columns`, each its name and its declaration (type, precision, scale), or `None`
/// where one of them is not [`fixed`].
pub(crate) fn declared<'a>(columns: impl IntoIterator<Item = (&'a str, &'a oracledb::DbType, u8, i8)>) -> Option<SchemaRef> {
    let fields = columns
        .into_iter()
        .map(|(name, db_type, precision, scale)| fixed(db_type, precision, scale).map(|kind| Field::new(name, kind, true)))
        .collect::<Option<Vec<_>>>()?;
    Some(Arc::new(Schema::new(fields)))
}

/// `rows` as batches under `schema`, each column of its declared type whatever its cells hold,
/// charged against `budget`.
///
/// # Errors
///
/// [`OracleError::NotOfItsColumnType`] for a cell its column's type cannot hold,
/// [`OracleError::Shape`] for rows `schema` does not describe, and a batch past `budget`.
pub(crate) fn typed(rows: &RowSet, schema: &SchemaRef, budget: ResultBudget) -> Result<ResultBatches, OracleError> {
    let width = || OracleError::Shape {
        cause: MalformedRowSet::RowWidth {
            row: 0,
            cells: schema.fields().len(),
            columns: rows.columns().len(),
        },
    };
    if schema.fields().len() != rows.columns().len() {
        return Err(width());
    }
    let arrays = schema
        .fields()
        .iter()
        .enumerate()
        .map(|(index, field)| column(rows, index, field))
        .collect::<Result<Vec<ArrayRef>, OracleError>>()?;
    let mut accumulating = Accumulating::announcing(Arc::clone(schema), usize::MAX, budget);
    if !rows.rows().is_empty() {
        let batch = RecordBatch::try_new(Arc::clone(schema), arrays).map_err(|_arrow| width())?;
        accumulating.push(batch).map_err(crate::batches::unannounced)?;
    }
    Ok(accumulating.finish())
}

/// Column `index` of `rows` as one array of `field`'s type.
fn column(rows: &RowSet, index: usize, field: &Field) -> Result<ArrayRef, OracleError> {
    let refused = || OracleError::NotOfItsColumnType {
        column: field.name().clone(),
    };
    let cells = rows.rows().iter().map(|row| row.get(index).unwrap_or(&Value::Null));
    Ok(match *field.data_type() {
        DataType::Int64 => Arc::new(Int64Array::from(
            cells
                .map(|cell| match *cell {
                    Value::Null => Ok(None),
                    Value::Integer(number) => Ok(Some(number)),
                    _ => Err(refused()),
                })
                .collect::<Result<Vec<_>, _>>()?,
        )),
        DataType::Float64 => Arc::new(Float64Array::from(
            cells
                .map(|cell| match *cell {
                    Value::Null => Ok(None),
                    Value::Real(number) => Ok(Some(number.get())),
                    _ => Err(refused()),
                })
                .collect::<Result<Vec<_>, _>>()?,
        )),
        DataType::Utf8 => Arc::new(StringArray::from(
            cells
                .map(|cell| match *cell {
                    Value::Null => Ok(None),
                    Value::Text(ref text) => Ok(Some(text.clone())),
                    _ => Err(refused()),
                })
                .collect::<Result<Vec<_>, _>>()?,
        )),
        _ => return Err(refused()),
    })
}

#[cfg(test)]
mod tests;
