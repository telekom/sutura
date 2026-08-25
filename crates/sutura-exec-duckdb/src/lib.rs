//! A [`Warehouse`] adapter over `DuckDB`, for local development and single-file work.
//!
//! `DuckDB` is the case where the data is a file and there is no server to authenticate against, so
//! the single-player credential is the process's own access. That is stated rather than hidden: this
//! adapter is not a stand-in for a data system with grants, and it is the one place in the design
//! where "run as the calling subject" is trivially satisfied because there is nobody else to be.
//!
//! Two things this adapter deliberately does not offer:
//!
//! **No arbitrary SQL entry point.** [`DuckDbWarehouse::execute`] takes a
//! [`GeneratedQuery`], which carries its parameters separately, and there is no method that takes a
//! string. A development affordance that ran a statement somebody typed would be the shortest path
//! around every check upstream of here.
//!
//! **No result caching.** Under row-level security a query-keyed cache is a cross-user leak, and
//! although this adapter has no row-level security to leak through, adding a cache here would be the
//! place the habit started.

use std::path::Path;

use duckdb::Connection;
use duckdb::types::Value as DuckValue;
use sutura_domain::model::TableName;
use sutura_domain::plan::QueryPlan;
use sutura_domain::warehouse::{GeneratedQuery, MalformedRowSet, ParamValue, RowSet, Value, Warehouse};
use sutura_semantic::generate::generate;
use sutura_semantic::{Dialect, GenerateError};

/// Why this data system could not answer.
#[derive(Debug, thiserror::Error)]
pub enum DuckDbError {
    #[error("could not open the database at {path}")]
    Open {
        path: String,
        #[source]
        cause: duckdb::Error,
    },
    #[error("the statement was not accepted")]
    Prepare {
        #[source]
        cause: duckdb::Error,
    },
    #[error("the statement failed while running")]
    Execute {
        #[source]
        cause: duckdb::Error,
    },
    /// A column came back as a type this adapter does not map.
    ///
    /// An error rather than a stringified fallback. A `LIST` or a `STRUCT` rendered with `Debug`
    /// would flow into an answer looking like data, and an anchor comparison against it would pass
    /// or fail for reasons nobody could read.
    #[error("column {column} came back as {duckdb_type}, which this adapter does not map")]
    UnsupportedType { column: usize, duckdb_type: &'static str },
    /// A day number came back that is not a date this build can represent.
    ///
    /// The cause is kept rather than discarded: "not a date" and "a date in the year 40 000" send a
    /// reader to different places.
    #[error("column {column} came back as a day number that is not a date")]
    NotADate {
        column: usize,
        #[source]
        cause: sutura_domain::calendar::InvalidDate,
    },
    #[error("the result set was not rectangular")]
    Shape {
        #[source]
        cause: MalformedRowSet,
    },
    /// The plan could not be rendered as SQL.
    ///
    /// This adapter speaks SQL, so it asks the compiler to render the plan for its own dialect. An
    /// adapter that executes a plan directly - the in-process engine - never reaches this.
    #[error("the plan could not be rendered for DuckDB")]
    Render {
        #[source]
        cause: GenerateError,
    },
    #[error("could not attach {path} as table {table}")]
    Attach {
        table: String,
        path: String,
        #[source]
        cause: duckdb::Error,
    },
}

/// A `DuckDB` database, behind the [`Warehouse`] port.
pub struct DuckDbWarehouse {
    source: sutura_domain::model::SourceName,
    connection: Connection,
}

impl core::fmt::Debug for DuckDbWarehouse {
    /// Hand-written because `Connection` is not `Debug`, and because a connection's own `Debug`
    /// would be the sort of thing that prints a path or a handle into a log for no benefit.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DuckDbWarehouse")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl DuckDbWarehouse {
    /// Opens a database file.
    pub fn open(source: sutura_domain::model::SourceName, path: &Path) -> Result<Self, DuckDbError> {
        let connection = Connection::open(path).map_err(|cause| DuckDbError::Open {
            path: path.display().to_string(),
            cause,
        })?;
        Ok(Self { source, connection })
    }

