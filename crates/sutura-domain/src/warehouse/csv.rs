//! Inferring the column types of a fixture CSV, once, for every adapter.
//!
//! A fixture is a committed CSV, and a data system has to be *given* typed columns before it can
//! answer. Three adapters read the same bytes - `DuckDB` via `read_csv`, the engine via Arrow, and
//! `Postgres` via `COPY` - and until this module they each decided the types for themselves.
//!
//! `read_csv_auto` turned a fractional column into `DOUBLE` (so a total answered as a float), the
//! engine inferred a double too, and only the Postgres importer held a fraction as an exact
//! `NUMERIC`. One classification here is what makes a decimal column a decimal on every wire.
//!
//! **The Postgres importer's logic is the origin, narrowed to spellings all three readers share.**
//! [`crate::warehouse::csv::infer`] probes Boolean, then a 64-bit integer, then an exact fixed-point
//! decimal, then a double, then a date, then text. Boolean means `true` or `false`; Postgres's `t`
//! and `f` shorthand stays text because the engine reader does not accept it as Boolean. A fixture
//! that Postgres typed `NUMERIC(38,2)` is a
//! [`crate::warehouse::csv::FixtureType::Decimal`] in its canonical scale here, and the engine and
//! `DuckDB` now agree rather than drifting to a float.
//!
//! # The boundary a column metric needs, stated per type
//!
//! - A column whose integers fit `i64` is [`crate::warehouse::csv::FixtureType::Integer`].
//! - A non-negative integer column that exceeds `i64` but fits `u64` is
//!   [`crate::warehouse::csv::FixtureType::WideInteger`].
//! - A column with any fixed-point decimal value is
//!   [`crate::warehouse::csv::FixtureType::Decimal`] at the widest canonical scale after trailing
//!   fractional zeroes are removed, when every possible subtotal fits the shared 38-digit type.
//! - A column with an integer and a fraction is a decimal too.
//! - Everything floating-point stays [`crate::warehouse::csv::FixtureType::Real`], a date stays
//!   [`crate::warehouse::csv::FixtureType::Date`], and an empty column is
//!   [`crate::warehouse::csv::FixtureType::Text`].
//!
//! This module lives in the domain - not beside any one adapter - because all three execution
//! crates depend on the domain and none may depend on another. It is a pure function over the CSV
//! text; nothing here reads a file or touches a data system.

use crate::{
    calendar::Date,
    model::{ColumnName, InvalidIdentifier},
};

/// The column types a fixture CSV can declare, and the one classification every adapter maps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureType {
    /// A case-insensitive `true`/`false` column.
    Boolean,
    /// A column of `i64` integers only (a `SUM` over it stays exact).
    Integer,
    /// A non-negative integer column that needs the shared `u64` range.
    WideInteger,
    /// An exact fixed-point decimal, carrying the widest canonical scale.
    ///
    /// An all-integer column is NOT this - it is [`Self::Integer`] or [`Self::WideInteger`] - because
    /// both stay exact. A column with an integer and a fraction is a decimal of the observed scale;
    /// a column of decimals only is a decimal too.
    Decimal { scale: u8 },
    /// Everything floating-point.
    Real,
    /// A `YYYY-MM-DD` column.
    Date,
    /// The fallback, for text and for a column with no data.
    Text,
}

/// One inferred column: its name, and the type every adapter should attach for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    name: ColumnName,
    kind: FixtureType,
}

impl Column {
    /// The column's name, parsed so it is safe to interpolate into a DDL statement.
    pub const fn name(&self) -> &ColumnName {
        &self.name
    }

    /// The type every adapter maps.
    pub const fn kind(&self) -> FixtureType {
        self.kind
    }
}

