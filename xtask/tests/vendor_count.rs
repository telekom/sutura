#![forbid(unsafe_code)]
//! `check-vendor-count`'s own module carries the unit-level proof for every branch; this is the
//! separate integration target `sutura/gates`' causality note asks for, so the gate's PRODUCTION
//! and its inline tests staying in one file does not orphan the mechanical red-before-green
//! proof `AGENTS.md` requires. A dedicated `tests/` file is `TestScope::WholeFile` by path alone
//! (`xtask/src/causality/regions.rs`), so it needs no `mod` declaration to stay reachable and
//! nothing here can be "held back" the way `vendor_count.rs` itself is: reverting the base tree
//! removes the gate entirely, and the built `xtask` binary answers `unknown task` instead of
//! running it - which is the base failure this file's own cells are red against, with no
//! red/green branching written here. `xtask/tests/default_features.rs` is the shape this copies.

#![cfg(test)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A fresh fixture tree: the two root markers `repo::root` looks for, plus whichever of
/// `VENDOR.md`, `Cargo.toml` and `REUSE.toml` the case supplies. `Cargo.toml` doubles as a root
/// marker and (when given) as one of the two repeaters - exactly the file the real tree uses for
/// both jobs.
fn tree(case: &str, vendor_md: Option<&str>, cargo_toml: Option<&str>, reuse_toml: Option<&str>) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("vendor-count-{case}-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clearing a stale fixture");
    }
    std::fs::create_dir_all(&root).expect("the fixture root");
    std::fs::write(root.join("flake.nix"), "{ }\n").expect("the first root marker");
    std::fs::write(root.join("Cargo.toml"), cargo_toml.unwrap_or("[workspace]\nmembers = []\n")).expect("the second root marker");
    if let Some(text) = vendor_md {
        std::fs::write(root.join("VENDOR.md"), text).expect("VENDOR.md");
    }
    if let Some(text) = reuse_toml {
        std::fs::write(root.join("REUSE.toml"), text).expect("REUSE.toml");
    }
    root
}

/// Run the real, built `xtask check-vendor-count` against `root`.
fn run(root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("check-vendor-count")
        .current_dir(root)
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("execute the real xtask binary")
}

/// One row of the mimalloc table, with `cardinal` and `items` free to vary independently - the
/// same helper `vendor_count.rs`'s own unit tests keep, duplicated here because this crate has no
/// `[lib]` target for an integration test to `use`.
fn row(cardinal: &str, items: &str) -> String {
    format!("| `vendor/mimalloc_rust/**` | up | MIT | abc | 2026-01-01 | Vendored. {cardinal} local changes. {items} |\n")
}

const SIX_ITEMS: &str = "(1) a. (2) b. (3) c. (4) d. (5) e. (6) f.";

#[test]
fn a_clean_tree_exits_zero_through_the_real_binary() {
    let root = tree(
        "clean",
        Some(&row("Six", SIX_ITEMS)),
        Some("[workspace]\n# six local changes to the build script and both manifests.\n"),
        Some("# the six local changes; this records the licence.\n"),
    );
    let output = run(&root);
    std::fs::remove_dir_all(&root).expect("remove the owned fixture");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}

#[test]
fn a_capitalised_cardinal_still_passes_through_the_real_binary() {
    // THE #857 BUG, END TO END: the mismatch is capitalised at sentence start in every one of the
    // three sites, exactly as the real VENDOR.md/Cargo.toml/REUSE.toml write it - a lowercase-only
    // search misses every one of these.
    let root = tree(
        "capitalised",
        Some(&row("Six", SIX_ITEMS)),
        Some("[workspace]\n# Six local changes to the build script and both manifests.\n"),
        Some("# Six local changes; this records the licence.\n"),
    );
    let output = run(&root);
    std::fs::remove_dir_all(&root).expect("remove the owned fixture");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}

#[test]
fn a_cardinal_versus_enumeration_mismatch_fails_through_the_real_binary() {
    // VENDOR.md's own row: the cardinal says Seven, the cell still enumerates six items. Both
    // repeaters ALSO say seven - reviewed and measured wrong when they said six, because that
    // made this fixture a cross-file mismatch too, and it survived deleting the row's own
    // cardinal-versus-enumeration comparison outright (the cross-file check caught it instead).
    // Held now, by `desync_the_enumeration_alone_fails` (the unit-level twin of this cell) and
    // by nothing else in either file.
    let root = tree(
        "internal-mismatch",
        Some(&row("Seven", SIX_ITEMS)),
        Some("[workspace]\n# seven local changes to the build script and both manifests.\n"),
        Some("# the seven local changes; this records the licence.\n"),
    );
    let output = run(&root);
    std::fs::remove_dir_all(&root).expect("remove the owned fixture");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8(output.stderr).expect("gate diagnostics");
    assert!(stderr.contains("VENDOR.md"), "{stderr}");
}

#[test]
fn a_repeater_stating_a_different_cardinal_fails_through_the_real_binary() {
    let root = tree(
        "cross-file-mismatch",
        Some(&row("Six", SIX_ITEMS)),
        Some("[workspace]\n# seven local changes to the build script and both manifests.\n"),
        Some("# the six local changes; this records the licence.\n"),
    );
    let output = run(&root);
    std::fs::remove_dir_all(&root).expect("remove the owned fixture");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8(output.stderr).expect("gate diagnostics");
    assert!(stderr.contains("Cargo.toml"), "{stderr}");
}

#[test]
fn a_repeater_that_stopped_stating_the_count_fails_through_the_real_binary() {
    // Not a vacuous pass: deleting the comment instead of fixing a wrong number must still fail.
    let root = tree(
        "repeater-dropped",
        Some(&row("Six", SIX_ITEMS)),
        Some("[workspace]\nno mention of any local changes here\n"),
        Some("# the six local changes; this records the licence.\n"),
    );
    let output = run(&root);
    std::fs::remove_dir_all(&root).expect("remove the owned fixture");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8(output.stderr).expect("gate diagnostics");
    assert!(stderr.contains("Cargo.toml"), "{stderr}");
}

#[test]
fn an_absent_vendor_md_fails_through_the_real_binary() {
    let root = tree(
        "absent-vendor-md",
        None,
        Some("[workspace]\nsix local changes\n"),
        Some("six local changes\n"),
    );
    let output = run(&root);
    std::fs::remove_dir_all(&root).expect("remove the owned fixture");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
}