    /// Opens a database that exists only for this process.
    ///
    /// What the golden suite uses: a fixture that is built from a committed CSV every run cannot
    /// drift from the CSV, and a database file in the repository would be a binary nobody reviews.
    pub fn in_memory(source: sutura_domain::model::SourceName) -> Result<Self, DuckDbError> {
        let connection = Connection::open_in_memory().map_err(|cause| DuckDbError::Open {
            path: String::from(":memory:"),
            cause,
        })?;
        Ok(Self { source, connection })
    }

    /// Exposes a CSV file as a table.
    ///
    /// A narrow, typed affordance instead of a general "run this SQL" method, which is what a local
    /// adapter usually grows and what would make every check upstream of here optional. The table
    /// name is a [`TableName`], so it cannot carry a quote; the path is a string, so it is escaped
    /// the only way a SQL string literal can be, by doubling every quote.
    ///
    /// `read_csv_auto` and not a bind parameter, because a table function's argument is part of the
    /// statement's shape rather than a value and `DuckDB` will not bind one there.
    pub fn attach_csv(&self, table: &TableName, path: &Path) -> Result<(), DuckDbError> {
        let literal = path.display().to_string().replace('\'', "''");
        let statement = format!(
            "CREATE OR REPLACE VIEW \"{}\" AS SELECT * FROM read_csv_auto('{literal}')",
            table.as_str()
        );
        self.connection
            .execute_batch(&statement)
            .map_err(|cause| DuckDbError::Attach {
                table: String::from(table.as_str()),
                path: path.display().to_string(),
                cause,
            })
    }

    /// The plan, rendered as a `DuckDB` statement.
    ///
    /// This adapter asks the compiler to render rather than rendering itself, which is why it depends
    /// on `sutura-semantic`: turning a plan into a dialect is the compiler's last stage, and a second
    /// implementation here would be a second set of quoting and placeholder decisions to keep in
    /// step. The dialect is not a parameter - a `DuckDB` adapter renders `DuckDB`.
    ///
    /// Free-standing rather than a method: it reads nothing from `self`, and taking `&self` would
    /// imply the rendering depends on which connection is open, which it must not.
    fn render(plan: &QueryPlan) -> Result<GeneratedQuery, DuckDbError> {
        generate(plan, Dialect::DuckDb).map_err(|cause| DuckDbError::Render { cause })
    }

    /// The parameters, as this driver wants them.
    ///
    /// A date is bound as its ISO text rather than as a date type. `DuckDB` compares a `DATE` column
    /// to a `VARCHAR` by casting the text, which is exactly the comparison we mean, and it keeps this
    /// adapter from needing a date library to agree with the one the domain does not have.
    fn bind(params: &[ParamValue]) -> Vec<Box<dyn duckdb::ToSql>> {
        params
            .iter()
            .map(|param| -> Box<dyn duckdb::ToSql> {
                match *param {
                    ParamValue::Text(ref v) => Box::new(v.clone()),
                    ParamValue::Integer(v) => Box::new(v),
                    ParamValue::Date(d) => Box::new(d.to_iso()),
                }
            })
            .collect()
    }