/// Why a fixture column could not be classified.
#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
    /// A header was not a column name.
    #[error(transparent)]
    InvalidIdentifier(#[from] InvalidIdentifier),
    /// Quoted CSV syntax has reader-specific semantics and is outside the shared fixture grammar.
    #[error("quoted CSV syntax is unsupported in fixtures")]
    UnsupportedQuotedSyntax,
    /// A fixed-point value or possible subtotal was wider than the exact shared type.
    #[error("column {column} contains fixed-point values this build cannot carry exactly")]
    DecimalNotCarryable { column: String },
    /// A data row did not have exactly the number of cells declared by the header.
    #[error("fixture row {row} has {found} cells, but the header declares {expected}")]
    RowWidth { row: usize, expected: usize, found: usize },
    /// Two headers name one column under a reader's case-insensitive lookup.
    #[error("fixture column {column} is declared more than once")]
    DuplicateColumn { column: String },
    /// A whitespace-only row is data for some readers and absent for others.
    #[error("fixture row {row} contains only whitespace")]
    WhitespaceOnlyRow { row: usize },
}

const CANDIDATES: &[FixtureType] = &[
    FixtureType::Boolean,
    FixtureType::Integer,
    FixtureType::WideInteger,
    FixtureType::Decimal { scale: 0 },
    FixtureType::Real,
    FixtureType::Date,
    FixtureType::Text,
];

/// Infers the type of each column of a fixture CSV.
///
/// Quoted syntax is refused before splitting, because the three readers do not give it one meaning.
/// Header names are then parsed as [`ColumnName`]s, so a column that maps to a DDL statement (the
/// `DuckDB` `types` argument, the engine's Arrow schema, Postgres's `CREATE TABLE`) cannot carry
/// another unparseable spelling. A malformed name is [`InvalidIdentifier`], the same refusal the
/// Postgres importer made.
pub fn infer(text: &str) -> Result<Vec<Column>, InferenceError> {
    if text.contains('"') {
        return Err(InferenceError::UnsupportedQuotedSyntax);
    }

    let mut lines = text.lines();
    let header = lines.next().unwrap_or_default();
    let names = split_row(header);
    let column_count = names.len();

    let mut parsed_names = Vec::with_capacity(column_count);
    for name in &names {
        let parsed = ColumnName::parse(name)?;
        // ponytail: fixture headers are tiny; use a normalized set if that changes.
        if parsed_names
            .iter()
            .any(|existing: &ColumnName| existing.as_str().eq_ignore_ascii_case(parsed.as_str()))
        {
            return Err(InferenceError::DuplicateColumn {
                column: String::from(parsed.as_str()),
            });
        }
        parsed_names.push(parsed);
    }

    let mut accumulators: Vec<Vec<String>> = names.iter().map(|_| Vec::new()).collect();
    for (index, line) in lines.enumerate() {
        if line.is_empty() {
            continue;
        }
        if line.trim().is_empty() {
            return Err(InferenceError::WhitespaceOnlyRow {
                row: index.saturating_add(2),
            });
        }
        let cells = split_row(line);
        if cells.len() != column_count {
            return Err(InferenceError::RowWidth {
                row: index.saturating_add(2),
                expected: column_count,
                found: cells.len(),
            });
        }
        for (index, cell) in cells.iter().enumerate() {
            if let Some(column) = accumulators.get_mut(index) {
                column.push(cell.clone());
            }
        }
    }

    parsed_names
        .into_iter()
        .zip(accumulators)
        .map(|(name, values)| {
            // The first candidate that holds every non-empty cell. Empty cells are neutral.
            let claimed = CANDIDATES
                .iter()
                .copied()
                .find(|candidate| values.iter().all(|cell| cell.is_empty() || candidate.holds(cell)));
            let kind = match claimed {
                // A column with no data gets the neutral type; letting the Boolean arm claim it
                // would be an arbitrary win for a type nothing needed.
                Some(claimed) if values.iter().any(|cell| !cell.is_empty()) => {
                    claimed.classify(&values).ok_or_else(|| InferenceError::DecimalNotCarryable {
                        column: String::from(name.as_str()),
                    })?
                }
                _ => FixtureType::Text,
            };
            Ok(Column { name, kind })
        })
        .collect()
}

