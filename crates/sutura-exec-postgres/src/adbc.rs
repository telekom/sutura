//! The PostgreSQL adapter's one transport, the second adapter on `sutura-adbc` (`telekom/sutura#913`).
//!
//! The [`Warehouse`](sutura_domain::warehouse::Warehouse) port over the self-built driver
//! (`nix/postgres-adbc.nix`), answering with [`PostgresError`](crate::PostgresError). A certified
//! answer's Arrow batches are handed on as they arrive, except a `NUMERIC` column, which `numeric`
//! re-reads per cell. **Every `kind: postgres` source is answered here**: the composition root
//! constructs [`AdbcPostgres`](crate::adbc::AdbcPostgres) for each, over the driver this artefact
//! links (both musl triples) or the one `SUTURA_POSTGRES_ADBC_DRIVER` names.
//!
//! # What holds what
//!
//! - **Caller text reaches the driver only through `execute`.** The pinned driver
//!   (`apache-arrow-adbc-24`) sends a parameterless `execute_update` through `PQexec`, the simple
//!   protocol, which runs every statement of a multi-statement string; `execute` asks for a result
//!   stream and goes through `PQprepare`, where the server refuses a second statement at `Parse`.
//!   So `session`'s `Setting::apply` is the one shipped `execute_update` here, its text is fixed
//!   literals and a [`NonZeroU32`](core::num::NonZeroU32) (the `fixtures` loader is the other, in no
//!   release), and `clippy.toml` bans every other call
//!   in the workspace - except one written inside an existing `disallowed_methods` expectation's
//!   scope, which that expectation covers too (the ban's own entry states it).
//! - **The per-request deadline is `SET LOCAL statement_timeout`** in the transaction the driver
//!   opens when autocommit is switched off, clamped to the deployment's ceiling
//!   (`SUTURA_DEV_STATEMENT_TIMEOUT_MS`), and always rolled back. A statement the server cancelled for it
//!   (`57014`), or a stream that failed once that timeout had run out, is the deadline to
//!   `Warehouse::deadline_exceeded`.
//! - **The channel is the declared one.** [`Conninfo`](crate::adbc::Conninfo) builds the libpq
//!   connection string from the declaration and classifies every libpq keyword, pinning each one the
//!   environment could weaken; its own header carries what it refuses and what it cannot pin.
//! - **The driver is the linked one where there is one.**
//!   [`PostgresDriver`](crate::adbc::PostgresDriver) is the archive this artefact links (both musl
//!   triples) or a mounted `.so` (every other build).
//!
//! # Limits
//!
//! - **`NUMERIC` is read exactly** (`numeric`): scale-zero text that fits an `i64` is an integer,
//!   the rest exact text - except a domain over it, which the driver tags with the domain's name and
//!   so stays text. `tests/types.rs` holds each mapped type against the tier through the real driver.
//! - **The cells here are a fake connection**; the integration tests are what reach a server through
//!   a real driver (mounted on a host, linked in `nix/shipped.nix`'s musl test build). The server
//!   refusing a multi-statement string at `Parse` is held there; the driver's `BEGIN` on
//!   autocommit-off is read off its source and observed only through `SET LOCAL` taking effect.
//!   `tests/kerberos.rs`'s Kerberos sign-in and its refused negative control (a declared service the
//!   KDC does not know) run through the linked `x86_64` musl driver, in its CI venue alone.
//! - **Loading and connecting are outside the deadline**: the driver is loaded and a connection
//!   opened per call, and only the statement runs under `SET LOCAL`.
//! - **Every port method.** `session`'s header says what each sends.
//! - **One declared identity.** A source signs in only as its declared shared service account: with
//!   a password or a client certificate, or as the one Kerberos principal the process's named
//!   credential cache holds, when [`Conninfo::kerberos`](crate::adbc::Conninfo::kerberos) declares it
//!   (the linked libpq through a static MIT krb5, `nix/postgres-adbc.nix`). No settings key selects
//!   Kerberos yet. OAuth and per-caller sign-in are not supported: [`Conninfo`](crate::adbc::Conninfo)
//!   refuses SSPI and OAuth on either driver, and the linked libpq is built without libcurl.

mod conninfo;
mod numeric;
mod session;

use std::path::PathBuf;

