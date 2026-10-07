#![forbid(unsafe_code)]
//! A [`Warehouse`] adapter over `DuckDB`, through its ADBC driver, for local development and
//! single-file work.
//!
//! `DuckDB` is the case where the data is a file and there is no server to authenticate against, so
//! the single-player credential is the process's own access. That is stated rather than hidden: this
//! adapter is not a stand-in for a data system with grants, and it is the one place in the design
//! where "run as the calling subject" is trivially satisfied because there is nobody else to be.
//!
//! **ADBC is the only transport** (`telekom/sutura#913`). `DuckDB` is its own ADBC driver - the
//! engine library defines `duckdb_adbc_init` - so the driver is the archive this artefact links
//! (`nix/duckdb-adbc.nix`, both musl triples) or the `libduckdb` [`MOUNTED_DRIVER`] names. A result
//! arrives as Arrow batches and is handed on as the driver typed it: which types answer is decided
//! once, by the domain's reader (`ResultBatches::to_rows`), for every Arrow adapter alike.
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
//!
//! ## Limits
//!
//! - **The deadline is carried, not enforced** (`docs/adr/0029`): the driver's `ConnectionCancel`
//!   interrupts a running statement, and honouring the deadline needs a watchdog per call
//!   (`telekom/sutura#1236`).
//! - **The driver runs every statement of a string but the last at `set_sql_query`**, and prepares
//!   the last (the pinned `StatementSetSqlQuery`). Only rendered statements and this crate's own
//!   `attach_*` views reach it, each one statement.
//! - **A certified answer is handed on unread**, so a `REAL`, a non-finite `DOUBLE` or an unmapped
//!   type is refused by the domain's reader downstream rather than as a [`DuckDbError`].

use std::path::Path;

use adbc_core::error::{Error as CoreError, Status};
use adbc_core::options::{OptionDatabase, OptionValue};
use adbc_core::{Connection as _, Database as _, Driver as _, Statement as _};
use adbc_driver_manager::{ManagedDatabase, ManagedDriver, ManagedStatement};
use arrow_array::RecordBatchReader as _;
use sutura_adbc::parameter_batch;
use sutura_domain::identity::{Presented, PresentedDisagreesWithPosture};
use sutura_domain::model::TableName;
use sutura_domain::plan::Executable;
use sutura_domain::warehouse::cardinality::{CountsNotRead, DeclaredKey, KeyUniqueness};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{
    Accumulating, AnchorRows, PreFlight, ResultBatches, RowSet, UnannouncedBatch, UnreadableCell, Warehouse,
};
use sutura_sql::generate::{generate, generate_key_probe, generate_leg};
use sutura_sql::{Dialect, GenerateError, GeneratedQuery};

/// The variable a host that links no driver names a mounted `libduckdb` with.
///
/// **Not a settings key**, for `sutura-adbc-postgres`'s `MOUNTED_DRIVER` reason: which driver file a
/// host carries is a property of the host, and a build that links one never reads this.
pub const MOUNTED_DRIVER: &str = "SUTURA_DUCKDB_ADBC_DRIVER";

