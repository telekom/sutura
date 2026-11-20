//! Client-side time bounds: the parsed option values and the deadline a statement computes once.

use crate::options;
use adbc_core::error::{Error, Status};
use std::future::Future;
use std::time::Duration;
use tokio::time::Instant;

/// The longest bound an option accepts: a year keeps `Instant + Duration` far from overflow.
const MAX: Duration = Duration::from_secs(366 * 24 * 60 * 60);

/// The bounds set by [`options::QUERY_TIMEOUT_SECONDS`] and [`options::UPDATE_TIMEOUT_SECONDS`].
/// Neither is set by default, and an unset bound leaves the call unbounded.
///
/// [`options::QUERY_TIMEOUT_SECONDS`]: crate::options::QUERY_TIMEOUT_SECONDS
/// [`options::UPDATE_TIMEOUT_SECONDS`]: crate::options::UPDATE_TIMEOUT_SECONDS
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Timeouts {
    pub(crate) query: Option<Duration>,
    pub(crate) update: Option<Duration>,
}

impl Timeouts {
    /// Set the bound named by the option `key` from its string `value`.
    pub(crate) fn set(&mut self, key: &str, value: &str) -> Result<(), Error> {
        let timeout = parse(key, value)?;

        match key {
            options::QUERY_TIMEOUT_SECONDS => self.query = Some(timeout),
            options::UPDATE_TIMEOUT_SECONDS => self.update = Some(timeout),
            other => return Err(unknown(other)),
        }

        Ok(())
    }

    /// Read back the bound named by the option `key`.
    pub(crate) fn get(&self, key: &str) -> Result<String, Error> {
        let (which, timeout) = match key {
            options::QUERY_TIMEOUT_SECONDS => ("query", self.query),
            options::UPDATE_TIMEOUT_SECONDS => ("update", self.update),
            other => return Err(unknown(other)),
        };

        timeout.map(render).ok_or_else(|| {
            Error::with_message_and_status(
                format!("no {which} timeout configured"),
                Status::NotFound,
            )
        })
    }
}

fn unknown(key: &str) -> Error {
    Error::with_message_and_status(
        format!("{key:?} is not a timeout option"),
        Status::InvalidArguments,
    )
}

/// Parse an option value as a positive number of seconds, fractions allowed.
pub(crate) fn parse(key: &str, value: &str) -> Result<Duration, Error> {
    let invalid = |why: &str| {
        Error::with_message_and_status(
            format!("error setting option {key:?} = {value:?}: {why}"),
            Status::InvalidArguments,
        )
    };

    let duration = value
        .parse::<f64>()
        .ok()
        .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
        .ok_or_else(|| invalid("expected a positive, finite number of seconds"))?;

    if duration.is_zero() {
        return Err(invalid("must be greater than zero"));
    }

    if duration > MAX {
        return Err(invalid("must be at most 366 days"));
    }

    Ok(duration)
}

/// The inverse of [`parse()`], for reading an option back.
pub(crate) fn render(timeout: Duration) -> String {
    timeout.as_secs_f64().to_string()
}

/// The moment a call must be finished by, computed once when the call starts.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Deadline(Option<Instant>);

/// The [`Deadline`] passed before the call finished.
#[derive(Debug)]
pub(crate) struct Elapsed;

impl Deadline {
    pub(crate) fn after(bound: Option<Duration>) -> Self {
        Self(bound.map(|bound| Instant::now() + bound))
    }

    /// Drive `work` until it finishes or the deadline passes.
    ///
    /// A deadline that has already passed refuses the work even if it could finish at once, so a
    /// stream that keeps delivering cannot outlive it. Dropping `work` on expiry closes whatever
    /// connection it holds, which is not proof that the server stopped the query.
    pub(crate) async fn bound<F: Future>(self, work: F) -> Result<F::Output, Elapsed> {
        let Some(at) = self.0 else {
            return Ok(work.await);
        };

        if Instant::now() >= at {
            return Err(Elapsed);
        }

        tokio::time::timeout_at(at, work).await.map_err(|_| Elapsed)
    }
}

