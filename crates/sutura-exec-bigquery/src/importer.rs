//! Turning a committed fixture CSV into one `BigQuery` table, for the corpus acceptance leg.
//!
//! **This is #78's shape, and the differences from it are the interesting part.** The Postgres
//! adapter's `importer` infers a column type per column from the committed bytes and renders
//! `CREATE TABLE` plus `COPY ... FROM STDIN`. That second half is what does not transfer: there is no
//! `COPY` here, and no way to hand the endpoint a CSV body beside a statement without either a load
//! job or `tabledata.insertAll`, each of which is a second wire surface nobody has run. So the rows
//! travel **inside the statement**, and that changes what this module has to be careful about.
//!
//! # Every value is re-rendered from a PARSED one, and the string arm refuses rather than escapes
//!
//! A fixture CSV is a document read off disk, which this repository treats as untrusted input - the
//! *panicking fragment API* row exists because an abort was reachable from a catalog file. A loader
//! that interpolated cells into SQL and escaped quotes would be one escaping bug away from executing
//! whatever a CSV says, and escaping is exactly the class of check that is *"moved rather than
//! answered"*.
//!
//! So nothing reaches the statement as it was written. [`ColumnType::literal`] parses each cell into a
//! Rust value first and renders THAT: an integer through `i64`, a double through `f64` with
//! non-finite refused, a boolean matched to one of two keywords, a date through the same shape check
//! the type inference used. **The one arm with no parse to hide behind is text, and it refuses
//! instead:** a cell outside `[A-Za-z0-9 _.-]` is [`FixtureNotUsable::Cell`] rather than a quoted
//! string, so a value carrying `'`, `` ` ``, `\` or a newline is unrepresentable in a rendered
//! statement rather than escaped into one. The whole example corpus is inside that set; the day a
//! fixture is not, somebody decides what to do about it in a diff rather than discovering it at the
//! endpoint.
//!
//! The same argument covers the identifiers, and it is #78's: the header is a CSV line interpolated
//! into `CREATE TABLE`, so every name is parsed as a [`ColumnName`] first and the table as a
//! [`TableName`] by its caller.
//!
//! # Why the whole load is ONE statement
//!
//! `CREATE OR REPLACE TABLE` then `INSERT` would be two jobs per table, and the table would exist
//! empty between them - a state a concurrent corpus run could read. `CREATE OR REPLACE TABLE ... AS
//! SELECT ... FROM UNNEST([STRUCT ...])` is one job, atomic in the way `CREATE OR REPLACE` is, and
//! it is idempotent against the last run. Four tables, four jobs, and the DDL costs nothing: the
//! bytes billed for a `CREATE TABLE AS SELECT` over a literal array are the bytes it scans, which is
//! none.
//!
//! The first element carries the explicit `STRUCT<...>` type and the rest are bare tuples that
//! coerce to it, which is what pins the column types - an array of bare tuples would let `GoogleSQL`
//! infer them, and inferring `INT64` for a column whose fixture happens to hold no decimal is how a
//! type drifts between the two sides of a differential.

use sutura_domain::model::{ColumnName, InvalidIdentifier, TableName};

/// Every column type this importer will declare.
///
/// The five `BigQuery` types [`crate::transport::FieldType`] maps, and no others - so a fixture
/// cannot produce a table whose columns come back as `Unmapped`. `NUMERIC` is deliberately absent:
/// nothing in the corpus needs an exact decimal, and a type nothing exercises reads as coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColumnType {
    Bool,
    Int64,
    Float64,
    Date,
    String,
}

/// The inference order, narrowest first.
///
/// A column takes the first type that holds every non-empty cell in it. Two orderings matter.
/// `Bool` before `Int64`: `0` and `1` are not booleans here, because `Bool::holds` accepts only the
/// two keywords. And `Int64` before `Float64`: `"1"` parses as both, and an integer column that came
/// back as a double would disagree with the engine's own CSV inference.
const NARROWEST_FIRST: [ColumnType; 5] = [
    ColumnType::Bool,
    ColumnType::Int64,
    ColumnType::Float64,
    ColumnType::Date,
    ColumnType::String,
];

