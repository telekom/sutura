//! A [`Warehouse`] adapter over `DuckDB`, for local development and single-file work.
//!
//! `DuckDB` is the case where the data is a file and there is no server to authenticate against, so
//! the single-player credential is the process's own access. That is stated rather than hidden: this
//! adapter is not a stand-in for a data system with grants, and it is the one place in the design
//! where "run as the calling subject" is trivially satisfied because there is nobody else to be.
//!
//! Two things this adapter deliberately does not offer:
//!
//! **No arbitrary SQL entry point.** [`DuckDbWarehouse::execute`] takes an [`Executable`] and renders
//! the statement itself, into a [`GeneratedQuery`] that carries its parameters separately. There is
//! no method that takes a string. A development affordance that ran a statement somebody typed would
//! be the shortest path around every check upstream of here.
//!
//! **No result caching.** Under row-level security a query-keyed cache is a cross-user leak, and
//! although this adapter has no row-level security to leak through, adding a cache here would be the
//! place the habit started.

use std::path::Path;

use duckdb::Connection;
use duckdb::types::Value as DuckValue;
use sutura_domain::model::TableName;
use sutura_domain::plan::Executable;
use sutura_domain::warehouse::{MalformedRowSet, ParamValue, Real, RowSet, Value, Warehouse};
use sutura_sql::generate::generate;
use sutura_sql::{Dialect, GenerateError, GeneratedQuery};

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
    ///
    /// The column is named by its LABEL rather than by its position, which is also what the engine
    /// does. The two adapters answer one plan, so an error from either has to be readable against the
    /// same projection, and "column 1" is a fact about a result set nobody has in front of them.
    #[error("column {column} came back as {duckdb_type}, which this adapter does not map")]
    UnsupportedType { column: String, duckdb_type: &'static str },
    /// A floating-point column came back as a value that is not a number.
    ///
    /// **What `zero_denominator: fails` actually produces.** The generator emits that ratio's
    /// division unguarded and casts the numerator to `DOUBLE` first, so the division is IEEE float
    /// division: `CAST(3 AS DOUBLE) / 0` is `inf` here rather than an error, and `0 / 0` is `NaN`.
    /// [`Real`] refuses all three, so the word `fails` is true of the metric that chose it instead of
    /// answering the string `inf` under a certified name.
    ///
    /// The cause names which of the three it was; this variant names the column.
    #[error("column {column} came back as a value that is not a finite number")]
    NotFinite {
        column: String,
        #[source]
        cause: sutura_domain::warehouse::NotFinite,
    },
    /// A day number came back that is not a date this build can represent.
    ///
    /// The cause is kept rather than discarded: "not a date" and "a date in the year 40 000" send a
    /// reader to different places.
    #[error("column {column} came back as a day number that is not a date")]
    NotADate {
        column: String,
        #[source]
        cause: sutura_domain::calendar::InvalidDate,
    },
    #[error("the result set was not rectangular")]
    Shape {
        #[source]
        cause: MalformedRowSet,
    },
    /// The driver handed back a result set with no statement behind it, so there are no column
    /// labels to read.
    ///
    /// **An error rather than an empty projection, and the empty projection was the bug.** This was
    /// `unwrap_or_default()`, which turns a missing schema into a zero-column result - and a
    /// `RowSet` with no columns and N rows is a shape [`RowSet::new`] ACCEPTS, because every row
    /// then has no cells either and the thing is rectangular. So a question would have been answered
    /// with a result set that had silently lost its projection, under a certified name and with
    /// provenance attached. Refusal beats degradation on a shape check: nothing downstream can tell
    /// "this metric has no columns" from "this driver told us nothing".
    ///
    /// No `#[source]`, because there is nothing to preserve: the handle is an `Option` and the
    /// absent case carries no cause. That is the whole of what the driver said.
    #[error("the result set came back without the statement that produced it, so it has no columns")]
    NoSchema,
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
    /// One leg of a federated answer, which nothing here can assemble above.
    ///
    /// **Not a refusal and not a default body.** [`Warehouse::execute`] takes an
    /// [`Executable`], so this adapter's match over what it can be handed is exhaustive - that is
    /// the mechanism, and this variant is what it costs today. Nothing constructs a
    /// [`LegPlan`](sutura_domain::plan::LegPlan) outside a test: there is no splitter and no
    /// combiner, so no code path reaches here.
    ///
    /// **This adapter could RENDER one** - `sutura_sql::generate_leg` renders every leg shape for
    /// this dialect and the golden family pins it - and rendering is not answering. Executing a leg
    /// with nothing above it returns rows at a finer grouping than the question asked for, and a
    /// half-federated answer under a certified metric name is the failure this repository exists to
    /// prevent. So the leg path arrives with the combiner and its differential test, not before.
    ///
    /// It carries the table rather than a sentence, because that is the one thing a reader chasing
    /// this needs and the message may be reworded.
    #[error("this adapter answers a whole plan, and the leg against {table} needs a combiner above it")]
    LegWithoutCombiner { table: String },
}

