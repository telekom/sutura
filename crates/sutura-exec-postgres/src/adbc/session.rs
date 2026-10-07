//! What one connection is asked, for each port method - generic over the connection, so a fake
//! stands in for the driver and every step here is held by a cell over one.
//!
//! Every method opens the same shape: autocommit off (the driver then issues `BEGIN` before the
//! first statement), one fixed setting through `execute_update`, the step, and a rollback whatever
//! happened. Nothing on this transport commits.
//!
//! - **`Warehouse::execute`** - [`answer`]: `SET LOCAL statement_timeout` from the deadline, then
//!   the statement through `execute`.
//! - **`Warehouse::dry_run`** - [`described`]: the same setting, then `execute_schema`, which the
//!   pinned driver implements as `PQprepare` and `PQdescribePrepared` and nothing else
//!   (`statement.cc:650`, `apache-arrow-adbc-24`): the server parses and plans, and no row is run
//!   or read.
//! - **`Warehouse::execute_raw`** - [`raw`]: `SET TRANSACTION READ ONLY` beside the deadline, the
//!   caller's one statement through `execute` (the server refuses a second at `Parse`), and the
//!   stream read no further than `MAX_ROWS` plus one - `docs/adr/0013`'s three properties.
//! - **The boot path** - [`boot`]: no request, so no deadline; the deployment's ceiling alone.
//!
//! **The limit**: `SET TRANSACTION READ ONLY` is sent through the simple protocol after the
//! driver's own `BEGIN`, read off the driver's source rather than observed; no cell here reaches a
//! server.

use core::num::{NonZeroU32, NonZeroUsize};
use std::time::{Duration, Instant};

use adbc_core::options::{OptionConnection, OptionValue};
use adbc_core::{Connection, Statement as _};
use arrow_array::{RecordBatch, RecordBatchReader};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{Accumulating, ResultBatches, ResultBudget};

use super::AdbcError;
use crate::deadline::deadline_statement_timeout_ms;

/// The rows one result stream may deliver before it is refused while being read - the `BigQuery`
/// transport's ceiling, for its reason: a federation leg carries no `LIMIT`, so this is what makes
/// a stream finite. `tests::a_stream_past_the_row_ceiling_is_refused_while_it_is_read` holds it.
const MOST_RESULT_ROWS: usize = 1_000_000;

/// The bytes one result stream may materialise, the `BigQuery` transport's budget.
pub(super) const MOST_RESULT_BYTES: NonZeroUsize = match NonZeroUsize::new(256 * 1024 * 1024) {
    Some(bytes) => bytes,
    None => NonZeroUsize::MIN,
};

/// The SQLSTATE a statement cancelled by `statement_timeout` fails with, `query_canceled`.
const QUERY_CANCELED: [u8; 5] = *b"57014";

/// What the server's cancel of a statement past `statement_timeout` says, as the driver relays it.
const STATEMENT_TIMEOUT_TEXT: &str = "canceling statement due to statement timeout";

/// The SQLSTATE `zero_denominator: fails` arrives as on this source, `division_by_zero`.
const DIVISION_BY_ZERO: [u8; 5] = *b"22012";

/// The two the raw path reads as the source refusing: `read_only_sql_transaction`,
/// `insufficient_privilege`.
const SOURCE_REFUSED: [[u8; 5]; 2] = [*b"25006", *b"42501"];

/// The rows a raw statement is read to: one past the cap, so the caller's own check still sees a
/// result over it rather than a truncated one that looks complete.
pub(super) const RAW_ROWS: usize = sutura_domain::plan::MAX_ROWS as usize + 1;

/// What the one `execute_update` in this module sends: fixed literals and a [`NonZeroU32`], so
/// nothing a caller wrote reaches the simple protocol through it. Zero would read as *no timeout*.
enum Setting {
    /// The certified path's: a timeout, or none where the boot path's ceiling is disabled.
    Certified(Option<NonZeroU32>),
    /// The raw tool's: `READ ONLY` beside the timeout.
    ReadOnly(NonZeroU32),
}