impl ColumnType {
    /// The `GoogleSQL` spelling, as a `STRUCT` field type.
    const fn sql(self) -> &'static str {
        match self {
            Self::Bool => "BOOL",
            Self::Int64 => "INT64",
            Self::Float64 => "FLOAT64",
            Self::Date => "DATE",
            Self::String => "STRING",
        }
    }

    /// Whether this type can hold the given non-empty cell.
    fn holds(self, value: &str) -> bool {
        match self {
            Self::Bool => matches!(value, "true" | "false"),
            Self::Int64 => value.parse::<i64>().is_ok(),
            Self::Float64 => value.parse::<f64>().is_ok_and(f64::is_finite),
            Self::Date => is_iso_date(value),
            // The fallback, reached only after every narrower type has been ruled out for at least
            // one cell. It still refuses in `literal`, which is where the charset is enforced.
            Self::String => true,
        }
    }

    /// One cell as a `GoogleSQL` literal, rendered from a value this function parsed.
    ///
    /// **Nothing in the returned text comes from `value` except through a parse**, which is the whole
    /// argument of this module's header. The `String` arm has no parse, so it has a charset instead.
    fn literal(self, value: &str, at: Cell) -> Result<String, FixtureNotUsable> {
        if value.is_empty() {
            return Ok(String::from("NULL"));
        }
        match self {
            Self::Bool => match value {
                "true" => Ok(String::from("TRUE")),
                "false" => Ok(String::from("FALSE")),
                _ => Err(at.not_a("boolean")),
            },
            // Rendered from the parsed number, so the text is digits and an optional sign whatever
            // the cell spelled - `+1`, `01` and `1` all render as `1`.
            Self::Int64 => value
                .parse::<i64>()
                .map(|parsed| parsed.to_string())
                .map_err(|_ignored| at.not_a("64-bit integer")),
            // `{}` on an `f64` is Rust's shortest round-tripping form, so the rendered text parses
            // back to the same double. Non-finite is refused rather than rendered: `inf` is not a
            // `GoogleSQL` literal, and a fixture holding one is a fixture to fix.
            Self::Float64 => match value.parse::<f64>() {
                Ok(parsed) if parsed.is_finite() => Ok(parsed.to_string()),
                Ok(_) | Err(_) => Err(at.not_a("finite double")),
            },
            // Re-checked rather than trusted: inference ran over the whole column and this runs over
            // this cell, and the ten characters that pass are digits and two hyphens.
            Self::Date => {
                if is_iso_date(value) {
                    Ok(format!("DATE '{value}'"))
                } else {
                    Err(at.not_a("ISO date"))
                }
            }
            // **The refusal, not an escape.** See the module header.
            Self::String => {
                if value.chars().all(is_plain) {
                    Ok(format!("'{value}'"))
                } else {
                    Err(at.not_a("plain text value"))
                }
            }
        }
    }
}

/// A value shaped `YYYY-MM-DD`.
///
/// A shape check and not a calendar: `2026-13-40` passes here and the endpoint refuses it, which is
/// the right division of labour - this module decides what may reach a statement, and what a date
/// MEANS belongs to whoever stores it.
fn is_iso_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(index, byte)| {
            if index == 4 || index == 7 {
                *byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
}

/// Whether a character may appear inside a rendered string literal.
///
/// Deliberately narrow: no quote of any kind, no backslash, no control character, nothing outside
/// ASCII. The example corpus's text columns are region and segment names, product names, customer
/// ids and contract terms, all of which are inside this.
const fn is_plain(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '.' | '-')
}

/// Where in the fixture a refusal happened.
///
/// **A position and never the value**, which is this repository's rule for a parse refusal: the
/// column INDEX and the row INDEX are enough to find the cell in the file, and the cell's own text
/// stays out of an error that a log will print.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cell {
    row: usize,
    column: usize,
}

impl Cell {
    const fn at(row: usize, column: usize) -> Self {
        Self { row, column }
    }

    const fn not_a(self, expected: &'static str) -> FixtureNotUsable {
        FixtureNotUsable::Cell {
            row: self.row,
            column: self.column,
            expected,
        }
    }
}