/// Splits one CSV line on commas. The fixtures carry no quoted commas, embedded newlines or quotes,
/// which is the ceiling this parser accepts - growing a general parser here is the day a fixture
/// needs one, and this is the same boundary the Postgres importer held.
fn split_row(line: &str) -> Vec<String> {
    line.split(',').map(str::to_owned).collect()
}

impl FixtureType {
    /// Whether this type can hold the given non-empty cell.
    fn holds(self, value: &str) -> bool {
        match self {
            Self::Boolean => value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false"),
            Self::Integer => value.parse::<i64>().is_ok(),
            Self::WideInteger => value.parse::<u64>().is_ok(),
            // A fixed-point decimal (a dot and digits, no exponent), or a plain integer that a
            // decimal column also carries. Chosen before `Real`, so a column with any fractional
            // value stays exact instead of being widened to a double. A column of ONLY integers is
            // claimed by `Integer` first, so an integer value here is never the deciding one.
            Self::Decimal { .. } => !matches!(decimal_shape(value), DecimalShape::NotDecimal),
            Self::Real => is_shared_real(value),
            Self::Date => is_date(value),
            // Text is the fallback and can hold anything; chosen only after every narrower type has
            // been ruled out for at least one value in the column.
            Self::Text => true,
        }
    }

    /// The concrete type for a column this candidate claims, resolving the observed scale.
    fn classify(self, values: &[String]) -> Option<Self> {
        match self {
            Self::Decimal { .. } => {
                let scale = values
                    .iter()
                    .filter_map(|cell| match decimal_shape(cell) {
                        DecimalShape::Exact { scale, .. } => Some(scale),
                        DecimalShape::NotDecimal | DecimalShape::OutOfRange => None,
                    })
                    .max()
                    .unwrap_or(0);
                let mut possible_positive_total = 0_i128;
                let mut possible_negative_total = 0_i128;
                for cell in values.iter().filter(|cell| !cell.is_empty()) {
                    let DecimalShape::Exact {
                        precision,
                        scale: cell_scale,
                    } = decimal_shape(cell)
                    else {
                        return None;
                    };
                    if precision.saturating_add(usize::from(scale - cell_scale)) > 38 {
                        return None;
                    }
                    let total = if cell.starts_with('-') {
                        &mut possible_negative_total
                    } else {
                        &mut possible_positive_total
                    };
                    *total = total.checked_add(decimal_magnitude(cell, scale - cell_scale)?)?;
                    if *total >= 10_i128.pow(38) {
                        return None;
                    }
                }
                Some(Self::Decimal { scale })
            }
            // The other shapes are already concrete.
            kind => Some(kind),
        }
    }
}

fn is_shared_real(value: &str) -> bool {
    let Ok(parsed) = value.parse::<f64>() else {
        return false;
    };
    let has_nonzero_mantissa = value
        .bytes()
        .take_while(|byte| !matches!(byte, b'e' | b'E'))
        .any(|byte| matches!(byte, b'1'..=b'9'));
    parsed.is_finite() && (parsed != 0.0 || !has_nonzero_mantissa)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecimalShape {
    NotDecimal,
    Exact { precision: usize, scale: u8 },
    OutOfRange,
}

fn decimal_shape(value: &str) -> DecimalShape {
    let unsigned = unsigned_decimal(value);
    let Some((whole, fraction)) = unsigned.split_once('.') else {
        if unsigned.is_empty() || !unsigned.bytes().all(|byte| byte.is_ascii_digit()) {
            return DecimalShape::NotDecimal;
        }
        let precision = unsigned.trim_start_matches('0').len();
        return if precision <= 38 {
            DecimalShape::Exact { precision, scale: 0 }
        } else {
            DecimalShape::OutOfRange
        };
    };
    if (whole.is_empty() && fraction.is_empty())
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return DecimalShape::NotDecimal;
    }
    let fraction = fraction.trim_end_matches('0');
    let Ok(scale) = u8::try_from(fraction.len()) else {
        return DecimalShape::OutOfRange;
    };
    let precision = format!("{whole}{fraction}").trim_start_matches('0').len();
    if scale > 38 || precision > 38 {
        DecimalShape::OutOfRange
    } else {
        DecimalShape::Exact { precision, scale }
    }
}