/// Why this data system could not answer.
#[derive(Debug, thiserror::Error)]
pub enum DuckDbError {
    #[error(
        "this build links no DuckDB ADBC driver and `{mounted}` is not set; point it at an absolute \
         libduckdb shared library",
        mounted = MOUNTED_DRIVER
    )]
    NoDriver,
    /// The driver did not load or initialise, by either route - a relative mounted path included.
    #[error("could not load the DuckDB ADBC driver")]
    Load {
        #[source]
        cause: CoreError,
    },
    #[error("could not open the database at {path}")]
    Open {
        path: String,
        #[source]
        cause: CoreError,
    },
    #[error("could not open a connection to the database")]
    Connect {
        #[source]
        cause: CoreError,
    },
    #[error("the statement was not accepted")]
    Prepare {
        #[source]
        cause: CoreError,
    },
    #[error("the statement failed while running")]
    Execute {
        #[source]
        cause: CoreError,
    },
    #[error("a result batch could not be read")]
    Batch {
        #[source]
        cause: arrow_schema::ArrowError,
    },
    #[error("the plan's values could not be assembled for binding")]
    Parameters {
        #[source]
        cause: arrow_schema::ArrowError,
    },
    #[error("the result stream did not match its announced schema")]
    Unannounced {
        #[source]
        cause: UnannouncedBatch,
    },
    /// The collected result would cost more than this adapter's materialisation budget to hold.
    ///
    /// The byte budget the port's
    /// [`result_did_not_fit`](sutura_domain::warehouse::Warehouse::result_did_not_fit) reads: a
    /// result refused for crossing it is *the result did not fit*, never a data-system failure,
    /// so a caller is refused rather than told to retry.
    #[error("a result would cost more than the {most_bytes}-byte materialisation budget to hold")]
    OverBudget { most_bytes: usize },
    /// A boot-path result the domain's reader refused - the type, or the value, names the column.
    #[error("a result cell could not be read")]
    Unreadable {
        #[source]
        cause: UnreadableCell,
    },
    /// A key probe's result was not the pair of counts its statement projects.
    ///
    /// A defect in the rendering or in the value mapping, never anything about the data: the probe
    /// projects two aggregates over no group, so one row of two integers is the only shape it can
    /// have. It travels as an `Err` from the port, which the boot path reads as *this declaration
    /// went unchecked* rather than as a violated one.
    #[error("the key probe did not come back as two counts")]
    KeyCounts {
        #[source]
        cause: CountsNotRead,
    },
    /// The plan could not be rendered as SQL.
    #[error("the plan could not be rendered for DuckDB")]
    Render {
        #[source]
        cause: GenerateError,
    },
    /// The fixture CSV could not be read to name its column types.
    #[cfg(feature = "fixtures")]
    #[error("could not read the fixture {path} to type its columns")]
    FixtureRead {
        path: String,
        #[source]
        cause: std::io::Error,
    },
    /// The fixture CSV did not satisfy the shared schema boundary.
    #[cfg(feature = "fixtures")]
    #[error("could not infer the fixture {path} schema")]
    FixtureSchema {
        path: String,
        #[source]
        cause: sutura_domain::warehouse::csv::InferenceError,
    },
    #[error("could not attach {path} as table {table}")]
    Attach {
        table: String,
        path: String,
        #[source]
        cause: CoreError,
    },
    /// The credential broker handed this adapter subject material it has nowhere to put.
    ///
    /// **An `Err` and never a refusal.** Nothing about the question was wrong: it is a wiring defect
    /// between the broker and the source declaration, and a refusal would invite a client to retry a
    /// deployment bug. `docs/adr/0008` part 4 is the decision, and the same variant exists on the
    /// engine adapter for the same reason - two implementors of one port, each answering for what it
    /// was handed, because neither may reach into the other for a shared check.
    ///
    /// One process holding one database under one operating-system identity, which is what
    /// [`Warehouse::IMPERSONATION`] declares here, so the only shape this can be handed is the
    /// deployment's own identity for that source.
    #[error(
        "source `{at}` was handed {presented}, and this adapter has nowhere for a subject's own \
         credential to arrive: it is one process holding one connection under one identity. This is \
         a wiring defect between the credential broker and the source declaration"
    )]
    NoPlaceForASubject { at: String, presented: &'static str },
    /// The broker presented a leg that does not agree with how this source was DECLARED.
    ///
    /// **A different question from the variant above.** `NoPlaceForASubject` compares what arrived
    /// against what this CODE can carry - the [`Warehouse::IMPERSONATION`] constant - and reads
    /// `posture` not at all. So a shared leg carrying a *different* operator acknowledgement matched
    /// the variant this adapter accepts, while provenance, which is read off `posture`, reported this
    /// adapter's own declaration instead. An `Err` rather than a refusal, for the reason above.
    #[error("the credential broker presented a leg that disagrees with how this source is declared")]
    PresentedDisagreesWithPosture {
        #[source]
        cause: PresentedDisagreesWithPosture,
    },
}

