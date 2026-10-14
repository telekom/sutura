//! The fixture tier: its credential, a private database per open, and a corpus CSV as a table.
//!
//! Behind the default-off `fixtures` feature, like `sutura_exec_postgres::fixture`, and for its
//! reason: nothing here belongs in a composition root, and `--all-features` compiles, lints and
//! tests it on every run.
//!
//! # The credential is configured or refused by name
//!
//! `nix/clickhouse-tier.nix` generates a password per start and prints the two exports from
//! `sutura-clickhouse-tier credentials`; `nix/with-tier.sh` and `checks.nextest` evaluate them where
//! they export `SUTURA_DEV_REQUIRE_TIER`. Nothing here defaults a value - the shape
//! `sutura_exec_postgres::fixture` records the defect of - so an unset or blank variable is a
//! [`UnconfiguredFixture`] naming it.
//!
//! # Column types: the golden matrix's, not the conformance path's
//!
//! [`sutura_domain::warehouse::csv::infer`] classifies each column. Its `Decimal` is attached as
//! `Float64` HERE, deliberately: the matrix's other adapters attach a fractional column as a
//! double (`DuckDB`'s `read_csv_auto`, `sutura_exec_postgres`'s `load_csv`, the engine's Arrow
//! inference), so a `Decimal` column here would answer the same question as text rather than as a
//! real and differ from all three for a reason that is the importer's, not the server's.
//!
//! **No column is `Nullable`, and an empty cell is refused.** `transport`'s `JOIN_USE_NULLS` is
//! measured to matter only over a non-`Nullable` schema, so a nullable importer would make the
//! executed goldens blind to that setting. And a non-`Nullable` column reads an empty CSV cell as
//! the type's default - `0`, `''` - where every other adapter reads `NULL`: a silently different
//! fixture. No golden-matrix fixture has one; a future one is refused rather than rewritten.
//!
//! The federated corpus holds one absent key on purpose, so [`ClickHouseWarehouse::load_federated_csv`]
//! declares `Nullable` exactly the columns holding an empty cell, at the matrix's type, and every
//! other column as the matrix does.

use std::collections::BTreeSet;
use std::path::Path;

use sutura_domain::identity::Secret;
use sutura_domain::model::{SourceName, TableName};
use sutura_domain::source::SourcePosture;
use sutura_domain::warehouse::csv::{self, FixtureType, InferenceError};

use crate::ClickHouseWarehouse;
use crate::transport::{BasicAuth, Endpoint, Http, HttpError};

/// One of the two values the `ClickHouse` fixture tier publishes into the environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FixtureVariable {
    /// The user the tier's `users.xml` declares.
    User,
    /// That user's password, generated per start by `nix/clickhouse-tier.nix`.
    Password,
}

impl FixtureVariable {
    /// Every variable, in the order [`credential_from_env`] reads them.
    pub const ALL: [Self; 2] = [Self::User, Self::Password];

    /// The environment variable's name, spelled once.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::User => "SUTURA_CLICKHOUSE_TIER_USER",
            Self::Password => "SUTURA_CLICKHOUSE_TIER_PASSWORD",
        }
    }
}

impl core::fmt::Display for FixtureVariable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name())
    }
}

/// Why there is no fixture credential to connect with. No variant carries a substitute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UnconfiguredFixture {
    #[error(
        "{0} is unset, so this worktree has no ClickHouse fixture credential. The tier publishes one: \
         run the suite with `just test`, which brings the tier up and exports what \
         `just clickhouse-tier credentials` prints."
    )]
    Unset(FixtureVariable),
    #[error(
        "{0} is set to an empty value, which is not a credential. Re-provision the tier - \
         `just clickhouse-tier stop` then `just test` - so it publishes one."
    )]
    Blank(FixtureVariable),
}

/// This process's fixture credential, as the tier exported it.
pub fn credential_from_env() -> Result<BasicAuth, UnconfiguredFixture> {
    parse(|variable| std::env::var(variable.name()).ok())
}