impl Setting {
    fn sql(&self) -> Option<String> {
        match *self {
            Self::Certified(None) => None,
            Self::Certified(Some(ms)) => Some(format!("SET LOCAL statement_timeout = {ms}")),
            Self::ReadOnly(ms) => Some(format!("SET TRANSACTION READ ONLY; SET LOCAL statement_timeout = {ms}")),
        }
    }

    fn apply<C>(&self, connection: &mut C) -> Result<(), AdbcError>
    where
        C: Connection,
    {
        let Some(sql) = self.sql() else {
            return Ok(());
        };
        let mut statement = connection.new_statement().map_err(AdbcError::Adbc)?;
        statement.set_sql_query(sql).map_err(AdbcError::Adbc)?;
        #[expect(
            clippy::disallowed_methods,
            reason = "the one execute_update: its text is `Setting::sql`, fixed literals and an integer"
        )]
        let updated = statement.execute_update();
        updated.map(drop).map_err(AdbcError::Adbc)
    }
}

/// `Warehouse::execute`'s statement under `deadline`.
pub(super) fn answer<C>(
    connection: &mut C,
    sql: &str,
    bound: Option<RecordBatch>,
    ceiling_ms: u32,
    deadline: Deadline,
) -> Result<ResultBatches, AdbcError>
where
    C: Connection,
{
    let timeout = left(ceiling_ms, deadline)?;
    transaction(connection, &Setting::Certified(Some(timeout)), |connection| {
        stream(connection, sql, bound, None, Some(timeout))
    })
}

/// `Warehouse::dry_run`'s statement, parsed and described by the server and never run.
pub(super) fn described<C>(connection: &mut C, sql: &str, ceiling_ms: u32, deadline: Deadline) -> Result<(), AdbcError>
where
    C: Connection,
{
    let timeout = left(ceiling_ms, deadline)?;
    transaction(connection, &Setting::Certified(Some(timeout)), |connection| {
        let mut statement = connection.new_statement().map_err(AdbcError::Adbc)?;
        statement.set_sql_query(sql).map_err(AdbcError::Adbc)?;
        statement.execute_schema().map(drop).map_err(AdbcError::Adbc)
    })
}

/// The raw tool's one statement, `READ ONLY`, read to [`RAW_ROWS`] at most.
pub(super) fn raw<C>(connection: &mut C, sql: &str, ceiling_ms: u32, deadline: Deadline) -> Result<ResultBatches, AdbcError>
where
    C: Connection,
{
    let timeout = left(ceiling_ms, deadline)?;
    transaction(connection, &Setting::ReadOnly(timeout), |connection| {
        stream(connection, sql, None, Some(RAW_ROWS), Some(timeout))
    })
}

/// The boot path's statement, under the connect-time ceiling alone.
pub(super) fn boot<C>(
    connection: &mut C,
    sql: &str,
    bound: Option<RecordBatch>,
    ceiling_ms: u32,
) -> Result<ResultBatches, AdbcError>
where
    C: Connection,
{
    let timeout = NonZeroU32::new(ceiling_ms);
    transaction(connection, &Setting::Certified(timeout), |connection| {
        stream(connection, sql, bound, None, timeout)
    })
}

/// Whether `error` is the deadline: spent before sending, or the server's `57014`.
///
/// `57014` is also what a manual `pg_cancel_backend` produces, which this cannot tell apart -
/// `docs/adr/0029`'s limit.
pub(super) fn deadline_exceeded(error: &AdbcError) -> bool {
    match *error {
        AdbcError::DeadlineSpent | AdbcError::TimedOut(_) => true,
        AdbcError::Adbc(ref cause) => sqlstate(cause) == QUERY_CANCELED,
        AdbcError::Load(_)
        | AdbcError::Batch(_)
        | AdbcError::Unannounced(_)
        | AdbcError::Parameters(_)
        | AdbcError::Unreadable(_) => false,
    }
}

