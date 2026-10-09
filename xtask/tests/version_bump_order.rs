#![forbid(unsafe_code)]
//! `check-version-bump` refuses a `cliff.toml` whose record skip rule comes before a
//! breaking-change rule, through the real binary.
//!
//! With the skip rule first, a breaking commit that names a record is filed under its type and the
//! release notes lose the "Breaking changes" heading. A dedicated `tests/` target, so `just
//! causality` can run it against the base tree: the base gate exits 0 over the reordered config
//! below, and this cell is red there by assertion.

#![cfg(test)]

use std::path::{Path, PathBuf};
use std::process::Output;

const WORKFLOW: &str = ".github/workflows/version-bump.yml";
const CLIFF: &str = "cliff.toml";

fn repo_file(rel: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("the workspace root");
    std::fs::read_to_string(root.join(rel)).expect("a file of the real tree")
}

/// A fresh fixture tree: the two root markers `repo::root` looks for, the real workflow and the
/// given `cliff.toml`.
fn tree(case: &str, cliff: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("version-bump-order-{case}-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clearing a stale fixture");
    }
    std::fs::create_dir_all(root.join(".github/workflows")).expect("the fixture workflow directory");
    for (rel, text) in [
        ("flake.nix", "{ }\n".to_owned()),
        ("Cargo.toml", "[workspace]\nmembers = []\n".to_owned()),
        (WORKFLOW, repo_file(WORKFLOW)),
        (CLIFF, cliff.to_owned()),
    ] {
        std::fs::write(root.join(rel), text).expect("a fixture file");
    }
    root
}

fn check_version_bump(root: &Path) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("check-version-bump")
        .current_dir(root)
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("execute the real xtask binary")
}

fn said(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// `cliff` with the record skip rule moved above the first breaking-change rule.
fn skip_first(cliff: &str) -> String {
    let skip = cliff
        .lines()
        .find(|line| line.contains("skip = true") && line.contains("docs/adr/"))
        .expect("the record skip rule");
    let rest = cliff.replace(&format!("{skip}\n"), "");
    let first = rest
        .lines()
        .find(|line| line.contains("Breaking changes"))
        .expect("a breaking-change rule");
    rest.replacen(first, &format!("{skip}\n{first}"), 1)
}

#[test]
fn a_record_skip_rule_above_the_breaking_rules_is_refused_and_the_real_config_is_clean() {
    let real = repo_file(CLIFF);
    let clean = check_version_bump(&tree("real", &real));
    assert!(clean.status.success(), "{}", said(&clean));

    let moved = skip_first(&real);
    assert_ne!(moved, real, "the fixture must differ from the real config");
    let output = check_version_bump(&tree("skip-first", &moved));
    let text = said(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("cliff.toml"), "{text}");
    assert!(text.contains("Breaking changes"), "{text}");
}