fn decimal_magnitude(value: &str, padding: u8) -> Option<i128> {
    let unsigned = unsigned_decimal(value);
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let fraction = fraction.trim_end_matches('0');
    let magnitude = format!("{whole}{fraction}").parse::<i128>().ok()?;
    magnitude.checked_mul(10_i128.checked_pow(u32::from(padding))?)
}

fn unsigned_decimal(value: &str) -> &str {
    value.strip_prefix('-').or_else(|| value.strip_prefix('+')).unwrap_or(value)
}

/// A date value written exactly as `YYYY-MM-DD`.
fn is_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 {
        return false;
    }
    let has_exact_shape = bytes.get(4) == Some(&b'-')
        && bytes.get(7) == Some(&b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit());
    has_exact_shape && Date::parse(value).is_ok()
}

#[cfg(test)]
mod tests {
    use super::{FixtureType, infer};

    #[test]
    fn an_all_integer_column_stays_integer() {
        let columns = infer("k\n0\n1\n").expect("a valid header");
        assert_eq!(columns.len(), 1);
        assert_eq!(columns[0].kind, FixtureType::Integer);
    }

    #[test]
    fn a_fractional_column_is_a_decimal_not_a_double() {
        // The whole point of the module: a fraction stays exact on every wire, so the engine and
        // DuckDB do not drift to a float where Postgres holds `NUMERIC`.
        let columns = infer("amount\n2.40\n3.57\n").expect("a valid header");
        assert_eq!(columns[0].kind, FixtureType::Decimal { scale: 2 });
    }

    #[test]
    fn a_positive_sign_stays_decimal() {
        let columns = infer("amount\n+2.40\n+3.57\n").expect("a valid header");
        assert_eq!(columns[0].kind, FixtureType::Decimal { scale: 2 });
    }

    #[test]
    fn a_column_with_integers_and_a_fraction_is_a_decimal() {
        let columns = infer("amount\n100\n2.5\n").expect("a valid header");
        assert_eq!(columns[0].kind, FixtureType::Decimal { scale: 1 });
    }

    #[test]
    fn an_integer_past_i64_uses_the_shared_unsigned_range() {
        let columns = infer("amount\n10000000000000000000\n").expect("a valid header");
        assert_eq!(columns[0].kind, FixtureType::WideInteger);
    }

    #[test]
    fn a_fixed_point_value_past_the_shared_decimal_ceiling_is_refused() {
        assert!(matches!(
            infer("amount\n100000000000000000000000000000000000000\n"),
            Err(super::InferenceError::DecimalNotCarryable { .. })
        ));
    }

    #[test]
    fn a_wide_numeric_identifier_in_a_text_column_stays_text() {
        let columns = infer("id\n100000000000000000000000000000000000000\nABC\n").expect("a valid text column");
        assert_eq!(columns[0].kind, FixtureType::Text);
    }

    #[test]
    fn a_decimal_that_overflows_at_the_columns_scale_is_refused() {
        assert!(matches!(
            infer("amount\n99999999999999999999999999999999999999\n0.1\n"),
            Err(super::InferenceError::DecimalNotCarryable { .. })
        ));
    }

    #[test]
    fn a_decimal_whose_possible_total_overflows_is_refused() {
        assert!(matches!(
            infer("amount\n90000000000000000000000000000000000000\n90000000000000000000000000000000000000\n"),
            Err(super::InferenceError::DecimalNotCarryable { .. })
        ));
    }

