//! The ADBC transport for PostgreSQL, the second adapter on `sutura-adbc` (`telekom/sutura#913`).
//!
//! One certified statement through the self-built driver (`nix/postgres-adbc.nix`), its Arrow
//! batches handed on as the port's [`ResultBatches`](sutura_domain::warehouse::ResultBatches) with
//! no per-cell walk of this crate's own.
//!
//! **Default-off, and wired to nothing.** No composition root constructs
//! [`AdbcPostgres`](crate::adbc::AdbcPostgres) and no `Warehouse` method reaches it: the
//! `tokio-postgres` path in `lib.rs` still answers every Postgres source. Whether and when that
//! changes is the cutover's decision, not this module's.
//!
//! # What holds what
//!
//! - **Caller text reaches the driver only through `execute`.** The pinned driver
//!   (`apache-arrow-adbc-24`) sends a parameterless `execute_update` through `PQexec`, the simple
//!   protocol, which runs every statement of a multi-statement string; `execute` asks for a result
//!   stream and goes through `PQprepare`, where the server refuses a second statement at `Parse`.
//!   So `LocalTimeout::apply` is the one `execute_update` in this module, its text is a fixed
//!   literal and a [`NonZeroU32`](core::num::NonZeroU32), and `clippy.toml` bans every other call
//!   in the workspace.
//! - **The per-request deadline is `SET LOCAL statement_timeout`** in the transaction the driver
//!   opens when autocommit is switched off, clamped to the same connect-time ceiling the
//!   `tokio-postgres` path uses, and always rolled back. A statement the server cancelled for it
//!   (`57014`) reads as
//!   [`AdbcPostgres::deadline_exceeded`](crate::adbc::AdbcPostgres::deadline_exceeded), the same
//!   split `deadline.rs` draws.
//! - **No linked-in driver.** `sutura-adbc` links exactly one archive under one `AdbcDriverInit`
//!   symbol, and it is the `BigQuery` driver's - so a PostgreSQL transport that took
//!   `sutura_adbc::linked_driver()` would open the wrong driver.
//!   [`MountedDriver`](crate::adbc::MountedDriver) has no linked spelling, which is why this
//!   transport takes it rather than a `DriverLocation`.
//!
//! # Limits
//!
//! - **`NUMERIC` drifts.** The driver maps `NUMERIC` to Arrow `Utf8` by OID whatever the scale, so
//!   `SELECT 1::numeric` is `Value::Text("1")` here and `Value::Integer(1)` on the `tokio-postgres`
//!   path; a fractional value is the same text on both. The cell
//!   `a_scale_zero_numeric_is_text_here_where_the_tokio_postgres_path_reads_an_integer` pins the
//!   pair. The parity of every other type with `lib.rs`'s `cell` is unmeasured.
//! - **No run against a real driver.** Every cell here is a fake connection. The server refusing a
//!   multi-statement string at `Parse`, the driver carrying `57014` in `sqlstate`, its `NUMERIC`
//!   mapping, its bind types and its `BEGIN` on autocommit-off are read off the driver's source,
//!   not observed.
//! - **Loading and connecting are outside the deadline**: the driver is loaded and a connection
//!   opened per call, and only the statement runs under `SET LOCAL`.
//! - **The channel is whatever the URI says.** libpq reads `sslmode` and friends from it; the
//!   declared `SourceTransport` a composition root turns into a `rustls::ClientConfig` for the
//!   `tokio-postgres` path is not applied here.
//! - **Only `execute`'s shape**: no `dry_run`, no raw statement, no boot-path call.

use core::num::{NonZeroU32, NonZeroUsize};
use std::path::PathBuf;
use std::time::Instant;

use adbc_core::error::Error as CoreError;
use adbc_core::options::{AdbcVersion, OptionConnection, OptionDatabase, OptionValue};
use adbc_core::{Connection, Database as _, Driver as _, Statement};
use adbc_driver_manager::ManagedDriver;
use arrow_array::{RecordBatch, RecordBatchReader};
pub use sutura_adbc::UnusableDriverPath;
use sutura_adbc::{DriverLocation, parameter_batch};
use sutura_domain::identity::Secret;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{Accumulating, ResultBatches, ResultBudget, UnannouncedBatch};
use sutura_sql::GeneratedQuery;

use crate::PostgresError;
use crate::deadline::deadline_statement_timeout_ms;