/// Why a committed fixture cannot become a table.
///
/// Every variant is a defect in a file in this repository rather than an input to handle, which is
/// why none of them carries the offending text: whoever sees one has the file.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FixtureNotUsable {
    /// A CSV header field is not a column name.
    #[error("a fixture header field is not a column name")]
    Header {
        #[source]
        cause: InvalidIdentifier,
    },
    /// The file has no header line, so there is no schema to build.
    #[error("a fixture has no header line")]
    NoHeader,
    /// The file has a header and no data, so the table would be empty and the corpus would compare
    /// nothing.
    #[error("a fixture has a header and no rows")]
    NoRows,
    /// A cell cannot be rendered as the type its column was inferred to be.
    #[error("row {row}, column {column} of a fixture is not a {expected}")]
    Cell {
        row: usize,
        column: usize,
        expected: &'static str,
    },
}

/// One fixture table: its columns, and its rows already rendered as literals.
#[derive(Debug)]
pub(crate) struct Fixture {
    columns: Vec<(String, ColumnType)>,
    /// One entry per data row, each already a parenthesised tuple of literals.
    tuples: Vec<String>,
}

impl Fixture {
    /// How many data rows this fixture carries, which is what a caller asserts a load moved.
    pub(crate) const fn rows(&self) -> usize {
        self.tuples.len()
    }

    /// The single statement that replaces the table with this fixture's rows.
    ///
    /// One `CREATE OR REPLACE TABLE ... AS SELECT * FROM UNNEST([...])`. The module header says why
    /// it is one statement and why the first element carries the explicit `STRUCT` type.
    ///
    /// The table is named UNQUALIFIED, so it resolves in the job's `defaultDataset` - the same
    /// resolution every statement this adapter renders relies on, which is what keeps a dataset name
    /// out of a statement and therefore out of any log.
    pub(crate) fn create_statement(&self, table: &TableName) -> String {
        let declared: Vec<String> = self
            .columns
            .iter()
            .map(|&(ref name, kind)| format!("`{name}` {}", kind.sql()))
            .collect();
        // Built by concatenation rather than by `write!`, because `write!` into a `String` returns a
        // `Result` that cannot fail and that the workspace's lint table gives no clean way to discard.
        let mut out = format!(
            "CREATE OR REPLACE TABLE `{}` AS SELECT * FROM UNNEST([STRUCT<{}>",
            table.as_str(),
            declared.join(", ")
        );
        for (index, tuple) in self.tuples.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            out.push_str(tuple);
        }
        out.push_str("])");
        out
    }
}

/// Splits one CSV line on commas.
///
/// The fixtures carry no quoted commas, embedded newlines or quotes, which is the ceiling this
/// parser accepts - #78 states the same boundary, and here it is doubled up on: a cell that DID
/// carry a quote would be refused by [`ColumnType::literal`] rather than mis-split into a statement.
fn split_row(line: &str) -> Vec<String> {
    line.split(',').map(|cell| cell.trim().to_owned()).collect()
}

