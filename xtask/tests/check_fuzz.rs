#![forbid(unsafe_code)]
//! `check-fuzz` over a fixture tree, driven through the real binary.
//!
//! The fixture holds no `.pre-commit-config.yaml`: no commit hook replays the seeds, so the gate
//! reads no hook file and a tree without one must pass. A target's imports are checked against the
//! "fuzzed tree" surface instead (`github.com/telekom/sutura#867`): `SURFACES` is compiled into the
//! binary, not fixture-controllable, so the only crate name guaranteed absent from it - on this
//! fixture and on the real repo alike - is one nobody has registered.

#![cfg(test)]

// The fixture tree is a [`scratch_tree::Tree`] - the sweep-before-create and the `Drop` sweep the
// module holds are the two halves #938's leftover trap had - rather than a hand-rolled exclusive
// `create_dir` on a pid-keyed path, which is what this file did until #938. `seal` and `Fixture`
// are the module's own tests' items; this integration test does not use them, so the include
// expects `dead_code` on it rather than weakening the module for everyone.
#[path = "scratch_tree/mod.rs"]
#[expect(
    dead_code,
    reason = "the module's own tests use `seal` and `Fixture`; this integration test does not"
)]
#[cfg(test)]
mod scratch_tree;

use std::process::Command;

/// A crate name no real target imports and no surface will ever list.
const UNLISTED_CRATE: &str = "sutura_unlisted_by_the_fuzzed_tree_surface";

const FUZZ_YAML: &str =
    "on:\n  workflow_dispatch: {}\njobs:\n  fuzz:\n    strategy:\n      matrix:\n        target:\n          - probe\n";

/// Every site `check-fuzz` requires to run the fuzz-scoped and the driver-scoped `cargo deny`
/// (`fuzz/deny_wiring.rs`).
const DENY: &[u8] = b"cargo deny --manifest-path fuzz/Cargo.toml check\nbash nix/deny-clickhouse-driver.sh\n";

/// The same sites with only one of the two runs.
const DENY_FUZZ_ONLY: &[u8] = b"cargo deny --manifest-path fuzz/Cargo.toml check\n";
const DENY_DRIVER_ONLY: &[u8] = b"bash nix/deny-clickhouse-driver.sh\n";

const RELEASE_YAML: &str = "on:\n  push:\n    tags: [v*]\njobs:\n  build:\n    steps:\n      - run: echo nothing\n";

fn probe_source(crate_name: &str) -> String {
    format!(
        "#![no_main]\nuse libfuzzer_sys::fuzz_target;\nuse {crate_name}::Thing;\nfuzz_target!(|data: &[u8]| {{ let _ = data; let _ = std::marker::PhantomData::<Thing>; }});\n"
    )
}

fn observe(case: &str, target_crate: &str, root_lock: &str, fuzz_lock: &str) -> std::process::Output {
    observe_with_deny(case, target_crate, root_lock, fuzz_lock, DENY)
}

fn observe_with_deny(case: &str, target_crate: &str, root_lock: &str, fuzz_lock: &str, deny: &[u8]) -> std::process::Output {
    let tree = scratch_tree::Tree::of(
        &format!("check-fuzz-{case}"),
        &[
            ("Cargo.toml", b"[workspace]\n" as &[u8]),
            ("Cargo.lock", root_lock.as_bytes()),
            ("flake.nix", deny),
            ("justfile", deny),
            ("nix/run-gate.sh", deny),
            (
                "fuzz/Cargo.toml",
                b"[package]\nname = \"fuzz\"\n\n[[bin]]\nname = \"probe\"\npath = \"fuzz_targets/probe.rs\"\n\n[profile.release]\npanic = \"abort\"\n",
            ),
            ("fuzz/Cargo.lock", fuzz_lock.as_bytes()),
            ("fuzz/fuzz_targets/probe.rs", probe_source(target_crate).as_bytes()),
            ("fuzz/seeds/probe/seed", b"seed"),
            (".github/workflows/fuzz.yml", FUZZ_YAML.as_bytes()),
            (".github/workflows/release.yml", RELEASE_YAML.as_bytes()),
        ],
    );
    let root = tree.root().to_path_buf();

    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("check-fuzz")
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("execute the real xtask binary");
    drop(tree);
    output
}