use adbc_core::error::Error as CoreError;
use adbc_core::options::{AdbcVersion, OptionDatabase, OptionValue};
use adbc_core::{Database as _, Driver as _};
use adbc_driver_manager::{ManagedConnection, ManagedDriver};
pub use conninfo::{Channel, Conninfo, GssEncryption, InvalidKerberosService, Kerberos, KerberosService, UnusableChannel};
pub use sutura_adbc::UnusableDriverPath;
use sutura_adbc::{DriverLocation, parameter_batch};
use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::raw::RawStatement;
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::cardinality::{DeclaredKey, KeyUniqueness};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{
    AnchorRows, PreFlight, RawExecution, RawRows, ResultBatches, RowSet, UnannouncedBatch, Warehouse,
};
use sutura_sql::generate::{generate, generate_key_probe, generate_leg};
use sutura_sql::{Dialect, GeneratedQuery};

use crate::PostgresError;
use crate::deadline::refuse_if_spent;

/// Why this transport could not answer.
#[derive(Debug, thiserror::Error)]
pub enum AdbcError {
    #[error("could not load the PostgreSQL ADBC driver")]
    Load(#[source] CoreError),
    #[error("an ADBC call to the PostgreSQL driver failed")]
    Adbc(#[source] CoreError),
    #[error("could not read a result batch")]
    Batch(#[source] arrow_schema::ArrowError),
    /// The stream failed once the statement's timeout had run out - the server cancelling it,
    /// read by the clock where the stream carries no SQLSTATE (`session::timed_out`).
    #[error("the source stopped the statement at its timeout")]
    TimedOut(#[source] arrow_schema::ArrowError),
    #[error("the ADBC result stream did not match its announced schema")]
    Unannounced(#[source] UnannouncedBatch),
    #[error("the plan's values could not be assembled for binding")]
    Parameters(#[source] arrow_schema::ArrowError),
    #[error("a result cell could not be read")]
    Unreadable(#[source] sutura_domain::warehouse::UnreadableCell),
    /// Spent once the connection was open - refused locally, as [`PostgresError::DeadlineSpent`].
    #[error("the deadline was already spent by the time the connection was open")]
    DeadlineSpent,
}

/// Where the PostgreSQL driver comes from: this artefact's own link, or a mounted `.so`.
///
/// Not `sutura_adbc::DriverLocation`, whose linked route is the `BigQuery` archive; a mounted path is
/// parsed by it, so an empty or relative one is refused exactly as for every ADBC adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresDriver(Route);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Route {
    Linked,
    Mounted(PathBuf),
}

/// The variable a host that links no driver names a mounted one with.
///
/// **Not a settings key**: which driver file a host carries is a property of the host rather than of
/// the semantic deployment, and a release artefact that links one never reads it - the order
/// `bigquery_driver` in `sutura-cli` gives for the other ADBC adapter, for its reason: a mounted
/// path must not be able to displace the driver a published artefact carries.
pub const MOUNTED_DRIVER: &str = "SUTURA_POSTGRES_ADBC_DRIVER";

/// Why this process has no PostgreSQL driver to open.
#[derive(Debug, thiserror::Error)]
pub enum NoDriver {
    #[error(
        "this build links no PostgreSQL ADBC driver and `{mounted}` is not set. A published \
         musl artefact carries its own; any other build points that variable at an absolute \
         libadbc_driver_postgresql shared library",
        mounted = MOUNTED_DRIVER
    )]
    Unset,
    #[error("`{mounted}` does not name a driver this process can open", mounted = MOUNTED_DRIVER)]
    Unusable(#[source] UnusableDriverPath),
}

impl PostgresDriver {
    /// The driver this process opens: the one this artefact links, else the one
    /// [`MOUNTED_DRIVER`] names.
    ///
    /// # Errors
    ///
    /// [`NoDriver`] where neither is there, or the named path is not one.
    pub fn from_host() -> Result<Self, NoDriver> {
        if let Some(linked) = Self::linked_in() {
            return Ok(linked);
        }
        let named = std::env::var(MOUNTED_DRIVER).map_err(|_absent| NoDriver::Unset)?;
        Self::parse(&named).map_err(NoDriver::Unusable)
    }

    /// The driver this artefact links, or `None` where it links none.
    #[must_use]
    pub fn linked_in() -> Option<Self> {
        sutura_adbc::LINKS_POSTGRES_DRIVER.then_some(Self(Route::Linked))
    }

    /// Parses a mounted driver's path.
    ///
    /// # Errors
    ///
    /// [`UnusableDriverPath`] for an empty or relative path. Whether a driver is there is the
    /// load's question, asked by the first call.
    pub fn parse(named: &str) -> Result<Self, UnusableDriverPath> {
        DriverLocation::parse(named).map(|_| Self(Route::Mounted(PathBuf::from(named))))
    }

    /// Loads and initialises the driver, opening no database - what `sutura doctor` asks.
    ///
    /// # Errors
    ///
    /// [`AdbcError::Load`], from either route.
    pub fn probe(&self) -> Result<(), AdbcError> {
        self.load().map(drop)
    }

    fn load(&self) -> Result<ManagedDriver, AdbcError> {
        match self.0 {
            Route::Linked => sutura_adbc::linked_postgres_driver(),
            Route::Mounted(ref path) => ManagedDriver::load_dynamic_from_filename(path, None, AdbcVersion::default()),
        }
        .map_err(AdbcError::Load)
    }
}

impl core::fmt::Display for PostgresDriver {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            Route::Linked => f.write_str("linked into this binary"),
            Route::Mounted(ref path) => write!(f, "mounted at {}", path.display()),
        }
    }
}

/// A PostgreSQL source reached through its ADBC driver, behind the [`Warehouse`] port.
#[derive(Debug)]
pub struct AdbcPostgres {
    source: SourceName,
    posture: SourcePosture,
    driver: PostgresDriver,
    conninfo: Conninfo,
    statement_timeout_ceiling_ms: u32,
}

impl AdbcPostgres {
    /// Takes the source, the driver and the connection string it connects with.
    ///
    /// Reads the deployment's statement-timeout ceiling (`SUTURA_DEV_STATEMENT_TIMEOUT_MS`), which a
    /// request's own budget may only narrow.
    ///
    /// # Errors
    ///
    /// [`PostgresError::InvalidStatementTimeout`] where that tuning value is not a millisecond count.
    pub fn new(
        source: SourceName,
        posture: SourcePosture,
        driver: PostgresDriver,
        conninfo: Conninfo,
    ) -> Result<Self, PostgresError> {
        Ok(Self {
            source,
            posture,
            driver,
            conninfo,
            statement_timeout_ceiling_ms: crate::statement_timeout_ms()?,
        })
    }

    fn connection(&self) -> Result<ManagedConnection, AdbcError> {
        open_connection(&self.driver, &self.conninfo)
    }

    /// Connects, and runs `step` on the connection.
    fn connected<T>(&self, step: impl FnOnce(&mut ManagedConnection) -> Result<T, AdbcError>) -> Result<T, PostgresError> {
        let mut connection = self.connection()?;
        Ok(step(&mut connection)?)
    }

    /// The raw tool's one statement, `READ ONLY` - `docs/adr/0013`'s three properties, `session::raw`.
    fn raw(&self, statement: &RawStatement, presented: &Presented, deadline: Deadline) -> Result<RawRows, PostgresError> {
        crate::deliverable(&self.source, &self.posture, presented)?;
        refuse_if_spent(deadline)?;
        let ceiling = self.statement_timeout_ceiling_ms;
        raw_rows(&self.connected(|connection| session::raw(connection, statement.as_str(), ceiling, deadline))?)
    }

    /// A boot-path statement's rows: no deadline, the connect-time ceiling alone.
    fn boot(&self, query: &GeneratedQuery) -> Result<RowSet, PostgresError> {
        let bound = parameter_batch(query.params()).map_err(AdbcError::Parameters)?;
        let batches =
            self.connected(|connection| session::boot(connection, query.sql(), bound, self.statement_timeout_ceiling_ms))?;
        numeric::rows(&batches)
    }

    /// Creates `schema` if it is not there - the per-cell isolation a fixture opens with
    /// [`Conninfo::in_schema`].
    ///
    /// # Errors
    ///
    /// [`PostgresError::Fixture`] where the server refused it, and the driver's load or connect.
    #[cfg(feature = "fixtures")]
    pub fn create_schema(&self, schema: &str) -> Result<(), PostgresError> {
        let statement = format!("CREATE SCHEMA IF NOT EXISTS \"{}\"", schema.replace('"', "\"\""));
        self.loaded(schema, "<schema>", &statement)
    }

    /// Exposes a CSV as a table: infers column types, recreates the table, then inserts the rows -
    /// re-inferred from the committed file on every run, so it cannot drift from it.
    ///
    /// # Errors
    ///
    /// [`PostgresError::FixtureRead`], [`PostgresError::InvalidColumnName`], or
    /// [`PostgresError::Fixture`] where the server refused the load.
    #[cfg(feature = "fixtures")]
    pub fn load_csv(&self, table: &sutura_domain::model::TableName, path: &std::path::Path) -> Result<(), PostgresError> {
        let schema =
            crate::importer::infer_schema(&read_fixture(path)?).map_err(|cause| PostgresError::InvalidColumnName { cause })?;
        self.loaded(table.as_str(), &path.display().to_string(), &schema.load_statement(table))
    }

    /// Exposes a conformance fixture with the same exact types as the other adapter bindings.
    ///
    /// # Errors
    ///
    /// [`PostgresError::FixtureRead`], [`PostgresError::FixtureSchema`], or
    /// [`PostgresError::Fixture`] where the server refused the load.
    #[cfg(feature = "fixtures")]
    pub fn load_fixture_csv(&self, table: &sutura_domain::model::TableName, path: &std::path::Path) -> Result<(), PostgresError> {
        let schema = crate::importer::infer_fixture_schema(&read_fixture(path)?)
            .map_err(|cause| PostgresError::FixtureSchema { cause })?;
        self.loaded(table.as_str(), &path.display().to_string(), &schema.load_statement(table))
    }

    #[cfg(feature = "fixtures")]
    fn loaded(&self, table: &str, path: &str, statement: &str) -> Result<(), PostgresError> {
        let fixture = |cause| PostgresError::Fixture {
            table: String::from(table),
            path: String::from(path),
            cause,
        };
        let mut connection = self.connection().map_err(fixture)?;
        session::load(&mut connection, statement).map_err(fixture)
    }
}

/// Loads `driver` and opens one connection over `conninfo`. It keeps its database and driver alive
/// itself.
fn open_connection(driver: &PostgresDriver, conninfo: &Conninfo) -> Result<ManagedConnection, AdbcError> {
    let mut driver = driver.load()?;
    #[expect(
        clippy::disallowed_methods,
        reason = "the libpq connection string is the driver's credential, and handing it to the driver is its purpose"
    )]
    let uri = OptionValue::String(conninfo.secret().expose_secret().to_owned());
    let database = driver
        .new_database_with_opts([(OptionDatabase::Uri, uri)])
        .map_err(AdbcError::Adbc)?;
    database.new_connection().map_err(AdbcError::Adbc)
}

/// A plain second connection to the fixture tier, for what no port method may do.
///
/// It creates a view, or holds a lock inside a transaction it leaves open until it is dropped. It
/// runs whatever it is handed, through the simple protocol - which is why it exists only under the
/// `fixtures` feature.
#[cfg(feature = "fixtures")]
pub struct FixtureAdmin(ManagedConnection);

#[cfg(feature = "fixtures")]
impl FixtureAdmin {
    /// One connection over `conninfo`.
    ///
    /// # Errors
    ///
    /// The driver's load or connect.
    pub fn open(driver: &PostgresDriver, conninfo: &Conninfo) -> Result<Self, AdbcError> {
        open_connection(driver, conninfo).map(Self)
    }