/// The parse over a lookup rather than the process environment, because `std::env::set_var` is
/// `unsafe` in Rust 2024 and this workspace forbids `unsafe` - so the refusals stay provokable.
fn parse(lookup: impl Fn(FixtureVariable) -> Option<String>) -> Result<BasicAuth, UnconfiguredFixture> {
    let present = |variable: FixtureVariable| match lookup(variable) {
        None => Err(UnconfiguredFixture::Unset(variable)),
        Some(value) if value.trim().is_empty() => Err(UnconfiguredFixture::Blank(variable)),
        Some(value) => Ok(value),
    };
    let user = present(FixtureVariable::User)?;
    let password = Secret::new(present(FixtureVariable::Password)?);
    Ok(BasicAuth::new(user, password))
}

/// Why the fixture tier could not be given a table.
#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    /// A database name reaches `CREATE DATABASE` as an identifier, so it has to be a word.
    #[error("{database:?} is not a word, so it cannot name a fixture database")]
    InvalidDatabaseName { database: String },
    #[error("could not read the fixture {path}")]
    Read {
        path: String,
        #[source]
        cause: std::io::Error,
    },
    #[error("the fixture {path} has no column types this importer can declare")]
    Schema {
        path: String,
        #[source]
        cause: InferenceError,
    },
    /// See the module header: a non-`Nullable` column would read this cell as a type default.
    #[error("the fixture {path} has an empty cell on line {line}, which a non-Nullable column would read as its type's default")]
    EmptyCell { path: String, line: usize },
    #[error("the server refused the fixture statement for {subject}")]
    Server {
        subject: String,
        #[source]
        cause: HttpError,
    },
}

impl ClickHouseWarehouse<Http> {
    /// Opens the adapter over `endpoint`, resolving every unqualified table name in `database`.
    ///
    /// The database is created here if absent, so several opens can share one server without
    /// clobbering each other's tables. `sutura_exec_postgres::fixture::FixtureCredential::conninfo_in`'s
    /// shape, over a database because that is `ClickHouse`'s namespace for a table.
    pub fn connect_in_database(
        source: SourceName,
        posture: SourcePosture,
        endpoint: Endpoint,
        auth: BasicAuth,
        database: &str,
        result_budget: sutura_domain::warehouse::ResultBudget,
    ) -> Result<Self, FixtureError> {
        if database.is_empty() || !database.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(FixtureError::InvalidDatabaseName {
                database: String::from(database),
            });
        }
        let transport = Http::connect(endpoint, Some(auth), result_budget.bytes() as u64);
        transport
            .command(&format!("CREATE DATABASE IF NOT EXISTS \"{database}\""), &[])
            .map_err(|cause| FixtureError::Server {
                subject: format!("database {database}"),
                cause,
            })?;
        Ok(Self::of(
            source,
            posture,
            transport.in_database(String::from(database)),
            result_budget,
        ))
    }

    /// Exposes a fixture CSV as a table.
    ///
    /// Columns are typed by [`csv::infer`] (see the module header for the one deliberate
    /// departure), the table is recreated, then the file is sent as the body of an
    /// `INSERT ... FORMAT CSVWithNames` the SERVER parses against those types.
    pub fn load_csv(&self, table: &TableName, path: &Path) -> Result<(), FixtureError> {
        self.load_csv_with_types(table, path, Columns::Matrix)
    }

    /// [`Self::load_csv`], except a column holding an empty cell is `Nullable`, so the cell is NULL.
    pub fn load_federated_csv(&self, table: &TableName, path: &Path) -> Result<(), FixtureError> {
        self.load_csv_with_types(table, path, Columns::Federated)
    }

    /// The conformance corpus keeps exact Decimal columns and its empty cells as NULL.
    pub fn load_conformance_csv(&self, table: &TableName, path: &Path) -> Result<(), FixtureError> {
        self.load_csv_with_types(table, path, Columns::Conformance)
    }

    fn load_csv_with_types(&self, table: &TableName, path: &Path, load: Columns) -> Result<(), FixtureError> {
        let shown = || path.display().to_string();
        let text = std::fs::read_to_string(path).map_err(|cause| FixtureError::Read { path: shown(), cause })?;
        if load == Columns::Matrix
            && let Some(line) = first_empty_cell(&text)
        {
            return Err(FixtureError::EmptyCell { path: shown(), line });
        }
        let declared = declared_columns(&text, load).map_err(|cause| FixtureError::Schema { path: shown(), cause })?;
        let server = |cause| FixtureError::Server {
            subject: format!("table {table}"),
            cause,
        };
        let create = format!(
            "CREATE OR REPLACE TABLE \"{}\" ({}) ENGINE = MergeTree ORDER BY tuple()",
            table.as_str(),
            declared.join(", ")
        );
        self.transport.command(&create, &[]).map_err(server)?;
        let insert = format!("INSERT INTO \"{}\" FORMAT CSVWithNames", table.as_str());
        self.transport.command(&insert, text.as_bytes()).map_err(server)
    }
}

