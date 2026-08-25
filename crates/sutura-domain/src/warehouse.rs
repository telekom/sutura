//! The execution port: the statement that goes out, and the rows that come back.
//!
//! The trait is named `Warehouse`, which is the port's name and says nothing about what sits behind
//! it. A file read by an in-process engine and a cluster with a login are both implementations.
//!
//! This is the one module in the domain that names SQL, and the distinction is worth being precise
//! about. [`crate::query`] is the *tool* surface, where SQL must be unrepresentable because the text
//! would come from a caller. Here the text is something sutura generated a moment ago from a pinned
//! definition, and the port has to hand it to something. What the port does *not* accept is a
//! statement with values pasted into it: [`GeneratedQuery`] keeps them apart, so an adapter cannot
//! receive a query whose parameters have already been flattened into the text.

use crate::calendar::Date;
use crate::model::SourceName;

/// A value bound to a placeholder.
///
/// A closed set rather than a string, because the whole point is that these never become text on
/// our side. An adapter binds them with whatever its driver offers, and the driver is what decides
/// how a date is written on the wire.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum ParamValue {
    Text(String),
    Integer(i64),
    Date(Date),
}

impl ParamValue {
    /// A human-readable form, for showing a plan to a person.
    ///
    /// **Display only.** It is deliberately not the SQL literal for the value: a function that
    /// produced one would be the thing somebody reaches for the day they want to inline a parameter,
    /// and inlining a parameter is the one move this type exists to prevent. Text is quoted the way
    /// `Debug` quotes it, which makes an empty or space-padded value visible rather than SQL-shaped.
    pub fn render(&self) -> String {
        match *self {
            Self::Text(ref v) => format!("{v:?}"),
            Self::Integer(v) => v.to_string(),
            Self::Date(d) => d.to_iso(),
        }
    }
}

/// A statement, its parameters, and the one data system it runs against.
///
/// **Parameters are a separate field and there is no constructor that merges them.** That is the
/// mechanism behind "no value from a question reaches the statement as text": to inline a value an
/// adapter would have to build the string itself, which is a diff rather than an oversight.
///
/// `source` rides along because a plan resolves to exactly one data system, and carrying it here is
/// what lets the composition root check that the adapter it is about to call is the one the plan
/// named.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GeneratedQuery {
    source: SourceName,
    sql: String,
    params: Vec<ParamValue>,
}

impl GeneratedQuery {
    pub const fn new(source: SourceName, sql: String, params: Vec<ParamValue>) -> Self {
        Self { source, sql, params }
    }

    #[inline]
    pub const fn source(&self) -> &SourceName {
        &self.source
    }

    #[inline]
    pub fn sql(&self) -> &str {
        &self.sql
    }

    #[inline]
    pub fn params(&self) -> &[ParamValue] {
        &self.params
    }
}

/// One cell of a result.
///
/// `Real` is deliberately last on the list of things to reach for. A measure over integer minor
/// units stays exact, and an anchor comparison over a float would depend on how two languages print
/// the same bits. It exists because `avg` has to land somewhere.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum Value {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
}

impl Value {
    /// The canonical text form, which is what an anchor is compared against.
    ///
    /// One function so there is one answer. An anchor comparison that formatted the value at the
    /// call site would compare differently in two places, and the failure would look like a data
    /// problem rather than a formatting one.
    pub fn render(&self) -> String {
        match *self {
            Self::Null => String::from("null"),
            Self::Integer(v) => v.to_string(),
            // Shortest round-trip formatting, so a value that came back as an exact decimal is
            // rendered as one rather than as its binary expansion.
            Self::Real(v) => v.to_string(),
            Self::Text(ref v) => v.clone(),
        }
    }
}

/// A result set: the column labels, and the rows.
///
/// Labels are `String` rather than [`crate::model::ColumnName`] because a generated projection names
/// things a model did not: the truncated time bucket, and the measure under the metric's own name.
/// Constraining them to model column names would mean either lying about what they are or refusing
/// to name them.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RowSet {
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
}

/// Why a result set could not be built.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MalformedRowSet {
    /// A row has a different number of cells than there are columns.
    ///
    /// Checked once here rather than trusted, because every consumer downstream indexes by column
    /// position, and the `indexing_slicing` ban means each of them would otherwise need its own
    /// fallback for a case that must not exist.
    #[error("row {row} has {cells} cells, and there are {columns} columns")]
    RowWidth { row: usize, cells: usize, columns: usize },
}

impl RowSet {
    /// Builds a result set, rejecting a ragged one.
    pub fn new(columns: Vec<String>, rows: Vec<Vec<Value>>) -> Result<Self, MalformedRowSet> {
        for (index, row) in rows.iter().enumerate() {
            if row.len() != columns.len() {
                return Err(MalformedRowSet::RowWidth {
                    row: index,
                    cells: row.len(),
                    columns: columns.len(),
                });
            }
        }
        Ok(Self { columns, rows })
    }

    #[inline]
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    #[inline]
    pub fn rows(&self) -> &[Vec<Value>] {
        &self.rows
    }

    /// Where a column with this label sits, if there is exactly one.
    ///
    /// `None` for a label that appears twice, not the first match. Two columns with one label means
    /// the projection is not what we think it is, and returning either of them would answer with a
    /// number from a column nobody chose. `Definitions::assemble` refuses the catalog shapes that
    /// could cause it, so this is the second line rather than the first.
    pub fn column_index(&self, label: &str) -> Option<usize> {
        let mut found = None;
        for (index, name) in self.columns.iter().enumerate() {
            if name == label {
                if found.is_some() {
                    return None;
                }
                found = Some(index);
            }
        }
        found
    }

