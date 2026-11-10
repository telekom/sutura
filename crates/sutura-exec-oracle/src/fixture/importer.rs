//! A fixture CSV as an Oracle table, for the golden matrix and the federated differential.
//!
//! `sutura_domain::warehouse::csv::infer` classifies each column once for every adapter, and this
//! module declares what Oracle holds it as:
//!
//! | Classification | Oracle column | Bound as |
//! | --- | --- | --- |
//! | `Boolean` | `BOOLEAN` (23ai) | a boolean |
//! | `Integer` | `NUMBER(19,0)` | a 64-bit integer |
//! | `WideInteger` | `NUMBER(38,0)` | a 64-bit unsigned integer |
//! | `Decimal`, `Real` | `BINARY_DOUBLE` | a double |
//! | `Date` | `DATE` | the text, through `TO_DATE(.., 'YYYY-MM-DD')` |
//! | `Text` | `VARCHAR2(4000)` | the text |
//!
//! **Every bind is independent of the session's NLS settings**: a number travels in the driver's
//! binary form and a date is parsed under an explicit format, so a tier whose `NLS_NUMERIC_CHARACTERS`
//! or `NLS_DATE_FORMAT` differs loads the same rows. Table and column names are quoted and lowercase,
//! as the renderer quotes them.
//!
//! # Limits
//!
//! - **A decimal is a double.** `Decimal` columns are `BINARY_DOUBLE`, not an exact `NUMBER`: the
//!   adapter reads a `NUMBER` with a scale back as text, so an exact column would answer differently
//!   from every other adapter for a reason that is the importer's. A sum over one is therefore as
//!   inexact as the other adapters' doubles, which the differential's real tolerance absorbs.
//! - **An integer column holds every `i64`**, which is nineteen digits, so `NUMBER(19,0)`; an integer
//!   beyond `i64` is `WideInteger` and `NUMBER(38,0)`, which the shared classification decides before
//!   this module sees a cell. A cell the importer cannot bind as its column's type is refused, by line
//!   and column, before any statement runs.
//! - **A text cell wider than 4000 bytes** is refused by the server, not here.
//! - **One batch per table, one transaction.** The table is dropped and recreated, so a load
//!   replaces what was there; a failure part-way leaves the dropped or empty table, and the
//!   connection's uncommitted rows are discarded when it closes.

use std::path::Path;

use sutura_domain::model::{ColumnName, TableName};
use sutura_domain::warehouse::csv::{self, FixtureType};

use super::FixtureError;
use crate::{DriverError, OracleWarehouse};

#[cfg(test)]
mod tests;

/// What one fixture column is declared as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OracleType {
    Boolean,
    Integer,
    WideInteger,
    Double,
    Date,
    Text,
}

impl OracleType {
    const fn of(kind: FixtureType) -> Self {
        match kind {
            FixtureType::Boolean => Self::Boolean,
            FixtureType::Integer => Self::Integer,
            FixtureType::WideInteger => Self::WideInteger,
            FixtureType::Decimal { .. } | FixtureType::Real => Self::Double,
            FixtureType::Date => Self::Date,
            FixtureType::Text => Self::Text,
        }
    }

    const fn ddl(self) -> &'static str {
        match self {
            Self::Boolean => "BOOLEAN",
            Self::Integer => "NUMBER(19,0)",
            Self::WideInteger => "NUMBER(38,0)",
            Self::Double => "BINARY_DOUBLE",
            Self::Date => "DATE",
            Self::Text => "VARCHAR2(4000)",
        }
    }

    /// The value expression of the `position`-th bind of a column of this type.
    fn value(self, position: usize) -> String {
        match self {
            Self::Date => format!("TO_DATE(:{position}, 'YYYY-MM-DD')"),
            Self::Boolean | Self::Integer | Self::WideInteger | Self::Double | Self::Text => format!(":{position}"),
        }
    }
}

/// One cell, typed as it is bound. `None` is SQL `NULL`: the fixture's empty cell.
#[derive(Debug, Clone, PartialEq)]
enum Cell {
    Boolean(Option<bool>),
    Integer(Option<i64>),
    Wide(Option<u64>),
    Double(Option<f64>),
    Text(Option<String>),
}

/// The variant that holds a column's parsed values.
type Wrap<T> = fn(Option<T>) -> Cell;

impl Cell {
    /// `text` as a cell of `kind`, or `None` where it is not one.
    fn parse(kind: OracleType, text: &str) -> Option<Self> {
        match kind {
            OracleType::Boolean => Self::of(text, parse_boolean, Self::Boolean),
            OracleType::Integer => Self::of(text, |text| text.parse().ok(), Self::Integer),
            OracleType::WideInteger => Self::of(text, |text| text.parse().ok(), Self::Wide),
            OracleType::Double => Self::of(text, |text| text.parse().ok(), Self::Double),
            OracleType::Date | OracleType::Text => Self::of(text, |text| Some(String::from(text)), Self::Text),
        }
    }

    /// An empty cell is `NULL` whatever the column's type; a present one is `parse`d, or is not a cell.
    fn of<T>(text: &str, parse: impl FnOnce(&str) -> Option<T>, wrap: Wrap<T>) -> Option<Self> {
        if text.is_empty() {
            Some(wrap(None))
        } else {
            parse(text).map(|value| wrap(Some(value)))
        }
    }