/// A `DuckDB` database, behind the [`Warehouse`] port.
pub struct DuckDbWarehouse {
    source: sutura_domain::model::SourceName,
    /// Which identity a query reaches this source as, handed over at construction. Kept so
    /// provenance is read off the thing that executed.
    posture: sutura_domain::source::SourcePosture,
    /// The materialisation budget every collected result is bounded with: one for the adapter's
    /// life, with no unset state, so a call site that has no budget does not compile.
    result_budget: sutura_domain::warehouse::ResultBudget,
    /// The one database every call opens its own connection on. An in-memory one lives exactly as
    /// long as this handle, and the driver manager serialises each FFI object, so this adapter is
    /// `Sync` - what a federated answer's scoped lookup leg borrows across a thread - with no lock.
    database: ManagedDatabase,
}

impl core::fmt::Debug for DuckDbWarehouse {
    /// Hand-written: a database handle's `Debug` would print a handle into a log for no benefit.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DuckDbWarehouse")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

/// The driver this process opens: the one this artefact links, else the one [`MOUNTED_DRIVER`]
/// names. Never both: a build that links one never reads the variable.
fn driver() -> Result<ManagedDriver, DuckDbError> {
    let loaded = if sutura_adbc::LINKS_DUCKDB_DRIVER {
        sutura_adbc::linked_duckdb_driver()
    } else {
        let named = std::env::var(MOUNTED_DRIVER).map_err(|_absent| DuckDbError::NoDriver)?;
        sutura_adbc::mounted_duckdb_driver(&named)
    };
    loaded.map_err(|cause| DuckDbError::Load { cause })
}

impl DuckDbWarehouse {
    /// Opens a database file.
    ///
    /// # Errors
    ///
    /// [`DuckDbError::NoDriver`] or [`DuckDbError::Load`] where there is no driver, and
    /// [`DuckDbError::Open`] where the database does not open - a path that is not UTF-8 included,
    /// refused rather than converted lossily into a path that names another file.
    pub fn open(
        source: sutura_domain::model::SourceName,
        posture: sutura_domain::source::SourcePosture,
        path: &Path,
        result_budget: sutura_domain::warehouse::ResultBudget,
    ) -> Result<Self, DuckDbError> {
        let named = path.display().to_string();
        let Some(utf8) = path.to_str() else {
            return Err(DuckDbError::Open {
                path: named,
                cause: CoreError::with_message_and_status("the path is not UTF-8", Status::InvalidArguments),
            });
        };
        let database = driver()?
            .new_database_with_opts([(OptionDatabase::Other(String::from("path")), OptionValue::from(utf8))])
            .map_err(|cause| DuckDbError::Open { path: named, cause })?;
        Ok(Self {
            source,
            posture,
            result_budget,
            database,
        })
    }

    /// Opens a database that exists only for this process.
    ///
    /// What the golden suite uses: a fixture that is built from a committed CSV every run cannot
    /// drift from the CSV, and a database file in the repository would be a binary nobody reviews.
    /// **The posture is a parameter and has no default**, for the reason the port gives: a defaulted
    /// posture would be a claim about who a query runs as that nobody made.
    ///
    /// # Errors
    ///
    /// As [`Self::open`].
    pub fn in_memory(
        source: sutura_domain::model::SourceName,
        posture: sutura_domain::source::SourcePosture,
        result_budget: sutura_domain::warehouse::ResultBudget,
    ) -> Result<Self, DuckDbError> {
        let database = driver()?.new_database().map_err(|cause| DuckDbError::Open {
            path: String::from(":memory:"),
            cause,
        })?;
        Ok(Self {
            source,
            posture,
            result_budget,
            database,
        })
    }