/// The rows one result stream may deliver before it is refused while being read - the `BigQuery`
/// transport's ceiling, for its reason: a federation leg carries no `LIMIT`, so this is what makes
/// a stream finite. `tests::a_stream_past_the_row_ceiling_is_refused_while_it_is_read` holds it.
const MOST_RESULT_ROWS: usize = 1_000_000;

/// The bytes one result stream may materialise, the `BigQuery` transport's budget.
const MOST_RESULT_BYTES: NonZeroUsize = match NonZeroUsize::new(256 * 1024 * 1024) {
    Some(bytes) => bytes,
    None => NonZeroUsize::MIN,
};

/// The SQLSTATE a statement cancelled by `statement_timeout` fails with, `query_canceled`.
const QUERY_CANCELED: [u8; 5] = *b"57014";

/// Why this transport could not answer.
#[derive(Debug, thiserror::Error)]
pub enum AdbcError {
    #[error("could not load the PostgreSQL ADBC driver")]
    Load(#[source] CoreError),
    #[error("an ADBC call to the PostgreSQL driver failed")]
    Adbc(#[source] CoreError),
    #[error("could not read a result batch")]
    Batch(#[source] arrow_schema::ArrowError),
    #[error("the ADBC result stream did not match its announced schema")]
    Unannounced(#[source] UnannouncedBatch),
    #[error("the plan's values could not be assembled for binding")]
    Parameters(#[source] arrow_schema::ArrowError),
    /// Spent before anything was sent - refused locally, the `tokio-postgres` path's `DeadlineSpent`.
    #[error("the deadline was already spent by the time the connection was open")]
    DeadlineSpent,
}

/// A driver this deployment mounted, at an absolute path - and never the linked-in route.
///
/// Parsed by `sutura_adbc::DriverLocation::parse`, so an empty or relative path is refused exactly
/// as it is for every ADBC adapter; kept as a path because the only other thing a `DriverLocation`
/// can be is the `BigQuery` archive this module's header describes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountedDriver(PathBuf);

impl MountedDriver {
    /// Parses a mounted driver's path.
    ///
    /// # Errors
    ///
    /// [`UnusableDriverPath`] for an empty or relative path. Whether a driver is there is the
    /// load's question, asked by the first call.
    pub fn parse(named: &str) -> Result<Self, UnusableDriverPath> {
        DriverLocation::parse(named).map(|_| Self(PathBuf::from(named)))
    }
}

/// A PostgreSQL source reached through its ADBC driver.
pub struct AdbcPostgres {
    driver: MountedDriver,
    uri: Secret,
    statement_timeout_ceiling_ms: u32,
}

impl core::fmt::Debug for AdbcPostgres {
    /// Hand-written so the URI - a libpq connection string, password included - is never printed.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AdbcPostgres")
            .field("driver", &self.driver)
            .finish_non_exhaustive()
    }
}

impl AdbcPostgres {
    /// Takes the driver and the libpq URI it connects with.
    ///
    /// Reads the connect-time ceiling the `tokio-postgres` path reads
    /// (`SUTURA_DEV_STATEMENT_TIMEOUT_MS`).
    ///
    /// # Errors
    ///
    /// [`PostgresError::InvalidStatementTimeout`] where that tuning value is not a millisecond count.
    pub fn new(driver: MountedDriver, uri: Secret) -> Result<Self, PostgresError> {
        Ok(Self {
            driver,
            uri,
            statement_timeout_ceiling_ms: crate::statement_timeout_ms()?,
        })
    }

    /// Runs one rendered statement and hands its batches on.
    ///
    /// # Errors
    ///
    /// [`AdbcError`]; [`Self::deadline_exceeded`] says which of them is the deadline.
    pub fn execute(&self, query: &GeneratedQuery, deadline: Deadline) -> Result<ResultBatches, AdbcError> {
        let bound = parameter_batch(query.params()).map_err(AdbcError::Parameters)?;
        let mut driver =
            ManagedDriver::load_dynamic_from_filename(&self.driver.0, None, AdbcVersion::default()).map_err(AdbcError::Load)?;
        #[expect(
            clippy::disallowed_methods,
            reason = "the libpq connection string is the driver's credential, and handing it to the driver is its purpose"
        )]
        let uri = OptionValue::String(self.uri.expose_secret().to_owned());
        let database = driver
            .new_database_with_opts([(OptionDatabase::Uri, uri)])
            .map_err(AdbcError::Adbc)?;
        let mut connection = database.new_connection().map_err(AdbcError::Adbc)?;
        answer(
            &mut connection,
            query.sql(),
            bound,
            self.statement_timeout_ceiling_ms,
            deadline,
        )
    }

    /// Whether `error` is the deadline: spent before sending, or the server's `57014`.
    ///
    /// `57014` is also what a manual `pg_cancel_backend` produces, which this cannot tell apart -
    /// `deadline.rs`'s limit, unchanged.
    #[must_use]
    pub fn deadline_exceeded(error: &AdbcError) -> bool {
        match *error {
            AdbcError::DeadlineSpent => true,
            AdbcError::Adbc(ref cause) => cause.sqlstate.map(|c| u8::try_from(c).unwrap_or(0)) == QUERY_CANCELED,
            AdbcError::Load(_) | AdbcError::Batch(_) | AdbcError::Unannounced(_) | AdbcError::Parameters(_) => false,
        }
    }
}

/// `SET LOCAL statement_timeout`, the one text this module sends through `execute_update`.
///
/// A [`NonZeroU32`] and a fixed literal, so nothing a caller wrote can reach the simple protocol
/// through it. Zero would read as *no timeout* to the server; the type rules it out.
struct LocalTimeout(NonZeroU32);

impl LocalTimeout {
    fn sql(&self) -> String {
        format!("SET LOCAL statement_timeout = {}", self.0)
    }

    fn apply<S>(&self, statement: &mut S) -> Result<(), AdbcError>
    where
        S: Statement,
    {
        statement.set_sql_query(self.sql()).map_err(AdbcError::Adbc)?;
        #[expect(
            clippy::disallowed_methods,
            reason = "the one execute_update: its text is `LocalTimeout::sql`, a fixed literal and an integer"
        )]
        let updated = statement.execute_update();
        updated.map(drop).map_err(AdbcError::Adbc)
    }
}

