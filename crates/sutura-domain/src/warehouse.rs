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

/// Why a floating-point cell was refused.
///
/// Two variants rather than one, because the two faults have different causes and a reader chasing
/// one is not chasing the other: an infinity is a non-zero quantity divided by zero, and a `NaN` is
/// zero divided by zero. The variant carries the value rather than a formatted sentence, for the
/// reason every error in this crate does.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum NotFinite {
    /// Infinite, in either direction.
    #[error("{value} is not a finite number")]
    Infinite { value: f64 },
    /// Not a number at all. Its own variant rather than a value on the one above, because `NaN`
    /// compares unequal to itself: an [`Infinite`] carrying one would make two of these errors
    /// unequal for a reason that has nothing to do with what happened.
    ///
    /// [`Infinite`]: NotFinite::Infinite
    #[error("NaN is not a number")]
    NotANumber,
}

/// A real number a result may carry: finite, and nothing else.
///
/// **Parsed rather than validated, and the class it closes is larger than the bug that found it.**
/// A cell used to be a raw `f64`, so `inf`, `-inf` and `NaN` were all representable, and
/// [`Value::render`] turned the first of them into the string `"inf"` - an answer under a metric's
/// own certified name that reads as data and is not a number. The route in was a ratio measure
/// declaring `zero_denominator: fails`: both adapters cast the numerator to a floating type before
/// dividing, so the division is IEEE float division, and IEEE float division by zero does not fail.
/// It answers `inf`, or `NaN` when both halves are zero.
///
/// Making the domain type refuse a non-finite value closes all three at once, at the one boundary
/// every adapter has to cross, rather than guarding the one variant that exposed it. An adapter that
/// gets one back has an error naming the column, which is what `fails` was always claiming to mean.
///
/// Construct it with [`parse`]. The field is private, so a non-finite value is unrepresentable
/// rather than merely rejected. There is deliberately no `Deref` and no arithmetic: two finite
/// numbers divide to a non-finite one, so a type that let the result back in without passing
/// [`parse`] again would be the hole this closes. [`Value`] is `Serialize` only today - if it ever
/// gains `Deserialize`, this needs `#[serde(try_from = ..)]` routing through [`parse`], because a
/// derived one writes straight into the private field.
///
/// [`parse`]: Real::parse
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Real(f64);

impl Real {
    /// Parses a real number, rejecting a non-finite one.
    pub const fn parse(value: f64) -> Result<Self, NotFinite> {
        if value.is_nan() {
            return Err(NotFinite::NotANumber);
        }
        if value.is_infinite() {
            return Err(NotFinite::Infinite { value });
        }
        Ok(Self(value))
    }

    /// The number, for a caller that has to do arithmetic on it.
    ///
    /// Named rather than reached through `Deref`, so the point at which the invariant stops applying
    /// is a call somebody wrote.
    #[inline]
    pub const fn get(self) -> f64 {
        self.0
    }
}

/// Shortest round-trip formatting, so a value that came back as an exact decimal is rendered as one
/// rather than as its binary expansion. Delegated rather than reimplemented, and this is the one
/// definition [`Value::render`] uses.
impl core::fmt::Display for Real {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

/// Exponent form, forwarding the formatter's precision.
///
/// It exists because comparing two engines' floats is done at a fixed number of significant digits -
/// summing the same rows in a different order changes the last place of an `f64` - and `{:.12e}` is
/// how that comparison is written. A formatting trait rather than `get`, so the comparison does not
/// have to leave the type to be expressed.
impl core::fmt::LowerExp for Real {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::LowerExp::fmt(&self.0, f)
    }
}

/// One cell of a result.
///
/// [`Real`] is deliberately last on the list of things to reach for. A measure over integer minor
/// units stays exact, and an anchor comparison over a float would depend on how two languages print
/// the same bits. It exists because `avg` has to land somewhere - and it is a checked type rather
/// than an `f64`, so the one thing a float can be that a number cannot does not fit in a cell.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum Value {
    Null,
    Integer(i64),
    Real(Real),
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
            // One definition of what a real number looks like, on the type that carries one.
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