/// A `DuckDB` database, behind the [`Warehouse`] port.
pub struct DuckDbWarehouse {
    source: sutura_domain::model::SourceName,
    /// Which identity a query reaches this source as, handed over at construction.
    ///
    /// The deployment declares the posture and the adapter declares its *capability* - see
    /// `Warehouse::IMPERSONATION` below. Kept so provenance is read off the thing that executed.
    posture: sutura_domain::source::SourcePosture,
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
    pub fn open(
        source: sutura_domain::model::SourceName,
        posture: sutura_domain::source::SourcePosture,
        path: &Path,
    ) -> Result<Self, DuckDbError> {
        let connection = Connection::open(path).map_err(|cause| DuckDbError::Open {
            path: path.display().to_string(),
            cause,
        })?;
        Ok(Self {
            source,
            posture,
            connection,
        })
    }

    /// Opens a database that exists only for this process.
    ///
    /// What the golden suite uses: a fixture that is built from a committed CSV every run cannot
    /// drift from the CSV, and a database file in the repository would be a binary nobody reviews.
    /// **The posture is a parameter and has no default**, for the reason the port gives: a defaulted
    /// posture would be a claim about who a query runs as that nobody made.
    pub fn in_memory(
        source: sutura_domain::model::SourceName,
        posture: sutura_domain::source::SourcePosture,
    ) -> Result<Self, DuckDbError> {
        let connection = Connection::open_in_memory().map_err(|cause| DuckDbError::Open {
            path: String::from(":memory:"),
            cause,
        })?;
        Ok(Self {
            source,
            posture,
            connection,
        })
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
    /// This adapter asks `sutura-sql` to render rather than rendering itself, which is why it depends
    /// on that crate: a second implementation here would be a second set of quoting and placeholder
    /// decisions to keep in step with every other SQL adapter. The dialect is not a parameter - a
    /// `DuckDB` adapter renders `DuckDB`.
    ///
    /// It does **not** depend on `sutura-semantic`, and that is the correction: turning a plan into a
    /// statement is not the compiler's last stage. The port carries a `QueryPlan`, this adapter
    /// compiles nothing, and while rendering lived in the core every consumer of the core linked a
    /// SQL generator - including one that renders nothing at all.
    ///
    /// Free-standing rather than a method: it reads nothing from `self`, and taking `&self` would
    /// imply the rendering depends on which connection is open, which it must not.
    /// One exhaustive match, so a third plan shape cannot be answered by accident. The leg arm
    /// refuses rather than renders, and [`DuckDbError::LegWithoutCombiner`] says why.
    fn render(executable: Executable<'_>) -> Result<GeneratedQuery, DuckDbError> {
        match executable {
            Executable::Query(plan) => generate(plan, Dialect::DuckDb).map_err(|cause| DuckDbError::Render { cause }),
            Executable::Leg(leg) => Err(DuckDbError::LegWithoutCombiner {
                table: String::from(leg.table().as_str()),
            }),
        }
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
                    ParamValue::Date(d) => Box::new(d.to_iso()),
                }
            })
            .collect()
    }

    /// One cell, as a domain value.
    ///
    /// **One mapping, two adapters.** The other implementor of the [`Warehouse`] port is the
    /// in-process engine, and a plan may be answered by either: a rule that held here and not there
    /// would mean an anchor certified against one adapter not reproducing against the other, which
    /// is the thing the differential suite exists to notice. So the set of types that answer, and
    /// the set that are refused, is decided once and written down in both places - `cell` in
    /// `crates/sutura-exec-datafusion/src/lib.rs` is the other half, and
    /// `every_type_this_adapter_maps_answers_what_the_engine_answers` in the tests below is the
    /// table both are held to.
    ///
    /// Every integer width answers, because all of them fit an `i64` losslessly. A 64-bit unsigned
    /// value that does not is rendered as text rather than wrapped. A 32-bit float is refused, and so
    /// is a 64-bit one that is not finite: `inf`, `-inf` and `NaN` are what an unguarded division
    /// answers rather than failing, and [`Real`] is where that stops being an answer.
    fn cell(label: &str, value: DuckValue) -> Result<Value, DuckDbError> {
        let unsupported = |duckdb_type: &'static str| DuckDbError::UnsupportedType {
            column: String::from(label),
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
            // REFUSED, not widened, and the two adapters have to say the same thing here. This arm
            // was `Value::Real(f64::from(v))`, which is the tempting one and is exactly wrong:
            // `0.1_f32` as an `f64` renders as `0.10000000149011612`. The in-process engine refuses
            // `Float32` for that reason - the comment is on `cell` in
            // `crates/sutura-exec-datafusion/src/lib.rs` - so widening here meant one plan answering
            // two different numbers depending on which adapter ran it, and an anchor certified
            // against one not reproducing against the other.
            DuckValue::Float(_) => Err(unsupported(
                "REAL; a 32-bit float has no exact 64-bit rendering, so it is refused rather than widened",
            )),
            // Checked, not taken. A `DOUBLE` column is where an unguarded division lands, and a
            // division by zero in IEEE arithmetic answers `inf` rather than failing - so this is the
            // arm that decides whether `zero_denominator: fails` means what its word says. The
            // engine's `cell` has the same arm for the same reason.
            DuckValue::Double(v) => Real::parse(v).map(Value::Real).map_err(|cause| DuckDbError::NotFinite {
                column: String::from(label),
                cause,
            }),
            // Text, so an exact decimal stays exact. Turning it into an `f64` here is how a total
            // that was correct in the data system stops being correct in an answer.
            DuckValue::Decimal(v) => Ok(Value::Text(v.to_string())),
            DuckValue::Text(v) => Ok(Value::Text(v)),
            // The generated projection truncates the time column, so a date is exactly what comes
            // back for the `period` column. Converted here rather than cast to text inside the
            // statement, which would bake one dialect's date format into every dialect's SQL.
            DuckValue::Date32(days) => sutura_domain::calendar::Date::from_days_since_epoch(days)
                .map(|date| Value::Text(date.to_iso()))
                .map_err(|cause| DuckDbError::NotADate {
                    column: String::from(label),
                    cause,
                }),
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
        //
        // Refused when the handle is absent rather than defaulted to no columns - see
        // [`DuckDbError::NoSchema`] for why an empty projection is a shape `RowSet::new` accepts and
        // therefore the one degradation nothing downstream could notice.
        let Some(statement) = rows.as_ref() else {
            return Err(DuckDbError::NoSchema);
        };
        let columns: Vec<String> = statement.column_names();
        let width = columns.len();
        let mut out: Vec<Vec<Value>> = Vec::new();
        while let Some(row) = rows.next().map_err(|cause| DuckDbError::Execute { cause })? {
            let mut cells = Vec::with_capacity(width);
            // Enumerated over the labels rather than over `0..width`, so a cell that cannot be
            // carried has its column's NAME to report and not its position. Under the
            // `indexing_slicing` ban the alternative is a lookup with a fallback, and the fallback
            // would be the string a reader actually gets on the day it matters.
            for (index, label) in columns.iter().enumerate() {
                let raw: DuckValue = row.get(index).map_err(|cause| DuckDbError::Execute { cause })?;
                cells.push(Self::cell(label, raw)?);
            }
            out.push(cells);
        }
        RowSet::new(columns, out).map_err(|cause| DuckDbError::Shape { cause })
    }
}