    #[test]
    fn opposite_sign_decimal_totals_are_bounded_separately() {
        let columns = infer("amount\n90000000000000000000000000000000000000.0\n-90000000000000000000000000000000000000.0\n")
            .expect("neither signed subtotal exceeds the decimal range");
        assert_eq!(columns[0].kind, FixtureType::Decimal { scale: 0 });
    }

    #[test]
    fn a_date_column_is_a_date() {
        let columns = infer("day\n2026-01-01\n2026-01-02\n").expect("a valid header");
        assert_eq!(columns[0].kind, FixtureType::Date);
    }

    #[test]
    fn a_text_column_stays_text() {
        let columns = infer("region\neast\nnorth\n").expect("a valid header");
        assert_eq!(columns[0].kind, FixtureType::Text);
    }

    #[test]
    fn a_boolean_column_is_a_boolean() {
        let columns = infer("flag\ntrue\nFALSE\nTrUe\n").expect("a valid header");
        assert_eq!(columns[0].kind, FixtureType::Boolean);
    }

    #[test]
    fn scientific_notation_is_a_real() {
        let columns = infer("amount\n1e2\n2e2\n").expect("a valid header");
        assert_eq!(columns[0].kind, FixtureType::Real);
    }

    #[test]
    fn quoted_syntax_is_refused_before_it_can_change_cell_meaning() {
        for text in ["value\n\"\"\n", "value\n\"1,2\"\n", "\"value\"\n1\n"] {
            assert!(matches!(infer(text), Err(super::InferenceError::UnsupportedQuotedSyntax)));
        }
    }

    #[test]
    fn a_real_must_stay_finite_and_not_underflow_across_readers() {
        for value in ["1e309", "1e-999"] {
            let columns = infer(&format!("amount\n{value}\n")).expect("a valid header");
            assert_eq!(columns[0].kind, FixtureType::Text, "`{value}`");
        }

        let columns = infer("amount\n0e-999\n").expect("an exact zero is shared");
        assert_eq!(columns[0].kind, FixtureType::Real);
    }

    #[test]
    fn arrow_boolean_shorthands_are_text() {
        let columns = infer("flag\nt\nf\n").expect("a valid header");
        assert_eq!(columns[0].kind, FixtureType::Text);
    }

    #[test]
    fn an_empty_column_is_text() {
        let columns = infer("only\n\n\n").expect("a valid header");
        assert_eq!(columns[0].kind, FixtureType::Text);
    }

    #[test]
    fn an_unparseable_header_is_refused() {
        infer("orders.amount\n1\n").expect_err("a qualified header is not a column name");
    }

    #[test]
    fn a_ragged_row_is_refused_in_both_directions() {
        for text in ["a,b\n1\n", "a,b\n1,2,3\n"] {
            assert!(matches!(infer(text), Err(super::InferenceError::RowWidth { row: 2, .. })));
        }
    }

    #[test]
    fn headers_are_unique_under_the_readers_case_policy() {
        for text in ["a,a\n1,2\n", "a,A\n1,2\n"] {
            assert!(matches!(
                infer(text),
                Err(super::InferenceError::DuplicateColumn { column }) if column == "a" || column == "A"
            ));
        }
    }

    #[test]
    fn a_whitespace_only_row_is_outside_the_shared_grammar() {
        assert!(matches!(
            infer("only\n \n"),
            Err(super::InferenceError::WhitespaceOnlyRow { row: 2 })
        ));
    }

    #[test]
    fn year_zero_is_text() {
        let columns = infer("day\n0000-01-01\n").expect("a valid header");
        assert_eq!(columns[0].kind, FixtureType::Text);
    }

    #[test]
    fn an_impossible_calendar_day_is_text() {
        let columns = infer("day\n2026-02-30\n").expect("a valid header");
        assert_eq!(columns[0].kind, FixtureType::Text);
    }
}