/// Where a plan runs.
///
/// **The port takes a [`crate::plan::QueryPlan`], not a statement, and that is what makes a second
/// kind of adapter possible.** Taking a rendered statement said that every data system speaks SQL.
/// An in-process engine does not: it executes a logical plan over Arrow and generates no SQL at all.
/// So the plan is the contract and rendering is one adapter's private business.
///
/// `dry_run` exists separately from `execute` because "would this be accepted" is worth being able
/// to ask before committing to the cost of an answer. An adapter with no such facility answers it by
/// checking what it can.
pub trait Warehouse {
    /// Why this data system could not answer. Typed per adapter: a connection failure, a rejected
    /// statement and a permission denial are not the same thing to whoever responds to them.
    type Error: core::error::Error + 'static;

    /// The name a plan uses to select this adapter.
    fn source(&self) -> &SourceName;

    /// Checks the plan is executable here, without producing rows.
    fn dry_run(&self, plan: &crate::plan::QueryPlan) -> Result<(), Self::Error>;

    /// Runs the plan and returns its rows.
    fn execute(&self, plan: &crate::plan::QueryPlan) -> Result<RowSet, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::{GeneratedQuery, MalformedRowSet, NotFinite, ParamValue, Real, RowSet, Value};
    use crate::calendar::Date;
    use crate::model::SourceName;

    fn source() -> SourceName {
        SourceName::parse("local").expect("a test source is a source")
    }

    fn real(value: f64) -> Real {
        Real::parse(value).expect("a test literal is finite")
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
        assert_eq!(Value::Real(real(0.3_f64)).render(), "0.3");
    }

    #[test]
    fn a_cell_cannot_hold_a_number_that_is_not_one() {
        // THE BUG THIS EXISTS FOR. `Real` used to be a raw `f64`, so a ratio measure declaring
        // `zero_denominator: fails` answered the string "inf" under its own certified metric name:
        // both adapters cast the numerator to a floating type before dividing, so the division is
        // IEEE float division, and IEEE float division by zero does not fail. Nothing between the
        // data system and the caller looked at the value, because nothing had a place to.
        //
        // Asserted over all three of the class rather than over the one variant that exposed it: a
        // guard on the division would have left `-inf` and `NaN` representable.
        assert_eq!(
            Real::parse(f64::INFINITY).unwrap_err(),
            NotFinite::Infinite { value: f64::INFINITY }
        );
        assert_eq!(
            Real::parse(f64::NEG_INFINITY).unwrap_err(),
            NotFinite::Infinite {
                value: f64::NEG_INFINITY
            }
        );
        assert_eq!(Real::parse(f64::NAN).unwrap_err(), NotFinite::NotANumber);
        // The messages an adapter's error chain ends in, so the reader is told which of the three.
        assert_eq!(
            Real::parse(f64::INFINITY).unwrap_err().to_string(),
            "inf is not a finite number"
        );
        assert_eq!(
            Real::parse(f64::NEG_INFINITY).unwrap_err().to_string(),
            "-inf is not a finite number"
        );
        assert_eq!(Real::parse(f64::NAN).unwrap_err().to_string(), "NaN is not a number");

        // And what a real number still does, so this is not a test that would pass with every float
        // refused. Zero and the subnormals are finite, and a metric that legitimately answers zero
        // must not be caught by a check aimed at a division by it.
        for finite in [0.0_f64, -0.0_f64, 0.3_f64, f64::MIN, f64::MAX, f64::MIN_POSITIVE] {
            // Compared as bits rather than with `==`, which `float_cmp` bans for the reason it exists:
            // the assertion here is that the value came through UNCHANGED, and bit equality is that
            // claim exactly. It also keeps negative zero distinguishable from zero.
            assert_eq!(real(finite).get().to_bits(), finite.to_bits(), "{finite} is a finite number");
        }
    }

    #[test]
    fn a_real_number_renders_the_same_way_wherever_it_is_formatted() {
        // `Value::render` is what an anchor is compared against and `{:.12e}` is what a differential
        // comparison between two engines uses. Both go through this one type, so neither can drift
        // into its own idea of what the number looks like.
        assert_eq!(format!("{}", real(0.3_f64)), "0.3");
        assert_eq!(format!("{:.12e}", real(190_007.333_333_333_34_f64)), "1.900073333333e5");
    }
}