/// Reads a fixture's text into a schema and a set of rendered rows.
///
/// Runs from the committed bytes every time, so a fixture cannot drift from the CSV on disk - which
/// is #78's reason for inferring rather than declaring, and it matters more here: the engine side of
/// the differential reads the same file through its own CSV inference, so a hand-written schema would
/// be a second opinion about the same bytes.
pub(crate) fn read_fixture(text: &str) -> Result<Fixture, FixtureNotUsable> {
    let mut lines = text.lines();
    let header = lines.next().ok_or(FixtureNotUsable::NoHeader)?;
    let names = split_row(header);
    let width = names.len();
    for name in &names {
        drop(ColumnName::parse(name).map_err(|cause| FixtureNotUsable::Header { cause })?);
    }

    // Two passes over the rows, because a type cannot be decided from one row: the first collects
    // the cells per column, the second renders them once the column's type is known.
    let mut grid: Vec<Vec<String>> = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let mut cells = split_row(line);
        // Padded or truncated to the header's width, so a ragged row cannot shift a value into a
        // later column - #78's reasoning, and it is the same defect here.
        cells.resize(width, String::new());
        grid.push(cells);
    }
    if grid.is_empty() {
        return Err(FixtureNotUsable::NoRows);
    }

    let mut columns: Vec<(String, ColumnType)> = Vec::with_capacity(width);
    for (index, name) in names.into_iter().enumerate() {
        let seen: Vec<&str> = grid
            .iter()
            .filter_map(|row| row.get(index))
            .map(String::as_str)
            .filter(|cell| !cell.is_empty())
            .collect();
        let kind = if seen.is_empty() {
            // A column with no data gets the neutral type. Letting the `Bool` arm claim it would be
            // an arbitrary win for a type nothing needed - #78's words, and the same choice.
            ColumnType::String
        } else {
            NARROWEST_FIRST
                .into_iter()
                .find(|kind| seen.iter().all(|cell| kind.holds(cell)))
                .unwrap_or(ColumnType::String)
        };
        columns.push((name, kind));
    }

    let mut tuples: Vec<String> = Vec::with_capacity(grid.len());
    for (row, cells) in grid.iter().enumerate() {
        let mut rendered: Vec<String> = Vec::with_capacity(width);
        for (column, cell) in cells.iter().enumerate() {
            let kind = columns.get(column).map_or(ColumnType::String, |&(_, kind)| kind);
            rendered.push(kind.literal(cell, Cell::at(row, column))?);
        }
        tuples.push(format!("({})", rendered.join(", ")));
    }

    Ok(Fixture { columns, tuples })
}

/// Why a fixture did not reach the dataset.
///
/// Two shapes rather than one, because a defect in a file in this repository and a refusal from the
/// endpoint are different problems for whoever reads the failure: the first is fixed in a diff and
/// the second is a grant, a quota or a dataset that is not there.
#[derive(Debug, thiserror::Error)]
pub enum FixtureNotLoaded<E>
where
    E: core::error::Error + 'static,
{
    /// The committed CSV could not be read. The PATH is carried because a fixture path is a path
    /// inside this repository, which is the one class of path this repository does write down.
    #[error("the fixture at {path} could not be read")]
    Unreadable {
        path: String,
        #[source]
        cause: std::io::Error,
    },
    /// The committed CSV cannot become a table.
    #[error("the fixture at {path} cannot become a table")]
    NotUsable {
        path: String,
        #[source]
        cause: FixtureNotUsable,
    },
    /// The endpoint refused the load.
    #[error("the endpoint did not load the fixture table")]
    Endpoint {
        #[source]
        cause: E,
    },
}

/// What one load answers with: the row count, or why it did not happen.
///
/// Named because `Result<usize, FixtureNotLoaded<T::Error>>` is over the `type_complexity` threshold
/// this workspace tightened, and for the reason `crate::Mapped` is named: the generic error is the
/// point, and erasing it would lose which transport failed.
pub type Loaded<E> = Result<usize, FixtureNotLoaded<E>>;

#[cfg(test)]
mod tests {
    use super::{ColumnType, FixtureNotUsable, read_fixture};

    const ROWS: &str = "month,subscription_key,status,mrr_cents,churned_in_month,data_gb\n\
        2026-01-01,1001,active,3749,false,2.47\n\
        2026-02-01,1002,active,5000,true,3.5\n";

    #[test]
    fn infers_the_fixture_column_shapes() {
        let fixture = read_fixture(ROWS).expect("the fixture headers are column names");
        let kinds: Vec<&str> = fixture.columns.iter().map(|&(_, kind)| kind.sql()).collect();
        assert_eq!(kinds, ["DATE", "INT64", "STRING", "INT64", "BOOL", "FLOAT64"]);
    }