impl Warehouse for DuckDbWarehouse {
    type Error = DuckDbError;

    /// **One process holding one connection under one operating-system identity**, so there is nowhere
    /// for a subject's own credential to arrive. The same answer the in-process engine gives, for the
    /// same reason, and it is declared rather than assumed: a source configured
    /// `impersonation-at-source` on this adapter does not start.
    const IMPERSONATION: sutura_domain::source::ImpersonationCapability =
        sutura_domain::source::ImpersonationCapability::NoPlaceForASubject;

    fn source(&self) -> &sutura_domain::model::SourceName {
        &self.source
    }

    fn posture(&self) -> &sutura_domain::source::SourcePosture {
        &self.posture
    }

    /// Prepares the statement without running it.
    ///
    /// A real check rather than a stub: preparing resolves every table and column name and validates
    /// the syntax, so a statement that would fail at the data system fails here, before anything is
    /// read.
    fn dry_run(&self, executable: Executable<'_>) -> Result<(), Self::Error> {
        let query = Self::render(executable)?;
        drop(
            self.connection
                .prepare(query.sql())
                .map_err(|cause| DuckDbError::Prepare { cause })?,
        );
        Ok(())
    }

    fn execute(&self, executable: Executable<'_>) -> Result<RowSet, Self::Error> {
        let query = Self::render(executable)?;
        self.run(&query)
    }
}