impl From<Elapsed> for Error {
    fn from(Elapsed: Elapsed) -> Self {
        Error::with_message_and_status("query execution timed out", Status::Timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::{pending, ready};

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn parses_whole_and_fractional_seconds() {
        assert_eq!(parse("key", "1").unwrap(), Duration::from_secs(1));
        assert_eq!(parse("key", "3600").unwrap(), Duration::from_secs(3600));
        assert_eq!(parse("key", "0.25").unwrap(), Duration::from_millis(250));
        assert_eq!(parse("key", "1.5").unwrap(), Duration::from_millis(1500));
        assert_eq!(parse("key", "0.001").unwrap(), Duration::from_millis(1));
    }

    #[test]
    fn refuses_what_is_not_a_positive_duration() {
        for bad in [
            "0", "0.0", "-5", "-0.5", "abc", "", " ", "NaN", "inf", "-inf", "1e-12", "1e30",
            "31622401",
        ] {
            let err = parse("key", bad).unwrap_err();
            assert_eq!(err.status, Status::InvalidArguments, "value {bad:?}");
        }
    }

    #[test]
    fn renders_what_it_parses() {
        for text in ["30", "0.25", "1.5"] {
            assert_eq!(render(parse("key", text).unwrap()), text);
        }
    }

    #[test]
    fn an_unset_bound_never_expires() {
        let finished = runtime().block_on(Deadline::after(None).bound(ready(7)));
        assert_eq!(finished.unwrap(), 7);
    }

    #[test]
    fn a_stalled_call_is_dropped_at_the_deadline() {
        let deadline = Deadline::after(Some(Duration::from_millis(50)));
        let started = std::time::Instant::now();

        let outcome = runtime().block_on(deadline.bound(pending::<()>()));

        assert!(outcome.is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_passed_deadline_refuses_work_that_could_finish() {
        let deadline = Deadline::after(Some(Duration::from_millis(1)));
        std::thread::sleep(Duration::from_millis(20));

        assert!(runtime().block_on(deadline.bound(ready(()))).is_err());
    }

    #[test]
    fn elapsed_is_a_timeout_status() {
        assert_eq!(Error::from(Elapsed).status, Status::Timeout);
    }
}

/// The options as a caller sets them, and the bounds they put on a statement.
#[cfg(test)]
mod through_the_driver {
    use crate::fake_server::FakeServer;
    use crate::{ClickhouseDriver, options};
    use adbc_core::error::Status;
    use adbc_core::options::OptionDatabase;
    use adbc_core::{Connection, Database, Driver, Optionable, Statement};
    use arrow_array::{Int64Array, RecordBatch};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    fn connection_to(server: &FakeServer) -> crate::ClickhouseConnection {
        let mut driver = ClickhouseDriver::init();
        let db = driver
            .new_database_with_opts([(OptionDatabase::Uri, server.url.clone().into())])
            .unwrap();
        db.new_connection().unwrap()
    }

    #[test]
    fn options_read_back_inherit_and_override() {
        let server = FakeServer::silent();
        let mut conn = connection_to(&server);

        assert_eq!(
            conn.get_option_string(options::QUERY_TIMEOUT_SECONDS.into())
                .unwrap_err()
                .status,
            Status::NotFound
        );

        conn.set_option(options::QUERY_TIMEOUT_SECONDS.into(), "0.25".into())
            .unwrap();
        conn.set_option(options::UPDATE_TIMEOUT_SECONDS.into(), "1.5".into())
            .unwrap();

        let mut statement = conn.new_statement().unwrap();
        let read = |statement: &crate::ClickhouseStatement, key: &str| {
            statement.get_option_string(key.into()).unwrap()
        };
        assert_eq!(read(&statement, options::QUERY_TIMEOUT_SECONDS), "0.25");
        assert_eq!(read(&statement, options::UPDATE_TIMEOUT_SECONDS), "1.5");

        statement
            .set_option(options::QUERY_TIMEOUT_SECONDS.into(), "2".into())
            .unwrap();
        assert_eq!(read(&statement, options::QUERY_TIMEOUT_SECONDS), "2");
        assert_eq!(read(&statement, options::UPDATE_TIMEOUT_SECONDS), "1.5");
    }

    #[test]
    fn an_invalid_value_and_the_old_name_are_refused() {
        let server = FakeServer::silent();
        let mut conn = connection_to(&server);
        let mut statement = conn.new_statement().unwrap();

        for key in [
            options::QUERY_TIMEOUT_SECONDS,
            options::UPDATE_TIMEOUT_SECONDS,
        ] {
            for bad in ["0", "-1", "abc"] {
                let error = conn.set_option(key.into(), bad.into()).unwrap_err();
                assert_eq!(error.status, Status::InvalidArguments, "{key} = {bad}");
                let error = statement.set_option(key.into(), bad.into()).unwrap_err();
                assert_eq!(error.status, Status::InvalidArguments, "{key} = {bad}");
            }
        }

        let old = "clickhouse.client.query_timeout";
        assert_eq!(
            conn.set_option(old.into(), "5".into()).unwrap_err().status,
            Status::InvalidArguments
        );
        assert_eq!(
            statement
                .set_option(old.into(), "5".into())
                .unwrap_err()
                .status,
            Status::InvalidArguments
        );
    }

    #[test]
    fn a_query_that_is_never_answered_times_out_at_the_query_bound() {
        let server = FakeServer::silent();
        let mut conn = connection_to(&server);
        conn.set_option(options::QUERY_TIMEOUT_SECONDS.into(), "0.3".into())
            .unwrap();
        let mut statement = conn.new_statement().unwrap();
        statement.set_sql_query("SELECT 1").unwrap();

        let started = Instant::now();
        let error = statement
            .execute()
            .err()
            .expect("the query was never answered");

        assert_eq!(error.status, Status::Timeout);
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn an_update_that_is_never_answered_times_out_at_the_update_bound() {
        let server = FakeServer::silent();
        let mut conn = connection_to(&server);
        conn.set_option(options::UPDATE_TIMEOUT_SECONDS.into(), "0.3".into())
            .unwrap();
        let mut statement = conn.new_statement().unwrap();
        statement
            .set_sql_query("CREATE TABLE t (n Int64) ENGINE = Memory")
            .unwrap();

        let started = Instant::now();
        let error = statement.execute_update().unwrap_err();

        assert_eq!(error.status, Status::Timeout);
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn a_streaming_ingest_that_is_never_answered_times_out_at_the_update_bound() {
        let server = FakeServer::silent();
        let mut conn = connection_to(&server);
        conn.set_option(options::UPDATE_TIMEOUT_SECONDS.into(), "0.3".into())
            .unwrap();
        let mut statement = conn.new_statement().unwrap();
        statement
            .set_sql_query("INSERT INTO t FORMAT ArrowStream")
            .unwrap();
        let column = Int64Array::from(vec![1]);
        statement
            .bind(RecordBatch::try_from_iter([("n", Arc::new(column) as _)]).unwrap())
            .unwrap();

        let started = Instant::now();
        let error = statement.execute_update().unwrap_err();

        assert_eq!(error.status, Status::Timeout);
        assert!(started.elapsed() < Duration::from_secs(4));
    }
}