    /// Runs `sql`, every statement in it, on this connection.
    ///
    /// # Errors
    ///
    /// [`AdbcError::Adbc`] where the server refused it.
    pub fn run(&mut self, sql: &str) -> Result<(), AdbcError> {
        session::load(&mut self.0, sql)
    }
}

/// A fixture file's text.
#[cfg(feature = "fixtures")]
fn read_fixture(path: &std::path::Path) -> Result<String, PostgresError> {
    std::fs::read_to_string(path).map_err(|cause| PostgresError::FixtureRead {
        path: path.display().to_string(),
        cause,
    })
}

impl From<AdbcError> for PostgresError {
    /// The deadline spent locally is one variant wherever it is noticed, and the server's
    /// `division by zero` keeps its own; every other failure is carried whole.
    fn from(cause: AdbcError) -> Self {
        match cause {
            AdbcError::DeadlineSpent => Self::DeadlineSpent,
            cause if session::division_by_zero(&cause) => Self::DivisionByZero { cause },
            cause => Self::Adbc { cause },
        }
    }
}

/// The plan, or a federated leg of one, as one Postgres statement.
fn render(executable: Executable<'_>) -> Result<GeneratedQuery, PostgresError> {
    match executable {
        Executable::Query(plan) => generate(plan, Dialect::Postgres),
        Executable::Leg(leg) => generate_leg(leg, Dialect::Postgres),
    }
    .map_err(|cause| PostgresError::Render { cause })
}

/// A raw statement's rows, `NUMERIC` read exactly and cut to one past the cap, which the stream
/// already stopped near.
fn raw_rows(batches: &ResultBatches) -> Result<RawRows, PostgresError> {
    let (columns, mut rows) = numeric::rows(batches)?.into_parts();
    rows.truncate(session::RAW_ROWS);
    Ok(RawRows::of(columns, rows))
}

impl Warehouse for AdbcPostgres {
    type Error = PostgresError;