/// The statement under `deadline`, inside a transaction the driver opens and this always rolls back.
///
/// Generic over the connection so a fake can stand in for the driver: every step this module owns
/// happens here, and `AdbcPostgres::execute` only loads and connects.
fn answer<C>(
    connection: &mut C,
    sql: &str,
    bound: Option<RecordBatch>,
    ceiling_ms: u32,
    deadline: Deadline,
) -> Result<ResultBatches, AdbcError>
where
    C: Connection,
{
    if deadline.remaining_at(Instant::now()).is_none() {
        return Err(AdbcError::DeadlineSpent);
    }
    let timeout = LocalTimeout(deadline_statement_timeout_ms(ceiling_ms, deadline));
    // `SET LOCAL` outside a transaction is a warning and a no-op, so the transaction is what makes
    // the deadline real: the driver issues `BEGIN` itself once autocommit is off.
    connection
        .set_option(OptionConnection::AutoCommit, OptionValue::from(false))
        .map_err(AdbcError::Adbc)?;
    let outcome = within(connection, sql, bound, &timeout);
    // Nothing on this path commits. A failed rollback leaves a transaction the connection's drop
    // discards anyway, so its error is not the caller's.
    drop(connection.rollback());
    outcome
}

fn within<C>(
    connection: &mut C,
    sql: &str,
    bound: Option<RecordBatch>,
    timeout: &LocalTimeout,
) -> Result<ResultBatches, AdbcError>
where
    C: Connection,
{
    timeout.apply(&mut connection.new_statement().map_err(AdbcError::Adbc)?)?;
    let mut statement = connection.new_statement().map_err(AdbcError::Adbc)?;
    statement.set_sql_query(sql).map_err(AdbcError::Adbc)?;
    if let Some(batch) = bound {
        statement.bind(batch).map_err(AdbcError::Adbc)?;
    }
    drained(statement.execute().map_err(AdbcError::Adbc)?)
}

/// Reads a stream into the port's batches, refusing past either ceiling at the batch that crosses it.
fn drained(reader: Box<dyn RecordBatchReader + Send>) -> Result<ResultBatches, AdbcError> {
    let mut accumulating = Accumulating::announcing(reader.schema(), MOST_RESULT_ROWS, ResultBudget::of_bytes(MOST_RESULT_BYTES));
    for batch in reader {
        accumulating
            .push(batch.map_err(AdbcError::Batch)?)
            .map_err(AdbcError::Unannounced)?;
    }
    Ok(accumulating.finish())
}