    /// One connection, and a statement over `sql` the driver has prepared.
    fn statement(&self, sql: &str) -> Result<ManagedStatement, DuckDbError> {
        let mut connection = self
            .database
            .new_connection()
            .map_err(|cause| DuckDbError::Connect { cause })?;
        let mut statement = connection.new_statement().map_err(|cause| DuckDbError::Prepare { cause })?;
        statement.set_sql_query(sql).map_err(|cause| DuckDbError::Prepare { cause })?;
        Ok(statement)
    }

    /// Exposes a CSV file as a table.
    ///
    /// A narrow, typed affordance instead of a general "run this SQL" method, which is what a local
    /// adapter usually grows and would make every check upstream of here optional. The table name is
    /// a [`TableName`], so it cannot carry a quote; the path is a string, so it is escaped the only
    /// way a SQL string literal can be, by doubling every quote.
    ///
    /// # Errors
    ///
    /// [`DuckDbError::Attach`] where the driver refused the view, a CSV it cannot read included.
    pub fn attach_csv(&self, table: &TableName, path: &Path) -> Result<(), DuckDbError> {
        let literal = path.display().to_string().replace('\'', "''");
        self.attached(
            table,
            path,
            &format!(
                "CREATE OR REPLACE VIEW \"{}\" AS SELECT * FROM read_csv_auto('{literal}')",
                table.as_str()
            ),
        )
    }

    /// Exposes a conformance fixture CSV with shared column typing.
    ///
    /// The columns are typed before the read, and the typing comes from
    /// `sutura_domain::warehouse::csv` - the one classification every adapter shares. Naming the
    /// complete map keeps decimals exact and prevents `DuckDB`'s boolean and wide-integer inference
    /// from drifting from the other fixture adapters. Available only with the default-off `fixtures`
    /// feature.
    ///
    /// # Errors
    ///
    /// [`DuckDbError::FixtureRead`], [`DuckDbError::FixtureSchema`], or [`DuckDbError::Attach`].
    #[cfg(feature = "fixtures")]
    pub fn attach_fixture_csv(&self, table: &TableName, path: &Path) -> Result<(), DuckDbError> {
        let literal = path.display().to_string().replace('\'', "''");
        self.attached(
            table,
            path,
            &format!(
                "CREATE OR REPLACE VIEW \"{}\" AS SELECT * FROM read_csv('{literal}', auto_detect=true, types={})",
                table.as_str(),
                duck_types(path)?,
            ),
        )
    }

    /// Runs one `CREATE VIEW` over a CSV. `DuckDB` binds `read_csv` while it prepares, so a file it
    /// cannot read is refused here, before the view exists.
    fn attached(&self, table: &TableName, path: &Path, statement: &str) -> Result<(), DuckDbError> {
        let attach = |cause| DuckDbError::Attach {
            table: String::from(table.as_str()),
            path: path.display().to_string(),
            cause,
        };
        let mut prepared = self.statement(statement).map_err(|error| match error {
            DuckDbError::Prepare { cause } => attach(cause),
            other => other,
        })?;
        prepared.execute().map(drop).map_err(attach)
    }

    /// Refuses credential material this adapter has nowhere to put. [`Self::render`] beside it is one
    /// exhaustive match over the plan shapes, so a third cannot be answered by accident.
    ///
    /// **One exhaustive match, called by both port methods that take a credential.** A copy per
    /// method is two places for the arms to disagree, and the pre-flight is exactly the call where a
    /// missing check would matter least and be noticed least - a statement prepared as the wrong
    /// identity resolves against tables the asker may not be able to see.
    fn deliverable(&self, presented: &Presented) -> Result<(), DuckDbError> {
        match *presented {
            Presented::SharedServiceUser { .. } => {}
            Presented::SubjectToken { .. } | Presented::SubjectPrincipal { .. } => {
                return Err(DuckDbError::NoPlaceForASubject {
                    at: String::from(self.source.as_str()),
                    presented: presented.as_str(),
                });
            }
        }
        // The second half, and it is a different question. The match above compares what arrived
        // against what this CODE can carry; this compares it against what this DEPLOYMENT declared
        // for the source. The capability check goes first because its message is the one that names
        // what a broker did wrong, and after it the only shape left is the shared one - so what this
        // call actually decides is whether the acknowledgement witness on the leg is this source's.
        presented
            .agrees_with(&self.posture, &self.source)
            .map_err(|cause| DuckDbError::PresentedDisagreesWithPosture { cause })
    }

