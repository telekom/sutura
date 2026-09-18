//! The fragments the pinned parser once did not return from, replayed against the fix.
//!
//! A submodule for the same mechanical reason as [`super::dialect_resolution`]: `cargo xtask
//! max-lines` fails at a thousand lines under `crates/` and the parent is within reach of it. The
//! `#[test]` that calls into this module stays in the parent, because the `mod` line and the test
//! have to sit in one file for `just causality` to compile either against base.
//!
//! #589: `polyglot-sql`'s `Parser::parse_data_type` used to loop forever on an unclosed
//! parenthesis, so `check` refused the condition before handing the fragment to the parse. Fixed
//! upstream by `tobilg/polyglot#447`, released in 0.12.0. These cells assert what the fix means
//! for a caller: the fragment now comes back as [`super::super::ExpressionError::Unparsable`], the
//! parser's own diagnosis, in place of the deleted guard's `UnclosedParenthesis`. **On the base
//! tree - the guard still present - every assertion below is red**: the guard answers first, and
//! its sentence never contains `is not SQL`.

use std::thread;
use std::time::{Duration, Instant};

use super::portable;

/// Four shapes of the same defect, delta-debugged in the issue that tracked it: `.:` reads the
/// next word as a custom data type and reaches the same loop with no `.:` in it at all
/// (`CAST(mrr_eur AS S1(9`), and a bare `SUM(mrr_eur` needs no `.:` either. All four are genuinely
/// unclosed, so the parse - not any bound here - is what has to refuse them.
#[test]
fn four_more_unclosed_shapes_are_refused_by_the_parse() {
    for fragment in ["mrr_eur.:S1(", "mrr_eur.:S1(')'", "CAST(mrr_eur AS S1(9", "SUM(mrr_eur"] {
        let verdict = verdict_within_the_deadline(fragment);
        assert!(
            verdict.contains("is not SQL"),
            "{fragment:?} opens a parenthesis it never closes and must be refused, got: {verdict}"
        );
    }
}

/// Every fuzz artifact this defect produced, quarantined as the committed seed named beside it so
/// `just fuzz-smoke` replays exactly these bytes. Three artifacts, one upstream defect - two
/// timeouts while `format!` copied a growing string, one out-of-memory once the copying had
/// churned enough of it - reproduced individually against the pinned parser: none returned.
const RECORDED: &[&[u8]] = &[
    b"$a^-a61.c.:S1a.:#S1(^cAUAU", // `unclosed-paren-timeout`
    b"SSE%LE.:E.E.:SEIF(~~~$R_ta", // `unclosed-paren-timeout-in-subscript`
    b"IF~F((NU .:rv ((NU .:r~>",   // `unclosed-paren-oom`
];

/// How long the compile is given to come back at all.
///
/// Not a performance assertion - with the fix the call returns in microseconds. This deadline only
/// matters if a future pin regresses the loop, so it fails the test rather than hanging the suite.
const MUST_RETURN_WITHIN: Duration = Duration::from_secs(10);

/// What the compile said about one fragment, within the deadline.
///
/// A worker thread and a poll loop rather than a bare call: on the base tree a fragment that still
/// loops never returns, and a bare `assert!` on the result would hang the suite instead of failing
/// it.
fn verdict_within_the_deadline(sql: &str) -> String {
    let owned = String::from(sql);
    let worker = thread::spawn(move || match portable(&owned) {
        Err(refusal) => refusal.to_string(),
        Ok(compiled) => format!("ACCEPTED: {:?}", compiled.renderings()),
    });
    let deadline = Instant::now() + MUST_RETURN_WITHIN;
    while !worker.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        worker.is_finished(),
        "the compile did not return within {MUST_RETURN_WITHIN:?} on {sql:?}"
    );
    worker.join().expect("the compile thread must not panic")
}

/// The body of [`super::the_sql_fuzz_artifacts_replay_as_a_refusal_not_an_unbounded_parse`]. Kept
/// under its original name so that wrapper needs no change: what moved is the assertion inside,
/// not the shape of the call.
pub(super) fn the_recorded_artifacts_are_refused_at_the_parenthesis_they_opened() {
    for artifact in RECORDED {
        let fragment = String::from_utf8_lossy(artifact).into_owned();
        let verdict = verdict_within_the_deadline(&fragment);
        assert!(
            verdict.contains("is not SQL"),
            "{fragment:?} must be refused by the parse itself, got: {verdict}"
        );
    }
}