#[cfg(test)]
mod tests {
    //! Every step `answer` owns, asserted over a fake connection that records what reached it.
    //! The last cell drives `AdbcPostgres::execute` itself over a path naming no driver, which is
    //! the negative control: each refusal above it has to arrive INSTEAD of that load failure.

    use core::cell::RefCell;
    use core::ffi::c_char;
    use std::collections::HashSet;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use adbc_core::error::{Error as CoreError, Result as AdbcResult, Status};
    use adbc_core::options::{InfoCode, ObjectDepth, OptionConnection, OptionStatement, OptionValue};
    use adbc_core::{Connection, Optionable, PartitionedResult, Statement};
    use arrow_array::{ArrayRef, Int8Array, RecordBatch, RecordBatchIterator, RecordBatchReader, StringArray};
    use arrow_schema::{Field, Schema};
    use sutura_domain::calendar::Date;
    use sutura_domain::identity::Secret;
    use sutura_domain::model::SourceName;
    use sutura_domain::warehouse::deadline::{Budget, Deadline};
    use sutura_domain::warehouse::{ParamValue, ResultBatches, UnannouncedBatch, Value};
    use sutura_sql::GeneratedQuery;

    use super::{AdbcError, AdbcPostgres, MountedDriver, answer};

    /// What reached the fake driver, in order.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Sent {
        AutoCommit(String),
        Update(String),
        Bound { rows: usize, columns: usize },
        Query(String),
        Rollback,
    }

    type Log = Rc<RefCell<Vec<Sent>>>;

    /// What `answer` returned, beside what reached the fake on the way.
    type Ran = (Result<ResultBatches, AdbcError>, Vec<Sent>);

    /// The connect-time ceiling every cell hands over, `SUTURA_DEV_STATEMENT_TIMEOUT_MS`'s default.
    const CEILING_MS: u32 = 15_000;

    fn not_asked(what: &str) -> CoreError {
        CoreError::with_message_and_status(format!("the fake driver was not expected to {what}"), Status::NotImplemented)
    }

    struct FakeConnection {
        log: Log,
        /// What `execute` answers with: a batch, or `None` for a statement the server refused.
        reply: Option<RecordBatch>,
    }

    struct FakeStatement {
        log: Log,
        reply: Option<RecordBatch>,
        sql: String,
    }

    impl Optionable for FakeConnection {
        type Option = OptionConnection;

        fn set_option(&mut self, key: Self::Option, value: OptionValue) -> AdbcResult<()> {
            if matches!(key, OptionConnection::AutoCommit) {
                let text = if let OptionValue::String(ref text) = value {
                    text.clone()
                } else {
                    String::from("<not a string>")
                };
                self.log.borrow_mut().push(Sent::AutoCommit(text));
            }
            Ok(())
        }

        fn get_option_string(&self, _key: Self::Option) -> AdbcResult<String> {
            Err(not_asked("read an option"))
        }

        fn get_option_bytes(&self, _key: Self::Option) -> AdbcResult<Vec<u8>> {
            Err(not_asked("read an option"))
        }

        fn get_option_int(&self, _key: Self::Option) -> AdbcResult<i64> {
            Err(not_asked("read an option"))
        }

        fn get_option_double(&self, _key: Self::Option) -> AdbcResult<f64> {
            Err(not_asked("read an option"))
        }
    }

    impl Connection for FakeConnection {
        type StatementType = FakeStatement;

        fn new_statement(&mut self) -> AdbcResult<Self::StatementType> {
            Ok(FakeStatement {
                log: Rc::clone(&self.log),
                reply: self.reply.clone(),
                sql: String::new(),
            })
        }

        fn cancel(&mut self) -> AdbcResult<()> {
            Err(not_asked("cancel"))
        }

        fn get_info(&self, _codes: Option<HashSet<InfoCode>>) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
            Err(not_asked("describe itself"))
        }

        fn get_objects(
            &self,
            _depth: ObjectDepth,
            _catalog: Option<&str>,
            _db_schema: Option<&str>,
            _table_name: Option<&str>,
            _table_type: Option<Vec<&str>>,
            _column_name: Option<&str>,
        ) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
            Err(not_asked("list objects"))
        }

        fn get_table_schema(
            &self,
            _catalog: Option<&str>,
            _db_schema: Option<&str>,
            _table_name: &str,
        ) -> AdbcResult<arrow_schema::Schema> {
            Err(not_asked("describe a table"))
        }

        fn get_table_types(&self) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
            Err(not_asked("list table types"))
        }

        fn get_statistic_names(&self) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
            Err(not_asked("list statistics"))
        }

        fn get_statistics(
            &self,
            _catalog: Option<&str>,
            _db_schema: Option<&str>,
            _table_name: Option<&str>,
            _approximate: bool,
        ) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
            Err(not_asked("read statistics"))
        }

        fn commit(&mut self) -> AdbcResult<()> {
            Err(not_asked("commit - nothing on this path commits"))
        }

        fn rollback(&mut self) -> AdbcResult<()> {
            self.log.borrow_mut().push(Sent::Rollback);
            Ok(())
        }

        fn read_partition(&self, _partition: impl AsRef<[u8]>) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
            Err(not_asked("read a partition"))
        }
    }

    impl Optionable for FakeStatement {
        type Option = OptionStatement;

        fn set_option(&mut self, _key: Self::Option, _value: OptionValue) -> AdbcResult<()> {
            Err(not_asked("set a statement option"))
        }

        fn get_option_string(&self, _key: Self::Option) -> AdbcResult<String> {
            Err(not_asked("read an option"))
        }

        fn get_option_bytes(&self, _key: Self::Option) -> AdbcResult<Vec<u8>> {
            Err(not_asked("read an option"))
        }

        fn get_option_int(&self, _key: Self::Option) -> AdbcResult<i64> {
            Err(not_asked("read an option"))
        }

        fn get_option_double(&self, _key: Self::Option) -> AdbcResult<f64> {
            Err(not_asked("read an option"))
        }
    }

    impl Statement for FakeStatement {
        fn bind(&mut self, batch: RecordBatch) -> AdbcResult<()> {
            self.log.borrow_mut().push(Sent::Bound {
                rows: batch.num_rows(),
                columns: batch.num_columns(),
            });
            Ok(())
        }

        fn bind_stream(&mut self, _reader: Box<dyn RecordBatchReader + Send>) -> AdbcResult<()> {
            Err(not_asked("bind a stream"))
        }

        fn execute(&mut self) -> AdbcResult<Box<dyn RecordBatchReader + Send + 'static>> {
            self.log.borrow_mut().push(Sent::Query(self.sql.clone()));
            let batch = self.reply.clone().ok_or_else(|| {
                CoreError::with_message_and_status("the server refused the statement", Status::InvalidArguments)
            })?;
            let schema = batch.schema();
            Ok(Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema)))
        }

        fn execute_update(&mut self) -> AdbcResult<Option<i64>> {
            self.log.borrow_mut().push(Sent::Update(self.sql.clone()));
            Ok(None)
        }

        fn execute_schema(&mut self) -> AdbcResult<arrow_schema::Schema> {
            Err(not_asked("describe a result"))
        }

        fn execute_partitions(&mut self) -> AdbcResult<PartitionedResult> {
            Err(not_asked("partition a result"))
        }

        fn get_parameter_schema(&self) -> AdbcResult<arrow_schema::Schema> {
            Err(not_asked("describe parameters"))
        }

        fn prepare(&mut self) -> AdbcResult<()> {
            Err(not_asked("prepare"))
        }

        fn set_sql_query(&mut self, query: impl AsRef<str>) -> AdbcResult<()> {
            query.as_ref().clone_into(&mut self.sql);
            Ok(())
        }

        fn set_substrait_plan(&mut self, _plan: impl AsRef<[u8]>) -> AdbcResult<()> {
            Err(not_asked("take a substrait plan"))
        }

        fn cancel(&mut self) -> AdbcResult<()> {
            Err(not_asked("cancel"))
        }
    }

    fn one_column(name: &str, column: ArrayRef) -> RecordBatch {
        let schema = Schema::new(vec![Field::new(name, column.data_type().clone(), true)]);
        RecordBatch::try_new(Arc::new(schema), vec![column]).expect("one column is a batch")
    }

    fn open_for(budget: Duration) -> Deadline {
        Deadline::opened_at(Instant::now(), Budget::parse(budget).expect("a positive budget parses"))
    }

    /// `answer` over a fake that replies with `reply`, and what reached it.
    fn run(reply: Option<RecordBatch>, bound: Option<RecordBatch>, deadline: Deadline) -> Ran {
        let log: Log = Rc::default();
        let mut connection = FakeConnection {
            log: Rc::clone(&log),
            reply,
        };
        let outcome = answer(&mut connection, "SELECT 1", bound, CEILING_MS, deadline);
        let sent = log.borrow().clone();
        (outcome, sent)
    }

    fn a_number() -> RecordBatch {
        one_column("n", Arc::new(Int8Array::from(vec![1_i8])))
    }

    #[test]
    fn the_deadline_is_set_local_inside_the_drivers_own_transaction_and_always_rolled_back() {
        // The whole mechanism in one sequence. Autocommit OFF is what opens the transaction -
        // without it `SET LOCAL` is a warning and a no-op, and the deadline bounds nothing. The
        // setting precedes the statement it bounds, and the rollback follows: nothing commits.
        // A minute's budget against a 15 s ceiling, so the value is the ceiling exactly.
        let (outcome, sent) = run(Some(a_number()), None, open_for(Duration::from_secs(60)));
        assert!(outcome.is_ok(), "{outcome:?}");
        assert_eq!(
            sent,
            vec![
                Sent::AutoCommit(String::from("false")),
                Sent::Update(String::from("SET LOCAL statement_timeout = 15000")),
                Sent::Query(String::from("SELECT 1")),
                Sent::Rollback,
            ]
        );
    }

    #[test]
    fn a_budget_under_the_ceiling_is_what_the_server_is_told() {
        // The other side of the clamp: the request's own budget, not the ceiling, when it is the
        // smaller. Between 4 s and 5 s, because the budget is read on this cell's own clock.
        let (_, sent) = run(Some(a_number()), None, open_for(Duration::from_secs(5)));
        let told = sent
            .iter()
            .find_map(|step| match *step {
                Sent::Update(ref sql) => sql.strip_prefix("SET LOCAL statement_timeout = ").map(str::to_owned),
                _ => None,
            })
            .expect("the timeout was sent");
        let millis: u32 = told.parse().expect("the timeout is a millisecond count");
        assert!((4_000..=5_000).contains(&millis), "{millis}");
    }

    #[test]
    fn a_spent_deadline_sends_nothing_and_reads_as_the_deadline() {
        let opened = Instant::now()
            .checked_sub(Duration::from_secs(60))
            .expect("this host's clock is more than a minute past its epoch");
        let gone = Deadline::opened_at(
            opened,
            Budget::parse(Duration::from_secs(1)).expect("a positive budget parses"),
        );
        let (outcome, sent) = run(Some(a_number()), None, gone);
        let refused = outcome.expect_err("a spent deadline is refused");
        assert!(matches!(refused, AdbcError::DeadlineSpent), "{refused:?}");
        assert!(AdbcPostgres::deadline_exceeded(&refused));
        assert_eq!(
            sent,
            Vec::<Sent>::new(),
            "nothing may reach the driver once the budget is gone"
        );
    }

    #[test]
    fn a_statement_the_server_refused_is_still_rolled_back_and_is_not_the_deadline() {
        let (outcome, sent) = run(None, None, open_for(Duration::from_secs(60)));
        let refused = outcome.expect_err("the fake refuses the statement");
        assert!(matches!(refused, AdbcError::Adbc(_)), "{refused:?}");
        assert!(
            !AdbcPostgres::deadline_exceeded(&refused),
            "a refusal with no SQLSTATE is not the deadline"
        );
        assert_eq!(sent.last(), Some(&Sent::Rollback), "{sent:?}");
    }

    #[test]
    fn the_plans_values_are_bound_as_one_row_on_the_statement_that_runs_them() {
        // After the setting ran and before the query executes, so they ride the query's own
        // statement - a batch bound to the setting's statement would appear before its update.
        let day = Date::parse("2026-06-01").expect("a test date is a date");
        let bound = sutura_adbc::parameter_batch(&[ParamValue::Text(String::from("north")), ParamValue::Date(day)])
            .expect("the values are bindable");
        let (outcome, sent) = run(Some(a_number()), bound, open_for(Duration::from_secs(60)));
        assert!(outcome.is_ok(), "{outcome:?}");
        assert_eq!(
            sent,
            vec![
                Sent::AutoCommit(String::from("false")),
                Sent::Update(String::from("SET LOCAL statement_timeout = 15000")),
                Sent::Bound { rows: 1, columns: 2 },
                Sent::Query(String::from("SELECT 1")),
                Sent::Rollback,
            ]
        );
    }

    fn with_sqlstate(code: [u8; 5]) -> AdbcError {
        let mut cause = CoreError::with_message_and_status("the statement failed", Status::Unknown);
        cause.sqlstate = code.map(|byte| c_char::try_from(byte).expect("a SQLSTATE is ASCII"));
        AdbcError::Adbc(cause)
    }

    #[test]
    fn a_statement_cancelled_by_its_timeout_reads_as_the_deadline_and_a_permission_refusal_does_not() {
        assert!(AdbcPostgres::deadline_exceeded(&with_sqlstate(*b"57014")));
        assert!(!AdbcPostgres::deadline_exceeded(&with_sqlstate(*b"42501")));
    }

    #[test]
    fn a_scale_zero_numeric_is_text_here_where_the_tokio_postgres_path_reads_an_integer() {
        // THE DRIFT, pinned on both sides so neither moves silently. The driver hands `NUMERIC` back
        // as `Utf8` whatever the scale (its source, not a run), and this transport carries it as
        // the text it is; `lib.rs` decodes the same `1::numeric` off the wire as an integer. Closing
        // the gap is the cutover's decision, and this cell is what it will have to change.
        let (outcome, _) = run(
            Some(one_column("total", Arc::new(StringArray::from(vec!["1"])))),
            None,
            open_for(Duration::from_secs(60)),
        );
        let rows = outcome
            .expect("a text column is answered")
            .to_rows()
            .expect("a text cell is read");
        assert_eq!(rows.rows(), [vec![Value::Text(String::from("1"))]]);
        // `1::numeric` on the wire: one base-10000 digit, weight 0, positive, scale 0.
        let wire = crate::numeric::decode_numeric(&[0, 1, 0, 0, 0, 0, 0, 0, 0, 1]).expect("a valid NUMERIC decodes");
        assert_eq!(
            crate::numeric::numeric_cell(&wire, "total").expect("a finite NUMERIC is a cell"),
            Value::Integer(1)
        );
    }

    #[test]
    fn a_stream_past_the_row_ceiling_is_refused_while_it_is_read() {
        // A literal rather than the constant, so raising the ceiling reddens this cell instead of
        // moving it.
        let (outcome, sent) = run(
            Some(one_column("n", Arc::new(Int8Array::from(vec![0_i8; 1_000_001])))),
            None,
            open_for(Duration::from_secs(60)),
        );
        let refused = outcome.expect_err("a million and one rows are past the ceiling");
        assert!(
            matches!(refused, AdbcError::Unannounced(UnannouncedBatch::OverBound { .. })),
            "{refused:?}"
        );
        assert_eq!(sent.last(), Some(&Sent::Rollback), "{sent:?}");
    }

    #[test]
    fn a_relative_driver_path_is_refused_before_anything_loads_it() {
        let refused = MountedDriver::parse("lib/libadbc_driver_postgresql.so").expect_err("a relative path is refused");
        assert!(matches!(refused, super::UnusableDriverPath::Relative { .. }), "{refused:?}");
    }

    #[test]
    fn a_path_naming_no_driver_fails_at_the_load_and_the_uri_is_never_printed() {
        // THE NEGATIVE CONTROL: the real `execute`, which every guard above lets through, fails on
        // the one thing a fake cannot stand in for. And the connection string, which carries the
        // password in a deployment (a plain marker here), is in neither the `Debug` nor the failure.
        let driver = MountedDriver::parse("/nonexistent/libadbc_driver_postgresql.so").expect("an absolute path parses");
        let transport = AdbcPostgres::new(driver, Secret::new("a-connection-string-marker")).expect("the default ceiling parses");
        assert!(
            !format!("{transport:?}").contains("connection-string-marker"),
            "{transport:?}"
        );
        let source = SourceName::parse("pg").expect("a test source is a source");
        let failed = transport
            .execute(
                &GeneratedQuery::literal(source, String::from("SELECT 1")),
                open_for(Duration::from_secs(60)),
            )
            .expect_err("no driver lives at this path");
        assert!(matches!(failed, AdbcError::Load(_)), "{failed:?}");
        assert!(!format!("{failed:?}").contains("connection-string-marker"), "{failed:?}");
    }
}