/// How `load_csv_with_types` declares a fixture's columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Columns {
    /// The golden matrix's types, none `Nullable`, and an empty cell refused.
    Matrix,
    /// The matrix's types, `Nullable` on exactly the columns holding an empty cell.
    Federated,
    /// Exact decimals, every column `Nullable`.
    Conformance,
}

/// Each column of a fixture CSV as `CREATE TABLE` declares it under `load`.
fn declared_columns(text: &str, load: Columns) -> Result<Vec<String>, InferenceError> {
    let nullable = if load == Columns::Federated {
        columns_with_an_empty_cell(text)
    } else {
        BTreeSet::new()
    };
    Ok(csv::infer(text)?
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let kind = match (load, column.kind()) {
                (Columns::Conformance, FixtureType::Decimal { scale }) => format!("Nullable(Decimal(38,{scale}))"),
                (Columns::Conformance, other) => format!("Nullable({})", clickhouse_type(other)),
                (_, other) if nullable.contains(&index) => format!("Nullable({})", clickhouse_type(other)),
                (_, other) => String::from(clickhouse_type(other)),
            };
            format!("\"{}\" {kind}", column.name().as_str())
        })
        .collect())
}

/// The column type attached for one shared fixture classification.
const fn clickhouse_type(kind: FixtureType) -> &'static str {
    match kind {
        FixtureType::Boolean => "Bool",
        FixtureType::Integer => "Int64",
        FixtureType::WideInteger => "UInt64",
        // A double, not `Decimal(38, scale)`: see the module header.
        FixtureType::Decimal { .. } | FixtureType::Real => "Float64",
        FixtureType::Date => "Date",
        FixtureType::Text => "String",
    }
}

/// The 1-based line of the first data row carrying an empty cell, if any.
fn first_empty_cell(text: &str) -> Option<usize> {
    text.lines()
        .enumerate()
        .skip(1)
        .filter(|(_, line)| !line.is_empty())
        .find(|(_, line)| line.split(',').any(str::is_empty))
        .map(|(index, _)| index.saturating_add(1))
}

/// The 0-based index of every column some data row leaves empty.
fn columns_with_an_empty_cell(text: &str) -> BTreeSet<usize> {
    text.lines()
        .skip(1)
        .filter(|line| !line.is_empty())
        .flat_map(|line| {
            line.split(',')
                .enumerate()
                .filter(|(_, cell)| cell.is_empty())
                .map(|(index, _)| index)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Columns, FixtureVariable, UnconfiguredFixture, declared_columns, first_empty_cell, parse};

    #[test]
    fn an_absent_or_blank_variable_is_refused_and_named() {
        for withheld in FixtureVariable::ALL {
            let absent = parse(|variable| (variable != withheld).then(|| String::from("configured")));
            assert_eq!(absent.err(), Some(UnconfiguredFixture::Unset(withheld)));
            let blank = parse(|variable| Some(String::from(if variable == withheld { " " } else { "configured" })));
            assert_eq!(blank.err(), Some(UnconfiguredFixture::Blank(withheld)));
        }
    }

    #[test]
    fn an_empty_cell_is_found_on_the_line_it_is_on() {
        assert_eq!(first_empty_cell("a,b\n1,2\n3,\n"), Some(3));
        assert_eq!(first_empty_cell("a,b\n1,2\n,4\n"), Some(3));
        assert_eq!(first_empty_cell("a,b\n1,2\n\n3,4\n"), None);
    }

    #[test]
    fn a_federated_load_declares_nullable_only_the_columns_holding_an_empty_cell() {
        let text = "a,b,c\n1,,x\n\n4,5,\n";
        let declared = |load| declared_columns(text, load).expect("the columns classify");
        assert_eq!(
            declared(Columns::Federated),
            ["\"a\" Int64", "\"b\" Nullable(Int64)", "\"c\" Nullable(String)"]
        );
        assert_eq!(declared(Columns::Matrix), ["\"a\" Int64", "\"b\" Int64", "\"c\" String"]);
    }
}