/// Whether `error` is the server's `22012`.
pub(super) fn division_by_zero(error: &AdbcError) -> bool {
    matches!(*error, AdbcError::Adbc(ref cause) if sqlstate(cause) == DIVISION_BY_ZERO)
}

/// A fixture's statements, committed - autocommit stays on, so this is the one path here that
/// writes. Behind the `fixtures` feature, which no release builds.
#[cfg(feature = "fixtures")]
pub(super) fn load<C>(connection: &mut C, sql: &str) -> Result<(), AdbcError>
where
    C: Connection,
{
    let mut statement = connection.new_statement().map_err(AdbcError::Adbc)?;
    statement.set_sql_query(sql).map_err(AdbcError::Adbc)?;
    #[expect(
        clippy::disallowed_methods,
        reason = "a fixture load: `importer`'s DDL and literals rendered from a parsed CSV, never caller text"
    )]
    let updated = statement.execute_update();
    updated.map(drop).map_err(AdbcError::Adbc)
}

/// Whether `error` is the source saying no: `25006` or `42501`.
pub(super) fn source_refused(error: &AdbcError) -> bool {
    matches!(*error, AdbcError::Adbc(ref cause) if SOURCE_REFUSED.contains(&sqlstate(cause)))
}

fn sqlstate(cause: &adbc_core::error::Error) -> [u8; 5] {
    cause.sqlstate.map(|c| u8::try_from(c).unwrap_or(0))
}

/// What is left of `deadline` as a timeout, or the local refusal once nothing is.
fn left(ceiling_ms: u32, deadline: Deadline) -> Result<NonZeroU32, AdbcError> {
    if deadline.remaining_at(Instant::now()).is_none() {
        return Err(AdbcError::DeadlineSpent);
    }
    Ok(deadline_statement_timeout_ms(ceiling_ms, deadline))
}

/// `step` inside a transaction the driver opens and this always rolls back.
///
/// `SET LOCAL` outside a transaction is a warning and a no-op, so autocommit off is what makes the
/// timeout real: the driver issues `BEGIN` itself before the first statement.
fn transaction<C, T>(
    connection: &mut C,
    setting: &Setting,
    step: impl FnOnce(&mut C) -> Result<T, AdbcError>,
) -> Result<T, AdbcError>
where
    C: Connection,
{
    connection
        .set_option(OptionConnection::AutoCommit, OptionValue::from(false))
        .map_err(AdbcError::Adbc)?;
    let outcome = setting.apply(connection).and_then(|()| step(connection));
    // Nothing on this path commits. A failed rollback leaves a transaction the connection's drop
    // discards anyway, so its error is not the caller's.
    drop(connection.rollback());
    outcome
}

fn stream<C>(
    connection: &mut C,
    sql: &str,
    bound: Option<RecordBatch>,
    stop: Option<usize>,
    timeout: Option<NonZeroU32>,
) -> Result<ResultBatches, AdbcError>
where
    C: Connection,
{
    let mut statement = connection.new_statement().map_err(AdbcError::Adbc)?;
    statement.set_sql_query(sql).map_err(AdbcError::Adbc)?;
    if let Some(batch) = bound {
        statement.bind(batch).map_err(AdbcError::Adbc)?;
    }
    let sent = Instant::now();
    drained(statement.execute().map_err(AdbcError::Adbc)?, stop).map_err(|error| timed_out(error, sent, timeout))
}