    /// One cell, by row and column position.
    ///
    /// `Option` rather than indexing, because `indexing_slicing` is denied for library crates here
    /// and because a caller that has a position from `column_index` still should not be able to
    /// panic on a result set that came back a different shape than expected.
    pub fn cell(&self, row: usize, column: usize) -> Option<&Value> {
        let cells = self.rows.get(row)?;
        cells.get(column)
    }

    /// The single cell of a single-row, single-column result, which is what an anchor check reads.
    ///
    /// `None` for any other shape rather than a panic or a silent first-cell: an anchor query that
    /// came back with three rows means the statement is not the one we thought, and reading its
    /// first cell would turn that into a wrong number.
    pub const fn scalar(&self) -> Option<&Value> {
        match (self.columns.as_slice(), self.rows.as_slice()) {
            ([_], [row]) => match row.as_slice() {
                [cell] => Some(cell),
                _ => None,
            },
            _ => None,
        }
    }
}

/// Where a statement runs.
///
/// `dry_run` exists separately from `execute` because "would this be accepted, and how much would it
/// read" is a question worth being able to ask before committing to the cost. An adapter with no
/// such facility answers it by checking what it can.
pub trait Warehouse {
    /// Why this data system could not answer. Typed per adapter: a connection failure, a rejected
    /// statement and a permission denial are not the same thing to whoever responds to them.
    type Error: core::error::Error + 'static;

    /// The name a plan uses to select this adapter.
    fn source(&self) -> &SourceName;

    /// Checks a statement without producing rows.
    fn dry_run(&self, query: &GeneratedQuery) -> Result<(), Self::Error>;

    /// Runs a statement and returns its rows.
    fn execute(&self, query: &GeneratedQuery) -> Result<RowSet, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::{GeneratedQuery, MalformedRowSet, ParamValue, RowSet, Value};
    use crate::calendar::Date;
    use crate::model::SourceName;

    fn source() -> SourceName {
        SourceName::parse("local").expect("a test source is a source")
    }

    #[test]
    fn a_generated_query_keeps_its_parameters_out_of_its_text() {
        // The mechanism behind the no-injection claim, asserted at the type level: an adapter
        // receives the statement and the values separately, so inlining one would be its own code
        // rather than an accident here.
        let query = GeneratedQuery::new(
            source(),
            String::from("SELECT 1 WHERE d >= ? AND d < ?"),
            vec![
                ParamValue::Date(Date::parse("2026-06-01").expect("a test date is a date")),
                ParamValue::Date(Date::parse("2026-07-01").expect("a test date is a date")),
            ],
        );
        assert!(!query.sql().contains("2026"), "{}", query.sql());
        assert_eq!(query.params().len(), 2);
        assert_eq!(query.source().as_str(), "local");
    }

    #[test]
    fn a_parameter_renders_for_a_reader_and_not_as_sql() {
        // The rendering exists so `sutura compile` can show a plan. It must not look like something
        // to paste into a statement: text keeps its quotes so an empty or padded value is visible,
        // and nothing here escapes anything, because escaping is what a bind parameter replaces.
        assert_eq!(ParamValue::Integer(42).render(), "42");
        assert_eq!(
            ParamValue::Date(Date::parse("2026-06-01").expect("a test date is a date")).render(),
            "2026-06-01"
        );
        assert_eq!(ParamValue::Text(String::from("north")).render(), "\"north\"");
        assert_eq!(ParamValue::Text(String::new()).render(), "\"\"");
        // A value that would be an injection if it were text in a statement renders visibly as a
        // value rather than as syntax.
        assert_eq!(ParamValue::Text(String::from("a' OR '1'='1")).render(), "\"a' OR '1'='1\"");
    }

    #[test]
    fn a_ragged_result_set_is_rejected_once_rather_than_handled_everywhere() {
        // Every consumer reads cells by column position. Without this check each of them needs its
        // own fallback for a shape that must not exist, and under the `indexing_slicing` ban those
        // fallbacks are where a wrong value gets substituted for a missing one.
        assert_eq!(
            RowSet::new(vec![String::from("a"), String::from("b")], vec![vec![Value::Integer(1)]],).unwrap_err(),
            MalformedRowSet::RowWidth {
                row: 0,
                cells: 1,
                columns: 2,
            }
        );
    }

    #[test]
    fn scalar_refuses_any_shape_that_is_not_one_cell() {
        // An anchor check reads this. If it returned the first cell of a three-row result, an
        // anchor would silently pass against a statement that grouped when it should not have.
        let one = RowSet::new(vec![String::from("v")], vec![vec![Value::Integer(7)]]).expect("one cell is a valid result");
        assert_eq!(one.scalar(), Some(&Value::Integer(7)));

        let two_rows = RowSet::new(
            vec![String::from("v")],
            vec![vec![Value::Integer(7)], vec![Value::Integer(8)]],
        )
        .expect("two rows is a valid result");
        assert_eq!(two_rows.scalar(), None);

        let two_columns = RowSet::new(
            vec![String::from("a"), String::from("b")],
            vec![vec![Value::Integer(7), Value::Integer(8)]],
        )
        .expect("two columns is a valid result");
        assert_eq!(two_columns.scalar(), None);

        let empty = RowSet::new(vec![String::from("v")], vec![]).expect("no rows is a result");
        assert_eq!(empty.scalar(), None);
    }

    #[test]
    fn rendering_is_one_function_so_an_anchor_compares_the_same_way_everywhere() {
        assert_eq!(Value::Integer(197_122).render(), "197122");
        assert_eq!(Value::Text(String::from("north")).render(), "north");
        assert_eq!(Value::Null.render(), "null");
        // Shortest round-trip: an exact decimal comes back as one rather than as 0.30000000000000004.
        assert_eq!(Value::Real(0.3_f64).render(), "0.3");
    }
}