    fn render(executable: Executable<'_>) -> Result<GeneratedQuery, DuckDbError> {
        match executable {
            Executable::Query(plan) => generate(plan, Dialect::DuckDb).map_err(|cause| DuckDbError::Render { cause }),
            Executable::Leg(leg) => generate_leg(leg, Dialect::DuckDb).map_err(|cause| DuckDbError::Render { cause }),
        }
    }

    /// Runs a statement and reads its stream against the materialisation budget, stopping once
    /// `stop` rows are in.
    ///
    /// Read whole before the connection is dropped, and refused at the batch that crosses the budget
    /// rather than after every batch is held - see [`Accumulating`]. The driver hands a stream one
    /// `DuckDB` chunk at a time, so that is the granularity both bounds work at.
    fn answered(&self, query: &GeneratedQuery, stop: Option<usize>) -> Result<ResultBatches, DuckDbError> {
        let mut statement = self.statement(query.sql())?;
        if let Some(bound) = parameter_batch(query.params()).map_err(|cause| DuckDbError::Parameters { cause })? {
            statement.bind(bound).map_err(|cause| DuckDbError::Execute { cause })?;
        }
        let reader = statement.execute().map_err(|cause| DuckDbError::Execute { cause })?;
        let mut accumulating = Accumulating::announcing(reader.schema(), usize::MAX, self.result_budget);
        for batch in reader {
            accumulating
                .push(batch.map_err(|cause| DuckDbError::Batch { cause })?)
                .map_err(|cause| match cause {
                    UnannouncedBatch::OverBudget { most_bytes } => DuckDbError::OverBudget { most_bytes },
                    cause => DuckDbError::Unannounced { cause },
                })?;
            if stop.is_some_and(|most| accumulating.delivered() >= most) {
                break;
            }
        }
        Ok(accumulating.finish())
    }

    /// A boot-path statement's rows, read by the domain's one decode.
    fn run(&self, query: &GeneratedQuery) -> Result<RowSet, DuckDbError> {
        self.answered(query, None)?
            .to_rows()
            .map_err(|cause| DuckDbError::Unreadable { cause })
    }
}

impl Warehouse for DuckDbWarehouse {
    type Error = DuckDbError;

    /// **One process holding one database under one operating-system identity**, so there is
    /// nowhere for a subject's own credential to arrive. The same answer the in-process engine
    /// gives, for the same reason, and it is declared rather than assumed: a source configured
    /// `impersonation-at-source` on this adapter does not start.
    const IMPERSONATION: sutura_domain::source::ImpersonationCapability =
        sutura_domain::source::ImpersonationCapability::NoPlaceForASubject;

    /// This adapter executes a [`LegPlan`](sutura_domain::plan::LegPlan): the differential suite
    /// runs the combiner above its two sources, so a leg it renders is handed to a combiner, never
    /// surfaced as a half-answer.
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &sutura_domain::model::SourceName {
        &self.source
    }

    fn posture(&self) -> &sutura_domain::source::SourcePosture {
        &self.posture
    }

    /// Prepares the statement without running it, as the identity this leg presents.
    ///
    /// Answers [`PreFlight::Accepted`] because it really asked: the driver prepares at
    /// `set_sql_query`, which resolves every table and column name and validates the syntax.
    /// `estimated_bytes` is `None`: preparing reads no plan statistics that would price bytes
    /// touched (`docs/adr/0030`).
    ///
    /// **The deadline is carried, not enforced here; see `docs/adr/0029`** and this crate's limits.
    fn dry_run(&self, executable: Executable<'_>, presented: &Presented, _deadline: Deadline) -> Result<PreFlight, Self::Error> {
        self.deliverable(presented)?;
        let query = Self::render(executable)?;
        drop(self.statement(query.sql())?);
        Ok(PreFlight::Accepted { estimated_bytes: None })
    }