/// A failure READING the stream is the timeout when the server's own cancel text says so, and
/// only once the timeout the server was told has run out.
///
/// **Why text and a clock, not a SQLSTATE**: the pinned driver reads a parameterless result through
/// `COPY ... TO STDOUT`, so a statement the server cancels mid-result fails inside the Arrow
/// stream, and the driver manager hands that back as a C-interface message with no SQLSTATE
/// (measured: a raw `pg_sleep` past its budget arrived as `Batch(CDataInterface(".. canceling
/// statement due to statement timeout"))`). The text is what separates it from a dropped
/// connection, a terminated backend or a manual `pg_cancel_backend` (`.. due to user request`); the
/// clock keeps a message that merely quotes it from counting early.
///
/// **The limit**: the text is the server's English message, so a server running a non-English
/// `lc_messages` reads its timeout as the batch failure it arrived as - refused all the same, but
/// not as the deadline.
fn timed_out(error: AdbcError, sent: Instant, timeout: Option<NonZeroU32>) -> AdbcError {
    match error {
        AdbcError::Batch(cause)
            if cause.to_string().contains(STATEMENT_TIMEOUT_TEXT)
                && timeout.is_some_and(|ms| sent.elapsed() >= Duration::from_millis(u64::from(ms.get()))) =>
        {
            AdbcError::TimedOut(cause)
        }
        other => other,
    }
}

/// Reads a stream into the port's batches, refusing past either ceiling at the batch that crosses
/// it, and stopping once `stop` rows are in.
fn drained(reader: Box<dyn RecordBatchReader + Send>, stop: Option<usize>) -> Result<ResultBatches, AdbcError> {
    let mut accumulating = Accumulating::announcing(reader.schema(), MOST_RESULT_ROWS, ResultBudget::of_bytes(MOST_RESULT_BYTES));
    for batch in reader {
        accumulating
            .push(batch.map_err(AdbcError::Batch)?)
            .map_err(AdbcError::Unannounced)?;
        if stop.is_some_and(|most| accumulating.delivered() >= most) {
            break;
        }
    }
    Ok(accumulating.finish())
}

#[cfg(test)]
mod tests {
    //! What each port method sends, over `super::super::tests`' fake connection.

    use std::sync::Arc;
    use std::time::Duration;

    use arrow_array::Int8Array;

    use crate::adbc::tests::{CEILING_MS, FakeConnection, Sent, a_number, one_column, open_for, with_sqlstate};

    #[test]
    fn dry_run_describes_the_statement_inside_the_deadline_and_never_runs_it() {
        let mut connection = FakeConnection::answering(None);
        let outcome = super::described(&mut connection, "SELECT 1", CEILING_MS, open_for(Duration::from_secs(60)));
        assert!(outcome.is_ok(), "{outcome:?}");
        assert_eq!(
            connection.sent(),
            vec![
                Sent::AutoCommit(String::from("false")),
                Sent::Update(String::from("SET LOCAL statement_timeout = 15000")),
                Sent::Described {
                    statement: 1,
                    sql: String::from("SELECT 1")
                },
                Sent::Rollback,
            ]
        );
    }

    #[test]
    fn a_raw_statement_runs_read_only_beside_its_deadline() {
        let mut connection = FakeConnection::answering(Some(a_number()));
        let outcome = super::raw(&mut connection, "SELECT 1", CEILING_MS, open_for(Duration::from_secs(60)));
        assert!(outcome.is_ok(), "{outcome:?}");
        assert_eq!(
            connection.sent(),
            vec![
                Sent::AutoCommit(String::from("false")),
                Sent::Update(String::from("SET TRANSACTION READ ONLY; SET LOCAL statement_timeout = 15000")),
                Sent::Query {
                    statement: 1,
                    sql: String::from("SELECT 1")
                },
                Sent::Rollback,
            ]
        );
    }

    #[test]
    fn a_raw_stream_is_read_no_further_than_the_batch_that_crosses_one_row_past_the_cap() {
        // Five batches of 4000 rows: the third takes the count past 10 001, so it is the last one
        // read. Literals, so moving the cap reddens this cell instead of moving it.
        let mut connection = FakeConnection::streaming(Some(one_column("n", Arc::new(Int8Array::from(vec![0_i8; 4_000])))), 5);
        let batches =
            super::raw(&mut connection, "SELECT 1", CEILING_MS, open_for(Duration::from_secs(60))).expect("a raw stream reads");
        assert_eq!(batches.batches().len(), 3);
        let mut certified = FakeConnection::streaming(Some(one_column("n", Arc::new(Int8Array::from(vec![0_i8; 4_000])))), 5);
        let batches = super::answer(
            &mut certified,
            "SELECT 1",
            None,
            CEILING_MS,
            open_for(Duration::from_secs(60)),
        )
        .expect("a certified stream reads");
        assert_eq!(batches.batches().len(), 5, "only the raw path stops early");
    }

