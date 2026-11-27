//! The `causality` and `postgres-linked-driver` (#1280) and `shipped-features` (#1696) arms of the
//! real `ci-aggregate` shell, run through `aggregate_shell`'s harness. All three were steps of `ci`,
//! so the required context that gated them before is the aggregate now: a skip or a failure where
//! `ci` selected them must be red. The `cross` arm is here too: its legs report a context no ruleset
//! lists, so a red one reaches a merge only through this shell.

use super::aggregate_shell::{run_aggregator, run_qa};

/// (result var, required var, the name the verdict must carry).
type Moved = (&'static str, &'static str, &'static str);

const MOVED: [Moved; 4] = [
    ("CAUS_RESULT", "CAUS_REQUIRED", "causality"),
    ("PGD_RESULT", "PGD_REQUIRED", "postgres-linked-driver"),
    ("SHIP_RESULT", "SHIP_REQUIRED", "shipped-features"),
    ("FUZZ_RESULT", "FUZZ_REQUIRED", "fuzz-compile"),
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

#[test]
fn a_cross_leg_that_ran_and_is_not_green_is_red_and_a_skipped_one_is_not() {
    for result in ["failure", "cancelled"] {
        let (ok, text) = run_qa(&[("CROSS_RESULT", result)]);
        assert!(!ok, "a `cross` that reported {result} cannot pass: {text}");
        assert!(text.contains("cross-link reported"), "the leg must be named: {text}");
    }
    // A cross job unchanged or outside the selection may skip.
    for result in ["success", "skipped"] {
        let (ok, text) = run_qa(&[("CROSS_RESULT", result)]);
        assert!(ok, "a `cross` that reported {result} must aggregate green: {text}");
    }
}

#[test]
fn the_fuzz_compile_job_is_pinned_like_the_other_classified_lanes() {
    // Held here rather than beside `classify` in `xtask/src/changes.rs`: this round's
    // `fuzz_compile` implementation lines live in that file too, so a test added there is not
    // mechanically separable from them for `cargo xtask causality`. This file is declared
    // `#[cfg(test)]` by `affected.rs` (`regions.rs::declared_under_cfg_test`, matched at this
    // file's own default-layout path), so the whole thing reads as test code no matter what
    // else it is next to.
    //
    // `fuzz_compile` has no `just gates` task of its own (`justfile`'s `fuzz-compile` recipe
    // calls `nix run .#fuzz-compile` directly), so unlike
    // `default_features::tests::both_lanes_still_invoke_this_gate` this holds only the CI half:
    // the job still runs the app, still gates rather than reports, and is still selected by the
    // flag `changes::touches_fuzzed_tree` sets.
    const WORKFLOW: &str = ".github/workflows/ci.yml";
    const JOB: &str = "fuzz-compile";
    const APP: &str = "fuzz-compile";
    const CLASSIFIED: &str = "needs.classify.outputs.fuzz_compile == 'true'";

    let root = crate::repo::root().expect("the repo root");
    let workflow = std::fs::read_to_string(root.join(WORKFLOW)).expect("ci.yml");
    crate::workflows::step::app_step(&workflow, JOB, APP)
        .unwrap_or_else(|| panic!("a live `nix run .#{APP}` step inside {WORKFLOW}'s `{JOB}` job"));
    let job = crate::workflows::step::job(&workflow, JOB).unwrap_or_default();
    let live = || job.iter().filter(|line| !line.trim_start().starts_with('#'));
    assert!(
        !live().any(|line| line.contains("continue-on-error")),
        "`{JOB}` tolerates a failure, so the CI half reports rather than gates:\n{}",
        job.join("\n")
    );
    assert!(
        live().any(|line| line.starts_with("    if:") && line.contains(CLASSIFIED)),
        "`{JOB}` is not selected by `{CLASSIFIED}`, the condition the step had in `ci`"
    );
}