    /// One cell, as a domain value.
    fn cell(index: usize, value: DuckValue) -> Result<Value, DuckDbError> {
        let unsupported = |duckdb_type: &'static str| DuckDbError::UnsupportedType {
            column: index,
            duckdb_type,
        };
        match value {
            DuckValue::Null => Ok(Value::Null),
            DuckValue::Boolean(v) => Ok(Value::Integer(i64::from(v))),
            DuckValue::TinyInt(v) => Ok(Value::Integer(i64::from(v))),
            DuckValue::SmallInt(v) => Ok(Value::Integer(i64::from(v))),
            DuckValue::Int(v) => Ok(Value::Integer(i64::from(v))),
            DuckValue::BigInt(v) => Ok(Value::Integer(v)),
            DuckValue::UTinyInt(v) => Ok(Value::Integer(i64::from(v))),
            DuckValue::USmallInt(v) => Ok(Value::Integer(i64::from(v))),
            DuckValue::UInt(v) => Ok(Value::Integer(i64::from(v))),
            // A `SUM` over integers comes back as one of the wide types. It is rendered as text when
            // it does not fit, rather than wrapped: a silently truncated total is a wrong number.
            DuckValue::UBigInt(v) => Ok(i64::try_from(v).map_or_else(|_| Value::Text(v.to_string()), Value::Integer)),
            DuckValue::HugeInt(v) => Ok(i64::try_from(v).map_or_else(|_| Value::Text(v.to_string()), Value::Integer)),
            DuckValue::Float(v) => Ok(Value::Real(f64::from(v))),
            DuckValue::Double(v) => Ok(Value::Real(v)),
            // Text, so an exact decimal stays exact. Turning it into an `f64` here is how a total
            // that was correct in the data system stops being correct in an answer.
            DuckValue::Decimal(v) => Ok(Value::Text(v.to_string())),
            DuckValue::Text(v) => Ok(Value::Text(v)),
            // The generated projection truncates the time column, so a date is exactly what comes
            // back for the `period` column. Converted here rather than cast to text inside the
            // statement, which would bake one dialect's date format into every dialect's SQL.
            DuckValue::Date32(days) => sutura_domain::calendar::Date::from_days_since_epoch(days)
                .map(|date| Value::Text(date.to_iso()))
                .map_err(|cause| DuckDbError::NotADate { column: index, cause }),
            DuckValue::Timestamp(..) => Err(unsupported(
                "a timestamp; a metric on a timestamp column is not supported yet",
            )),
            DuckValue::Blob(_) => Err(unsupported("BLOB")),
            _ => Err(unsupported("a nested or interval type")),
        }
    }

    /// Runs a statement and collects its rows.
    ///
    /// **The column names are read after the query, not before, and that order is not cosmetic.**
    /// `Statement::column_names` panics on a statement that has not been executed - its own
    /// documentation says so - because the schema arrives with the result rather than with the
    /// prepare. Asking first is a panic reachable from any question, which is what
    /// `panic = "abort"` on the shipped profiles turns into a dead process.
    fn run(&self, query: &GeneratedQuery) -> Result<RowSet, DuckDbError> {
        let mut statement = self
            .connection
            .prepare(query.sql())
            .map_err(|cause| DuckDbError::Prepare { cause })?;
        let bound = Self::bind(query.params());
        let refs: Vec<&dyn duckdb::ToSql> = bound.iter().map(AsRef::as_ref).collect();
        let mut rows = statement
            .query(refs.as_slice())
            .map_err(|cause| DuckDbError::Execute { cause })?;
        // Owned, so the immutable borrow of `rows` ends before the loop needs it mutably.
        let columns: Vec<String> = rows.as_ref().map(duckdb::Statement::column_names).unwrap_or_default();
        let width = columns.len();
        let mut out: Vec<Vec<Value>> = Vec::new();
        while let Some(row) = rows.next().map_err(|cause| DuckDbError::Execute { cause })? {
            let mut cells = Vec::with_capacity(width);
            for index in 0..width {
                let raw: DuckValue = row.get(index).map_err(|cause| DuckDbError::Execute { cause })?;
                cells.push(Self::cell(index, raw)?);
            }
            out.push(cells);
        }
        RowSet::new(columns, out).map_err(|cause| DuckDbError::Shape { cause })
    }
}

impl Warehouse for DuckDbWarehouse {
    type Error = DuckDbError;

    fn source(&self) -> &sutura_domain::model::SourceName {
        &self.source
    }

    /// Prepares the statement without running it.
    ///
    /// A real check rather than a stub: preparing resolves every table and column name and validates
    /// the syntax, so a statement that would fail at the data system fails here, before anything is
    /// read.
    fn dry_run(&self, plan: &QueryPlan) -> Result<(), Self::Error> {
        let query = Self::render(plan)?;
        drop(
            self.connection
                .prepare(query.sql())
                .map_err(|cause| DuckDbError::Prepare { cause })?,
        );
        Ok(())
    }

    fn execute(&self, plan: &QueryPlan) -> Result<RowSet, Self::Error> {
        let query = Self::render(plan)?;
        self.run(&query)
    }
}