    #[test]
    fn the_boot_path_runs_under_the_ceiling_alone_and_sends_no_setting_when_it_is_disabled() {
        let mut connection = FakeConnection::answering(Some(a_number()));
        let outcome = super::boot(&mut connection, "SELECT 1", None, CEILING_MS);
        assert!(outcome.is_ok(), "{outcome:?}");
        assert_eq!(
            connection.sent(),
            vec![
                Sent::AutoCommit(String::from("false")),
                Sent::Update(String::from("SET LOCAL statement_timeout = 15000")),
                Sent::Query {
                    statement: 1,
                    sql: String::from("SELECT 1")
                },
                Sent::Rollback,
            ]
        );
        let mut disabled = FakeConnection::answering(Some(a_number()));
        let outcome = super::boot(&mut disabled, "SELECT 1", None, 0);
        assert!(outcome.is_ok(), "{outcome:?}");
        assert_eq!(
            disabled.sent(),
            vec![
                Sent::AutoCommit(String::from("false")),
                Sent::Query {
                    statement: 0,
                    sql: String::from("SELECT 1")
                },
                Sent::Rollback,
            ]
        );
    }

    #[test]
    fn a_stream_failure_is_the_timeout_exactly_once_the_timeout_has_run_out() {
        use core::num::NonZeroU32;
        use std::time::Instant;

        use super::super::AdbcError;

        let failure = || {
            AdbcError::Batch(arrow_schema::ArrowError::CDataInterface(String::from(
                "ERROR: canceling statement due to statement timeout",
            )))
        };
        let ms = NonZeroU32::new(50);
        let long_ago = Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("a second ago is representable");
        let read = super::timed_out(failure(), long_ago, ms);
        assert!(matches!(read, AdbcError::TimedOut(_)), "{read:?}");
        assert!(super::deadline_exceeded(&read));
        let read = super::timed_out(failure(), Instant::now(), ms);
        assert!(
            matches!(read, AdbcError::Batch(_)),
            "inside the window it is the failure it was: {read:?}"
        );
        assert!(!super::deadline_exceeded(&read));
        let read = super::timed_out(failure(), long_ago, None);
        assert!(
            matches!(read, AdbcError::Batch(_)),
            "no timeout was set, so none ran out: {read:?}"
        );
    }

    #[test]
    fn a_stream_failure_that_is_not_the_servers_cancel_is_not_the_timeout_after_the_window() {
        use core::num::NonZeroU32;
        use std::time::Instant;

        use super::super::AdbcError;

        let long_ago = Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("a second ago is representable");
        for text in [
            "server closed the connection unexpectedly",
            "FATAL: terminating connection due to administrator command",
            "ERROR: canceling statement due to user request",
        ] {
            let failure = AdbcError::Batch(arrow_schema::ArrowError::CDataInterface(String::from(text)));
            let read = super::timed_out(failure, long_ago, NonZeroU32::new(50));
            assert!(matches!(read, AdbcError::Batch(_)), "{text}: {read:?}");
            assert!(!super::deadline_exceeded(&read), "{text}");
        }
    }

    #[test]
    fn a_read_only_or_permission_refusal_is_the_source_refusing_and_a_timeout_is_not() {
        assert!(super::source_refused(&with_sqlstate(*b"25006")));
        assert!(super::source_refused(&with_sqlstate(*b"42501")));
        assert!(!super::source_refused(&with_sqlstate(*b"57014")));
    }
}