    /// Carried, not enforced here; see [`Self::dry_run`]'s note and `docs/adr/0029`.
    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        self.deliverable(presented)?;
        let query = Self::render(executable)?;
        self.answered(&query, executable.row_limit())
    }

    /// Re-runs an anchor's plan, under the one identity this database was opened with.
    ///
    /// It takes no credential because there is no caller at boot, and it takes an
    /// [`AnchorPlan`](sutura_domain::plan::AnchorPlan) rather than a bare plan, so that the one
    /// method here needing no credential cannot be handed a caller's question either.
    fn verify_anchor(&self, plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        let query = generate(plan.plan(), Dialect::DuckDb).map_err(|cause| DuckDbError::Render { cause })?;
        self.run(&query).map(AnchorRows::of)
    }

    /// Counts a declared join key's values and its distinct values, in one statement.
    ///
    /// **Overridden because it can be**: the driver is a library in this process and the probe is
    /// one aggregate scan with no group, so there is no round trip to trade against.
    fn declared_key(&self, key: DeclaredKey<'_>) -> Result<KeyUniqueness, Self::Error> {
        let query = generate_key_probe(&key, Dialect::DuckDb).map_err(|cause| DuckDbError::Render { cause })?;
        let rows = self.run(&query)?;
        KeyUniqueness::read(&rows).map_err(|cause| DuckDbError::KeyCounts { cause })
    }

    /// The MATERIALISATION BUDGET alone: a result refused for crossing it is a governance outcome a
    /// caller cannot retry past. Every other failure shape stays `false`.
    fn result_did_not_fit(&self, error: &Self::Error) -> bool {
        matches!(*error, DuckDbError::OverBudget { .. })
    }
}

/// The `types` argument for [`DuckDbWarehouse::attach_fixture_csv`].
/// Reads the fixture bytes, infers the shared type of every column, and renders the complete
/// `DuckDB` type map. Naming every type keeps booleans and wide integers on the same boundary as
/// the other fixture adapters instead of inheriting `read_csv_auto`'s separate inference rules.
#[cfg(feature = "fixtures")]
fn duck_types(path: &Path) -> Result<String, DuckDbError> {
    let text = std::fs::read_to_string(path).map_err(|cause| DuckDbError::FixtureRead {
        path: path.display().to_string(),
        cause,
    })?;
    let columns = sutura_domain::warehouse::csv::infer(&text).map_err(|cause| DuckDbError::FixtureSchema {
        path: path.display().to_string(),
        cause,
    })?;
    let mapped = columns
        .into_iter()
        .map(|column| {
            let kind = match column.kind() {
                sutura_domain::warehouse::csv::FixtureType::Boolean => String::from("BOOLEAN"),
                sutura_domain::warehouse::csv::FixtureType::Integer => String::from("BIGINT"),
                sutura_domain::warehouse::csv::FixtureType::WideInteger => String::from("UBIGINT"),
                sutura_domain::warehouse::csv::FixtureType::Decimal { scale } => format!("DECIMAL(38,{scale})"),
                sutura_domain::warehouse::csv::FixtureType::Real => String::from("DOUBLE"),
                sutura_domain::warehouse::csv::FixtureType::Date => String::from("DATE"),
                sutura_domain::warehouse::csv::FixtureType::Text => String::from("VARCHAR"),
            };
            format!("'{}': '{kind}'", column.name().as_str())
        })
        .collect::<Vec<_>>();
    Ok(format!("{{{}}}", mapped.join(", ")))
}

/// What the driver hands over, read through the domain's one decode.
///
/// **The type table lives once, in the domain** (`crates/sutura-domain/src/warehouse/arrow/tests.rs`),
/// because this adapter no longer maps a value of its own. What these cells hold is the other half:
/// which Arrow type the pinned driver gives each `DuckDB` type, through a real in-memory database -
/// the widths a Parquet file has and a CSV never will, which the golden suite cannot reach.
#[cfg(test)]
mod tests;