    #[test]
    fn the_statement_declares_the_struct_once_and_carries_every_row() {
        let fixture = read_fixture(ROWS).expect("the fixture headers are column names");
        let table = sutura_domain::model::TableName::parse("fct").expect("a table name parses");
        let sql = fixture.create_statement(&table);
        assert!(
            sql.starts_with(
                "CREATE OR REPLACE TABLE `fct` AS SELECT * FROM UNNEST([STRUCT<`month` DATE, \
                 `subscription_key` INT64, `status` STRING, `mrr_cents` INT64, \
                 `churned_in_month` BOOL, `data_gb` FLOAT64>"
            ),
            "{sql}"
        );
        assert!(
            sql.contains("(DATE '2026-01-01', 1001, 'active', 3749, FALSE, 2.47)"),
            "{sql}"
        );
        assert!(sql.contains("(DATE '2026-02-01', 1002, 'active', 5000, TRUE, 3.5)"), "{sql}");
        assert!(sql.ends_with("])"), "{sql}");
        assert_eq!(fixture.rows(), 2);
    }

    #[test]
    fn an_empty_cell_becomes_null_rather_than_an_empty_string() {
        let fixture = read_fixture("k,v\n1,\n2,7\n").expect("a valid header");
        assert!(
            fixture.tuples.iter().any(|tuple| tuple == "(1, NULL)"),
            "{:?}",
            fixture.tuples
        );
    }

    #[test]
    fn a_number_only_column_stays_an_integer_and_zero_is_not_a_boolean() {
        let fixture = read_fixture("k\n0\n1\n").expect("a valid header");
        assert_eq!(fixture.columns.first().map(|&(_, kind)| kind), Some(ColumnType::Int64));
    }

    #[test]
    fn a_header_that_is_not_a_column_name_is_refused() {
        assert!(matches!(
            read_fixture("orders.amount\n1\n"),
            Err(FixtureNotUsable::Header { .. })
        ));
        assert!(matches!(read_fixture("`quoted`\n1\n"), Err(FixtureNotUsable::Header { .. })));
    }

    /// **The one this module exists for.** A text cell carrying a quote, a backtick, a backslash or a
    /// statement terminator is REFUSED rather than escaped, so no rendered statement can carry it.
    /// The refusal names the position and not the value.
    #[test]
    fn a_text_cell_that_could_close_a_literal_is_refused_and_the_refusal_names_no_value() {
        for hostile in [
            "k\nO'Brien\n",
            "k\n'); DROP TABLE x --\n",
            "k\nback`tick\n",
            "k\nback\\slash\n",
            "k\nnon-ascii-\u{00e9}\n",
        ] {
            let refused = read_fixture(hostile).expect_err("a hostile cell is refused");
            assert_eq!(
                refused,
                FixtureNotUsable::Cell {
                    row: 0,
                    column: 0,
                    expected: "plain text value"
                },
                "{hostile:?}"
            );
            let printed = refused.to_string();
            assert!(!printed.contains("DROP"), "{printed}");
            assert!(!printed.contains('\''), "{printed}");
        }
    }

    #[test]
    fn a_non_finite_double_is_refused_rather_than_rendered() {
        // `inf` is not a GoogleSQL literal, and `Float64::holds` refuses it - so the column falls
        // through to `String`, where the charset then accepts it as text. That is the honest outcome
        // and it is asserted rather than left implied: what must NOT happen is a column typed
        // FLOAT64 with `inf` written into the statement.
        let fixture = read_fixture("v\ninf\n").expect("a valid header");
        assert_eq!(fixture.columns.first().map(|&(_, kind)| kind), Some(ColumnType::String));
    }

    #[test]
    fn a_fixture_with_no_rows_is_refused_rather_than_creating_an_empty_table() {
        // A green corpus over an empty table is the shape this repository calls a cell that cannot
        // fail, so an empty fixture is a refusal at the loader rather than four empty tables.
        assert!(matches!(read_fixture("k,v\n"), Err(FixtureNotUsable::NoRows)));
        assert!(matches!(read_fixture(""), Err(FixtureNotUsable::NoHeader)));
    }

    #[test]
    fn a_ragged_row_is_padded_rather_than_shifted() {
        let fixture = read_fixture("a,b,c\n1,2,3\n4,5\n").expect("a valid header");
        assert!(
            fixture.tuples.iter().any(|tuple| tuple == "(4, 5, NULL)"),
            "{:?}",
            fixture.tuples
        );
    }
}