/// The value mapping, as a table.
///
/// **The table is duplicated on purpose, and the duplication is the test.** The two `cell` functions
/// live in different crates - this one takes a `duckdb::types::Value` and the engine's takes an
/// Arrow array - and neither crate may depend on the other: they are two implementors of one port,
/// and a shared test helper would have to live in the domain, which is not allowed to know that
/// either of them exists. So the agreement is asserted as the same expected column written out in
/// both places: here, and in `crates/sutura-exec-datafusion/src/value_mapping_tests.rs`. The two
/// test names quote each other, so a change to one that is not made to the other shows up as a
/// failing assertion rather than as a disagreement nobody notices until an anchor stops
/// reproducing.
///
/// The alternative - one test that calls both adapters - is a test in `sutura-app`, which is where
/// `tests/differential.rs` already runs one plan through both and compares the rows. That test can
/// only see the types a fixture CSV produces, which is why the table below is not redundant with it:
/// it reaches the widths a Parquet file has and a CSV never will.
#[cfg(test)]
mod tests {
    use super::{DuckDbError, DuckDbWarehouse, Real};
    use duckdb::types::{Decimal, TimeUnit, Value as DuckValue};
    use sutura_domain::calendar::Date;
    use sutura_domain::model::SourceName;
    use sutura_domain::warehouse::{RowSet, Value};
    use sutura_sql::GeneratedQuery;

    fn real(value: f64) -> Real {
        Real::parse(value).expect("a test literal is finite")
    }

