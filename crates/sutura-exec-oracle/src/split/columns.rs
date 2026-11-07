//! The one schema every partition of a split answers, where its columns have one -
//! `github.com/telekom/sutura#828`.
//!
//! A row-speaking answer is typed by its cells (`sutura_domain::warehouse::arrow::of_rows`): an
//! empty or all-`NULL` column reads as `Int64`, and a `NUMBER` column whose cells are not all
//! integers reads as text. Two partitions of one leg can then answer two schemas. A granted split
//! needs one, so a column is typed by what the database declares it, never by its cells, and a leg
//! with a column no declaration fixes is read on the planning node.
//!
//! Not built yet: no column is fixed, so no split describes one schema, and [`typed`] types by the
//! cells, as a whole read does.

use arrow_schema::{DataType, SchemaRef};
use sutura_domain::warehouse::arrow::of_row_set;
use sutura_domain::warehouse::{ResultBatches, RowSet};

use crate::OracleError;

/// The Arrow type every partition answers a column the database declares as `db_type`,
/// `precision` and `scale` with, or `None` where its cells decide the type.
pub(crate) const fn fixed(_db_type: &oracledb::DbType, _precision: u8, _scale: i8) -> Option<DataType> {
    None
}

/// The schema of `columns`, each its name and its declaration (type, precision, scale), or `None`
/// where one of them is not [`fixed`].
pub(crate) fn declared<'a>(_columns: impl IntoIterator<Item = (&'a str, &'a oracledb::DbType, u8, i8)>) -> Option<SchemaRef> {
    None
}

/// `rows` as batches under `schema`, each column of its declared type whatever its cells hold.
///
/// # Errors
///
/// [`OracleError::NotOfItsColumnType`] for a cell its column's type cannot hold, and
/// [`OracleError::Shape`] for rows `schema` does not describe.
pub(crate) fn typed(rows: &RowSet, _schema: &SchemaRef) -> Result<ResultBatches, OracleError> {
    of_row_set(rows).map_err(|cause| OracleError::Shape { cause })
}

#[cfg(test)]
mod tests;
