//! The Oracle reader's answer to each failure, and what it logs.

use sutura_domain::model::SourceName;
use sutura_exec_datafusion::Unread;
use sutura_exec_oracle::OracleError;
use sutura_runtime::testing::Capture;

use super::unread;

fn reader_source() -> SourceName {
    SourceName::parse("oracle_reader_q7").expect("a source name")
}

/// What `unread` logs for `cause`, under the shipped default directive.
fn reader_log(cause: OracleError) -> String {
    let sink = Capture::new();
    let telemetry = sutura_config::TelemetrySettings::new(
        sutura_config::ServiceName::parse("sutura-test").expect("a test service name is a name"),
        sutura_config::LogFilter::parse("info").expect("a test directive is a directive"),
        sutura_config::LogFormat::Bunyan,
        true,
    );
    let subscriber =
        sutura_runtime::telemetry::subscriber(&telemetry, sink.clone()).expect("a valid directive builds a subscriber");
    drop(tracing::subscriber::with_default(subscriber, || {
        unread(&reader_source(), cause)
    }));
    sink.contents()
}

/// Only a partition past the reader's result budget is typed as one that did not fit; a stale
/// snapshot is the engine's failure.
#[test]
fn only_an_oracle_partition_past_the_readers_budget_is_read_as_one_that_did_not_fit() {
    let read = unread(&reader_source(), OracleError::OverBudget { most_bytes: 1 });
    assert!(matches!(read, Unread::DidNotFit), "{read:?}");
    let stale = unread(&reader_source(), OracleError::SnapshotStale { most_seconds: 3 });
    assert!(matches!(stale, Unread::Failed(_)), "{stale:?}");
}

/// A failed Oracle partition is logged naming the source, never the cause's text.
#[test]
fn a_readers_failed_oracle_partition_is_logged_naming_the_source_and_never_the_cause() {
    let marker = "planted_oracle_marker_5c1e";
    let failed = reader_log(OracleError::UnsupportedType {
        column: String::from(marker),
        oracle_type: "RAW",
    });
    assert!(failed.contains("oracle_reader_q7"), "{failed}");
    assert!(!failed.contains(marker), "{failed}");
    let unfit = reader_log(OracleError::OverBudget { most_bytes: 1 });
    assert!(unfit.contains("oracle_reader_q7"), "{unfit}");
}