    /// One row of the shared table: what the value is called, what the driver hands over, and the
    /// domain value both adapters have to produce for it. Named because the tuple is over the
    /// `type_complexity` threshold this workspace tightened, and a `Vec<(..)>` of three is where it
    /// starts to be unreadable anyway.
    type Case = (&'static str, DuckValue, Value);

    fn day(iso: &str) -> Date {
        Date::parse(iso).expect("a test date is a date")
    }

    fn source() -> SourceName {
        SourceName::parse("local").expect("a test source is a source")
    }

    /// The posture a test opens this adapter with.
    ///
    /// Shared, and it is the honest declaration rather than a convenience: one process holds one
    /// connection under one operating-system identity, which is what the capability constant above
    /// says out loud.
    fn shared_posture() -> sutura_domain::source::SourcePosture {
        sutura_domain::source::SourcePosture::SharedServiceUser {
            declared: sutura_domain::source::SharedIdentityDeclared::of(
                sutura_domain::source::AcknowledgementReason::parse("one process, one connection, one operating-system identity")
                    .expect("a fixture reason is a reason"),
            ),
        }
    }

    #[test]
    fn every_type_this_adapter_maps_answers_what_the_engine_answers() {
        // The twin of `every_type_the_engine_maps_answers_what_the_data_source_answers` in
        // `crates/sutura-exec-datafusion/src/value_mapping_tests.rs`. Same logical values, same
        // expected column, one row per width - because a Parquet `INT32` column under a `min` or a
        // `max` used to answer here and error there.
        // Boundary values rather than round ones, written in hex where the decimal form is a bit
        // pattern nobody reads: an arm that reached for the wrong width would come back truncated
        // or sign-flipped, and 42 would survive that.
        let cases: Vec<Case> = vec![
            ("NULL", DuckValue::Null, Value::Null),
            ("BOOLEAN true", DuckValue::Boolean(true), Value::Integer(1)),
            ("BOOLEAN false", DuckValue::Boolean(false), Value::Integer(0)),
            ("TINYINT", DuckValue::TinyInt(i8::MIN), Value::Integer(-128)),
            ("SMALLINT", DuckValue::SmallInt(i16::MIN), Value::Integer(-0x8000)),
            ("INTEGER", DuckValue::Int(i32::MAX), Value::Integer(0x7FFF_FFFF)),
            ("BIGINT", DuckValue::BigInt(i64::MIN), Value::Integer(i64::MIN)),
            ("UTINYINT", DuckValue::UTinyInt(u8::MAX), Value::Integer(255)),
            ("USMALLINT", DuckValue::USmallInt(u16::MAX), Value::Integer(0xFFFF)),
            ("UINTEGER", DuckValue::UInt(u32::MAX), Value::Integer(0xFFFF_FFFF)),
            ("UBIGINT that fits an i64", DuckValue::UBigInt(42), Value::Integer(42)),
            (
                "UBIGINT that does not",
                DuckValue::UBigInt(u64::MAX),
                Value::Text(String::from("18446744073709551615")),
            ),
            ("DOUBLE", DuckValue::Double(0.1), Value::Real(real(0.1))),
            // Zero is finite, and it is here because the check that refuses `inf` is a check about a
            // division BY zero: a metric that legitimately answers zero must still answer.
            ("DOUBLE zero", DuckValue::Double(0.0), Value::Real(real(0.0))),
            (
                "DECIMAL stays text so it stays exact",
                DuckValue::Decimal(Decimal::new(9, 2, 12_345).expect("a test decimal is a decimal")),
                Value::Text(String::from("123.45")),
            ),
            (
                "VARCHAR",
                DuckValue::Text(String::from("north")),
                Value::Text(String::from("north")),
            ),
            (
                "DATE as ISO text",
                DuckValue::Date32(day("2026-06-01").days_since_epoch()),
                Value::Text(String::from("2026-06-01")),
            ),
        ];
        for (name, raw, expected) in cases {
            assert_eq!(DuckDbWarehouse::cell(name, raw).expect(name), expected, "{name}");
        }
    }

    #[test]
    fn a_32_bit_float_is_refused_here_because_it_is_refused_there() {
        // The finding this arm exists for. It was `Value::Real(f64::from(v))`, and the engine's
        // `cell` refused `Float32` in the same release - so one plan over a `REAL` column answered
        // 0.10000000149011612 through the data source and errored through the engine. Refusing is
        // the half of the disagreement that can be fixed without inventing a rendering: there is no
        // `f64` that is `0.1_f32`, and picking one silently is how a number nobody got wrong stops
        // matching itself.
        let error = DuckDbWarehouse::cell("amount", DuckValue::Float(0.1)).expect_err("a 32-bit float is not mapped");
        assert!(
            matches!(error, DuckDbError::UnsupportedType { ref column, .. } if column == "amount"),
            "{error:?}"
        );
        let message = error.to_string();
        assert!(message.contains("REAL"), "{message}");
        assert!(message.contains("column amount"), "{message}");
        // And the type that DOES answer, so this is not a test that would pass with every float
        // refused.
        assert_eq!(
            DuckDbWarehouse::cell("amount", DuckValue::Double(0.1)).expect("a 64-bit float is mapped"),
            Value::Real(real(0.1))
        );
    }

    #[test]
    fn a_non_finite_double_is_refused_here_because_it_is_refused_there() {
        // THE FINDING THIS ARM EXISTS FOR, and the twin of
        // `a_non_finite_double_is_refused_on_both_sides_of_the_port` in
        // `crates/sutura-exec-datafusion/src/value_mapping_tests.rs`. This arm was `Value::Real(v)`
        // on a raw `f64`, and the value that reached it was real: a ratio measure declaring
        // `zero_denominator: fails` renders as an unguarded division with the numerator cast to
        // `DOUBLE`, and `CAST(3 AS DOUBLE) / 0` in this data system is `inf`, not an error. So the
        // metric answered the string "inf" under its own certified name, and the engine answered the
        // same string, so the differential test agreed and passed.
        //
        // All three of the class, not just the one a zero denominator produces first: a guard on the
        // division would have left `-inf` and `NaN` on the way in.
        for (name, raw) in [
            ("positive infinity", f64::INFINITY),
            ("negative infinity", f64::NEG_INFINITY),
            ("not a number", f64::NAN),
        ] {
            let error = DuckDbWarehouse::cell("revenue_per_refunded_order", DuckValue::Double(raw)).expect_err(name);
            assert!(
                matches!(error, DuckDbError::NotFinite { ref column, .. } if column == "revenue_per_refunded_order"),
                "{name}: {error:?}"
            );
            assert_eq!(
                error.to_string(),
                "column revenue_per_refunded_order came back as a value that is not a finite number",
                "{name}"
            );
        }
        // And the values that DO answer, so this is not a test that would pass with every double
        // refused - zero included, because the check is about dividing BY zero and not about it.
        assert_eq!(
            DuckDbWarehouse::cell("revenue", DuckValue::Double(0.0)).expect("zero is a finite number"),
            Value::Real(real(0.0))
        );
        assert_eq!(
            DuckDbWarehouse::cell("average_order", DuckValue::Double(63_335.777_777_777_78))
                .expect("an average is a finite number"),
            Value::Real(real(63_335.777_777_777_78))
        );
    }

    #[test]
    fn a_type_neither_adapter_maps_names_itself_rather_than_being_rendered() {
        // A `Debug` fallback here would flow into an answer looking like data, and an anchor
        // comparison against it would pass or fail for a reason nobody could read.
        for raw in [
            DuckValue::Blob(vec![0_u8, 1_u8]),
            DuckValue::Timestamp(TimeUnit::Microsecond, 0),
            DuckValue::List(vec![DuckValue::Int(1)]),
        ] {
            let error = DuckDbWarehouse::cell("payload", raw).expect_err("an unmapped type is an error");
            assert!(
                matches!(error, DuckDbError::UnsupportedType { ref column, .. } if column == "payload"),
                "{error:?}"
            );
        }
    }

    #[test]
    fn the_column_labels_come_from_the_statement_that_answered() {
        // The half of the schema read that is reachable, and the reason the labels are taken from
        // the executed statement at all: a projection is what an answer is read by. `run` is
        // exercised directly with a literal statement, because the labels have to be right before
        // any plan is involved.
        let warehouse = DuckDbWarehouse::in_memory(source(), shared_posture()).expect("an in-memory database opens");
        let query = GeneratedQuery::new(source(), String::from("SELECT 1 AS period, 'north' AS region"), Vec::new());
        let rows = warehouse.run(&query).expect("a literal select answers");
        assert_eq!(rows.columns(), ["period", "region"]);
        assert_eq!(rows.rows().len(), 1);
    }

    #[test]
    fn a_result_set_with_no_statement_behind_it_is_refused_rather_than_answered_with_no_columns() {
        // THE DEGRADATION THIS VARIANT REPLACED. The column labels were read as
        // `rows.as_ref().map(Statement::column_names).unwrap_or_default()`, so an absent handle
        // produced an empty projection - and this is why nothing downstream would have caught it: a
        // `RowSet` with no columns and N rows is REJECTED BY NOTHING, because every row has no cells
        // either and the result is rectangular.
        let degraded = RowSet::new(Vec::new(), vec![Vec::new(), Vec::new()])
            .expect("no columns and no cells per row is rectangular, which is what made the default silent");
        assert!(degraded.columns().is_empty());
        assert_eq!(degraded.rows().len(), 2, "two rows of nothing, and a valid result set");
        // So the shape has to be refused where it arises. Constructed rather than provoked: the
        // handle is present for every statement this adapter runs - the test above is that path -
        // and a driver that stopped handing one over is exactly the change that must not turn into
        // an answer. The message names what is missing, because "no columns" on its own reads as a
        // fact about the metric rather than about the driver.
        let error = DuckDbError::NoSchema;
        assert_eq!(
            error.to_string(),
            "the result set came back without the statement that produced it, so it has no columns"
        );
        assert!(
            core::error::Error::source(&error).is_none(),
            "an absent handle carries no cause, and inventing one would be worse than saying so"
        );
    }
}