#[test]
fn a_target_importing_a_crate_the_fuzzed_tree_surface_misses_is_refused() {
    let output = observe("unlisted", UNLISTED_CRATE, "", "");
    let stderr = String::from_utf8(output.stderr.clone()).expect("gate diagnostics");
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains(&UNLISTED_CRATE.replace('_', "-")),
        "the refusal must name the uncovered crate: {stderr}"
    );
    assert!(
        stderr.contains("`fuzzed tree` row in xtask/src/hook_coverage/surfaces.rs never names"),
        "the surface gap must be reported: {stderr}"
    );
}

/// The row once named two `BigQuery` paths that no longer exist, and a prefix match on
/// `crates/sutura-exec-bigquery/` let them cover an import of the crate.
#[test]
fn a_target_importing_the_bigquery_crate_is_refused_because_the_row_names_none_of_it() {
    let output = observe("bigquery", "sutura_exec_bigquery", "", "");
    let stderr = String::from_utf8(output.stderr.clone()).expect("gate diagnostics");
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("sutura-exec-bigquery"),
        "the refusal must name the crate: {stderr}"
    );
}

#[test]
fn a_complete_fixture_passes_with_no_hook_config_in_the_tree() {
    let output = observe("no-import", "std", "", "");
    // Every requirement this fixture satisfies (declared, seeded, in the matrix, locked,
    // aborting) carries the gate to a clean pass with no hook file in the tree.
    let stderr = String::from_utf8(output.stderr.clone()).expect("gate diagnostics");
    assert_eq!(output.status.code(), Some(0), "{stderr}");
}

/// The `yoke-derive` drift that reddened every pull request once crates.io yanked 0.8.3, refused
/// by the real binary - so the wiring into `check-fuzz` is held, not only `lock_drift::gaps`.
#[test]
fn a_fuzz_lock_pin_the_root_lock_does_not_hold_is_refused() {
    let pin = |version: &str| format!("[[package]]\nname = \"yoke-derive\"\nversion = \"{version}\"\n");
    let output = observe("lock-drift", "std", &pin("0.8.2"), &pin("0.8.3"));
    let stderr = String::from_utf8(output.stderr.clone()).expect("gate diagnostics");
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("fuzz/Cargo.lock pins yoke-derive at \"0.8.3\", but Cargo.lock pins it at \"0.8.2\""),
        "the refusal must name the package and both versions: {stderr}"
    );
}

/// The driver lock's `cargo deny` is required at every site, refused by the real binary - so the
/// call that folds `deny_wiring::driver_gaps` into the gate is held, not only the unit cells over it.
#[test]
fn a_site_without_the_driver_deny_script_is_refused() {
    let output = observe_with_deny("driver-deny", "std", "", "", DENY_FUZZ_ONLY);
    let stderr = String::from_utf8(output.stderr.clone()).expect("gate diagnostics");
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("does not run `nix/deny-clickhouse-driver.sh`"),
        "the refusal must name the script: {stderr}"
    );
}

/// The same for the fuzz lock's run, whose `deny_wiring::gaps` call no other cell held.
#[test]
fn a_site_without_the_fuzz_deny_run_is_refused() {
    let output = observe_with_deny("fuzz-deny", "std", "", "", DENY_DRIVER_ONLY);
    let stderr = String::from_utf8(output.stderr.clone()).expect("gate diagnostics");
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("does not run `cargo deny --manifest-path fuzz/Cargo.toml`"),
        "the refusal must name the fuzz run: {stderr}"
    );
}