    fn as_dyn(&self) -> &dyn oracledb::ToDbValue {
        match self {
            Self::Boolean(value) => value,
            Self::Integer(value) => value,
            Self::Wide(value) => value,
            Self::Double(value) => value,
            Self::Text(value) => value,
        }
    }
}

const fn parse_boolean(text: &str) -> Option<bool> {
    if text.eq_ignore_ascii_case("true") {
        Some(true)
    } else if text.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// Why a fixture is not a table of the types it infers.
#[derive(Debug, thiserror::Error)]
pub enum Unloadable {
    /// The shared classification refused the fixture.
    #[error(transparent)]
    Schema(#[from] csv::InferenceError),
    /// The cell is not a value of the column's declared Oracle type.
    #[error("line {line}, column {column}: {cell:?} is not a {declared} value")]
    Cell {
        line: usize,
        column: String,
        cell: String,
        declared: &'static str,
    },
}

/// A fixture table, parsed and typed, ready to become statements.
#[derive(Debug, PartialEq)]
pub(super) struct Table {
    name: TableName,
    columns: Vec<(ColumnName, OracleType)>,
    rows: Vec<Row>,
}

impl Table {
    pub(super) fn parse(name: &TableName, text: &str) -> Result<Self, Unloadable> {
        let columns: Vec<(ColumnName, OracleType)> = csv::infer(text)?
            .into_iter()
            .map(|column| (column.name().clone(), OracleType::of(column.kind())))
            .collect();
        let rows = rows(text, &columns)?;
        Ok(Self {
            name: name.clone(),
            columns,
            rows,
        })
    }

    pub(super) fn drop_statement(&self) -> String {
        format!("DROP TABLE IF EXISTS \"{}\" PURGE", self.name.as_str())
    }

    pub(super) fn create_statement(&self) -> String {
        let defines: Vec<String> = self
            .columns
            .iter()
            .map(|&(ref column, kind)| format!("\"{}\" {}", column.as_str(), kind.ddl()))
            .collect();
        format!("CREATE TABLE \"{}\" ({})", self.name.as_str(), defines.join(", "))
    }

    /// The batch `INSERT`, or `None` for a fixture with a header and no rows.
    pub(super) fn insert_statement(&self) -> Option<String> {
        if self.rows.is_empty() {
            return None;
        }
        let names: Vec<String> = self
            .columns
            .iter()
            .map(|(column, _)| format!("\"{}\"", column.as_str()))
            .collect();
        let values: Vec<String> = self
            .columns
            .iter()
            .enumerate()
            .map(|(index, &(_, kind))| kind.value(index.saturating_add(1)))
            .collect();
        Some(format!(
            "INSERT INTO \"{}\" ({}) VALUES ({})",
            self.name.as_str(),
            names.join(", "),
            values.join(", ")
        ))
    }
}

/// One row's cells, in column order.
type Row = Vec<Cell>;

/// Every row of `text` after its header, each cell bound as its column's type.
fn rows(text: &str, columns: &[(ColumnName, OracleType)]) -> Result<Vec<Row>, Unloadable> {
    text.lines()
        .enumerate()
        .skip(1)
        .filter(|(_, line)| !line.is_empty())
        .map(|(index, line)| {
            line.split(',')
                .zip(columns)
                .map(|(text, &(ref column, kind))| {
                    Cell::parse(kind, text).ok_or_else(|| Unloadable::Cell {
                        line: index.saturating_add(1),
                        column: String::from(column.as_str()),
                        cell: String::from(text),
                        declared: kind.ddl(),
                    })
                })
                .collect()
        })
        .collect()
}

/// Loads a fixture CSV as the table `table` through `warehouse`'s own connection: see
/// `OracleWarehouse::load_csv`.
pub(crate) fn load(warehouse: &OracleWarehouse, table: &TableName, path: &Path) -> Result<(), FixtureError> {
    let shown = || path.display().to_string();
    let text = std::fs::read_to_string(path).map_err(|cause| FixtureError::Read { path: shown(), cause })?;
    let loaded = Table::parse(table, &text).map_err(|cause| FixtureError::Unloadable { path: shown(), cause })?;
    let refused = |what: &str| {
        let subject = format!("{what} {table}");
        move |cause: oracledb::Error| FixtureError::Statement {
            subject,
            cause: DriverError::from(cause),
        }
    };
    let _guard = warehouse.execution_lock.lock();
    let connection = &warehouse.connection;
    connection
        .execute(&loaded.drop_statement(), &[])
        .map_err(refused("dropping"))?;
    connection
        .execute(&loaded.create_statement(), &[])
        .map_err(refused("creating"))?;
    if let Some(insert) = loaded.insert_statement() {
        let bound: Vec<Vec<&dyn oracledb::ToDbValue>> =
            loaded.rows.iter().map(|row| row.iter().map(Cell::as_dyn).collect()).collect();
        let sets: Vec<&[&dyn oracledb::ToDbValue]> = bound.iter().map(Vec::as_slice).collect();
        connection
            .execute_batch(&insert, sets.as_slice())
            .map_err(refused("loading"))?;
    }
    connection.commit().map_err(refused("committing"))
}
