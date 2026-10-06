#![forbid(unsafe_code)]
//! `check-fuzz` over a fixture tree, driven through the real binary.
//!
//! The fixture holds no `.pre-commit-config.yaml`: the fuzz replay is CI-only, so the gate reads no
//! hook file and a tree without one must pass.

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

const FUZZ_YAML: &str =
    "on:\n  workflow_dispatch: {}\njobs:\n  fuzz:\n    strategy:\n      matrix:\n        target:\n          - probe\n";

/// Every site `check-fuzz` requires to run the fuzz-scoped `cargo deny` (`fuzz/deny_wiring.rs`).
const DENY: &[u8] = b"cargo deny --manifest-path fuzz/Cargo.toml check\n";

const RELEASE_YAML: &str = "on:\n  push:\n    tags: [v*]\njobs:\n  build:\n    steps:\n      - run: echo nothing\n";

fn probe_source(crate_name: &str) -> String {
    format!(
        "#![no_main]\nuse libfuzzer_sys::fuzz_target;\nuse {crate_name}::Thing;\nfuzz_target!(|data: &[u8]| {{ let _ = data; let _ = std::marker::PhantomData::<Thing>; }});\n"
    )
}

fn observe(case: &str, target_crate: &str, root_lock: &str, fuzz_lock: &str) -> std::process::Output {
    let tree = scratch_tree::Tree::of(
        &format!("check-fuzz-{case}"),
        &[
            ("Cargo.toml", b"[workspace]\n" as &[u8]),
            ("Cargo.lock", root_lock.as_bytes()),
            ("flake.nix", DENY),
            ("justfile", DENY),
            ("nix/run-gate.sh", DENY),
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
