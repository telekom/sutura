#![forbid(unsafe_code)]
//! `check-fuzz` must refuse a target whose imported crate reaches neither hook surface.
//!
//! `github.com/telekom/sutura#867`. Drives the real binary against a fixture tree rather than
//! unit-testing the reader directly: `xtask/src/hook_coverage/surfaces.rs`'s `SURFACES` table is
//! compiled into the binary, not fixture-controllable, so the only crate name guaranteed absent
//! from it - on this fixture and on the real repo alike - is one nobody has registered.

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

/// A crate name no real target imports and no hook surface will ever list.
const UNLISTED_CRATE: &str = "sutura_unlisted_by_either_hook_surface";

const FUZZ_YAML: &str =
    "on:\n  workflow_dispatch: {}\njobs:\n  fuzz:\n    strategy:\n      matrix:\n        target:\n          - probe\n";

const RELEASE_YAML: &str = "on:\n  push:\n    tags: [v*]\njobs:\n  build:\n    steps:\n      - run: echo nothing\n";

fn precommit(files_pattern: &str) -> String {
    format!(
        "default_install_hook_types: [pre-commit, pre-push, commit-msg]\nrepos:\n  - repo: local\n    hooks:\n      - id: fuzz\n        name: fuzz (git delta)\n        entry: bash nix/run-fuzz.sh smoke\n        language: system\n        files: {files_pattern}\n        pass_filenames: false\n"
    )
}

fn probe_source(crate_name: &str) -> String {
    format!(
        "#![no_main]\nuse libfuzzer_sys::fuzz_target;\nuse {crate_name}::Thing;\nfuzz_target!(|data: &[u8]| {{ let _ = data; let _ = std::marker::PhantomData::<Thing>; }});\n"
    )
}

fn observe(case: &str, target_crate: &str) -> std::process::Output {
    let tree = scratch_tree::Tree::of(
        &format!("hook-paths-{case}"),
        &[
            ("Cargo.toml", b"[workspace]\n" as &[u8]),
            ("flake.nix", b"{}\n"),
            (".pre-commit-config.yaml", precommit("^(fuzz/)").as_bytes()),
            (
                "fuzz/Cargo.toml",
                b"[package]\nname = \"fuzz\"\n\n[[bin]]\nname = \"probe\"\npath = \"fuzz_targets/probe.rs\"\n\n[profile.release]\npanic = \"abort\"\n",
            ),
            ("fuzz/Cargo.lock", b""),
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
fn a_target_importing_a_crate_neither_hook_surface_claims_is_refused() {
    let output = observe("unlisted", UNLISTED_CRATE);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8(output.stderr).expect("gate diagnostics");
    assert!(
        stderr.contains(&UNLISTED_CRATE.replace('_', "-")),
        "the refusal must name the uncovered crate: {stderr}"
    );
    assert!(
        stderr.contains("no row in xtask/src/hook_coverage/surfaces.rs claims"),
        "the surfaces gap must be reported: {stderr}"
    );
    assert!(
        stderr.contains("`files:` pattern"),
        "the hook-files gap must be reported: {stderr}"
    );
}

#[test]
fn a_target_importing_no_crate_at_all_is_unaffected_by_the_correlation() {
    let output = observe("no-import", "std");
    // `std` never matches `sutura_[a-z_]+`, so there is nothing to correlate and every other
    // requirement this fixture satisfies (declared, seeded, in the matrix, locked, aborting)
    // carries the gate to a clean pass.
    let stderr = String::from_utf8(output.stderr.clone()).expect("gate diagnostics");
    assert_eq!(output.status.code(), Some(0), "{stderr}");
}