    /// One connection string under the deployment's declared identity, so nowhere for a subject's
    /// own credential to arrive - the static-credential half. A source configured
    /// `impersonation-at-source` on this adapter does not start.
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    /// The one adapter this build links that may accept a raw statement at all - `docs/adr/0013`'s
    /// showcase source. The statement is handed to the server unexamined; Postgres's own parser and
    /// its own `GRANT`/`REVOKE` model are what authorize or refuse it, inside `READ ONLY`.
    const ACCEPTS_RAW_STATEMENTS: bool = true;

    /// A [`LegPlan`](sutura_domain::plan::LegPlan) renders through `generate_leg` at
    /// [`Dialect::Postgres`] and runs as any other statement; `tests/conformance.rs` binds the packs
    /// `executes_legs`. Identity is unchanged - two Postgres legs are both `shared-service-user`.
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    /// Parsed and described by the server inside the deadline, never run - `session::described`.
    ///
    /// `estimated_bytes` is `None`: Postgres plans in rows and a cost unit, not bytes, and folding
    /// that into a byte-denominated budget would need a conversion this adapter does not attempt
    /// (`docs/adr/0030`).
    fn dry_run(&self, executable: Executable<'_>, presented: &Presented, deadline: Deadline) -> Result<PreFlight, Self::Error> {
        crate::deliverable(&self.source, &self.posture, presented)?;
        let query = render(executable)?;
        refuse_if_spent(deadline)?;
        let ceiling = self.statement_timeout_ceiling_ms;
        self.connected(|connection| session::described(connection, query.sql(), ceiling, deadline))?;
        Ok(PreFlight::Accepted { estimated_bytes: None })
    }

    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        crate::deliverable(&self.source, &self.posture, presented)?;
        let query = render(executable)?;
        refuse_if_spent(deadline)?;
        let bound = parameter_batch(query.params()).map_err(AdbcError::Parameters)?;
        let ceiling = self.statement_timeout_ceiling_ms;
        let batches = self.connected(|connection| session::answer(connection, query.sql(), bound, ceiling, deadline))?;
        numeric::batches(batches)
    }

    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        let query = generate(plan.plan(), Dialect::Postgres).map_err(|cause| PostgresError::Render { cause })?;
        self.boot(&query).map(AnchorRows::of)
    }

    fn declared_key(&self, key: DeclaredKey<'_>) -> Result<KeyUniqueness, Self::Error> {
        let query = generate_key_probe(&key, Dialect::Postgres).map_err(|cause| PostgresError::Render { cause })?;
        KeyUniqueness::read(&self.boot(&query)?).map_err(|cause| PostgresError::KeyCounts { cause })
    }

    fn execute_raw(&self, statement: &RawStatement, presented: &Presented, deadline: Deadline) -> RawExecution<Self::Error> {
        Some(self.raw(statement, presented, deadline))
    }

    fn source_refused(&self, error: &Self::Error) -> bool {
        matches!(*error, PostgresError::Adbc { ref cause } if session::source_refused(cause))
    }

    fn deadline_exceeded(&self, error: &Self::Error) -> bool {
        match *error {
            PostgresError::DeadlineSpent => true,
            PostgresError::Adbc { ref cause } => session::deadline_exceeded(cause),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests;
