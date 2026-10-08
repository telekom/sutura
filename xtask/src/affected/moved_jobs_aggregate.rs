//! The `causality` and `postgres-linked-driver` arms of the real `ci-aggregate` shell (#1280), run
//! through `aggregate_shell`'s harness. Both were steps of `ci`, so the required context that gated
//! them before is the aggregate now: a skip or a failure where `ci` selected them must be red.

use super::aggregate_shell::run_aggregator;

/// (result var, required var, the name the verdict must carry).
type Moved = (&'static str, &'static str, &'static str);

const MOVED: [Moved; 2] = [
    ("CAUS_RESULT", "CAUS_REQUIRED", "causality"),
    ("PGD_RESULT", "PGD_REQUIRED", "postgres-linked-driver"),
];

/// A clean aggregate but for the one pair under test.
fn with(result_var: &'static str, result: &'static str, required_var: &'static str, required: &'static str) -> (bool, String) {
    run_aggregator(&[
        ("CI_RESULT", "success"),
        ("KC_RESULT", "skipped"),
        ("KC_SELECTED", ""),
        ("BQ_RESULT", "skipped"),
        ("BQ_SELECTED", ""),
        (result_var, result),
        (required_var, required),
    ])
}

#[test]
fn a_required_moved_job_that_did_not_succeed_is_red() {
    for (result_var, required_var, name) in MOVED {
        for result in ["skipped", "failure", "cancelled"] {
            let (ok, text) = with(result_var, result, required_var, "true");
            assert!(!ok, "a selected `{name}` that reported {result} cannot pass: {text}");
            assert!(text.contains(&format!("{name} must run")), "the leg must be named: {text}");
        }
    }
}

#[test]
fn a_failed_unrequired_moved_job_is_red() {
    for (result_var, required_var, name) in MOVED {
        let (ok, text) = with(result_var, "failure", required_var, "false");
        assert!(!ok, "a failed `{name}` cannot be waived: {text}");
        assert!(
            text.contains(&format!("{name} reported")),
            "the failed leg must be named: {text}"
        );
    }
}
