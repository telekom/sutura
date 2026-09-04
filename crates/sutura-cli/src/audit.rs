//! The record the `query` command writes, driven through the command itself.
//!
//! **Its own `#[cfg(test)]` module, and the reason is the causality gate rather than tidiness.**
//! `cargo xtask causality` reconstructs the baseline by reverting every changed file that added no
//! test, so a test written into `commands.rs` beside the fix it proves would be held at HEAD with
//! the fix and could never be red against the base behaviour. Here the two are separate files and
//! the proof is mechanical.
//!
//! Driven through [`crate::commands::query`] - the command's own entry point - rather than the
//! generic helper beneath it, so the signature this file names is one both versions of that file
//! have. What it asserts is the LOG, because that is the only place a record is observable: the
//! sink writes onto the process subscriber, this binary installs none, and installing one is what
//! lets a test read the bytes. That is also the limit the invariants row states - a record is
//! written, and whether anything keeps it is the deployment's answer.

use std::path::{Path, PathBuf};

use sutura_runtime::testing::Capture;

/// The documented example: the catalog, the questions and the CSVs the quickstart uses.
fn example() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player")
}

/// Runs `sutura query` over one of the example's questions with a subscriber over a buffer, and
/// returns the audit records it wrote.
///
/// `info` and not `trace`, deliberately: `info` is the shipped default directive, so a record that
/// appeared only under a wider one would not be a record an operator gets.
///
/// Filtered to the two messages the sink writes, so an unrelated line from the engine or the
/// settings cannot make a count assertion pass or fail for the wrong reason.
fn records(question: &str) -> Vec<serde_json::Value> {
    let sink = Capture::new();
    let telemetry = sutura_config::TelemetrySettings::new(
        sutura_config::ServiceName::parse("sutura-test").expect("a test service name is a name"),
        sutura_config::LogFilter::parse("info").expect("a test directive is a directive"),
        sutura_config::LogFormat::Bunyan,
        true,
    );
    let subscriber =
        sutura_runtime::telemetry::subscriber(&telemetry, sink.clone()).expect("a valid directive builds a subscriber");
    let args = [
        example().join("catalog").display().to_string(),
        example().join("questions").join(question).display().to_string(),
        example().join("data").display().to_string(),
    ];
    // The exit code is bound and not asserted on: `ExitCode` implements no comparison and no
    // accessor, so there is nothing honest to compare it with. The record is the assertion, and for
    // an answered question it exists only on the path that got as far as printing rows.
    let _exit = tracing::subscriber::with_default(subscriber, || crate::commands::query(&args));
    sink.contents()
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|line| matches!(message(line), Some("answered" | "refused")))
        .collect()
}

/// The message a record carries: `answered` or `refused`.
fn message(record: &serde_json::Value) -> Option<&str> {
    record.get("msg").and_then(serde_json::Value::as_str)
}

/// One field of a record, by key.
///
/// By KEY and never by substring, which is the difference between asserting a record and asserting
/// that a string appears somewhere in a log: a value written onto the wrong field reads the same to
/// a `contains` and means nothing to a collector.
fn field<'record>(record: &'record serde_json::Value, key: &str) -> Option<&'record str> {
    record.get(key).and_then(serde_json::Value::as_str)
}

/// The one record this command wrote.
fn only(records: &[serde_json::Value]) -> &serde_json::Value {
    assert_eq!(records.len(), 1, "one record per outcome: {records:?}");
    records.first().expect("a set of one has a first")
}

/// **A1 of issue #266.** This command called `sutura_app::answer` directly and dropped the deadline
/// with `into_outcome`, so the one shipped command a person runs on a terminal answered a certified
/// question with no record - while the invariants row said every outcome is recorded before it is
/// returned and named the service constructor this command did not use.
#[test]
fn an_answered_question_is_recorded() {
    let written = records("recurring-revenue-june.yaml");
    let record = only(&written);
    assert_eq!(message(record), Some("answered"), "{record:?}");
    assert_eq!(
        field(record, "definition_version"),
        Some("local-working-tree"),
        "the record says which definitions produced the number: {record:?}"
    );
    // The honest subject for this command: no transport established anybody, so the identity the
    // data system was reached under is the process's own, and the record says so rather than
    // leaving the field empty.
    assert_eq!(
        field(record, "subject_established"),
        Some("deployment"),
        "the record says what established the subject: {record:?}"
    );
    assert!(
        record.get("rows").and_then(serde_json::Value::as_u64).is_some(),
        "an answer's record carries the row count: {record:?}"
    );
}

/// The other outcome, and it is the one a carve-out would most easily have been argued for: a
/// refusal reaches the caller inside the `Ok`, so nothing about it looks like an error to skip.
#[test]
fn a_refused_question_is_recorded_and_names_the_refusal() {
    let written = records("refused-metric-unknown.yaml");
    let record = only(&written);
    assert_eq!(message(record), Some("refused"), "{record:?}");
    assert!(
        field(record, "reason").is_some_and(|reason| reason.contains("MetricUnknown")),
        "the record names which refusal it was: {record:?}"
    );
}
