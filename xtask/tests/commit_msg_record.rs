#![forbid(unsafe_code)]
//! `commit-msg` and `check-pr-title` refuse a `feat`, `fix` or breaking subject that names a
//! decision record, through the real binary.
//!
//! `cliff.toml` skips every commit whose subject names a record, so a change a user reads must
//! not be one. A dedicated `tests/` target, so `just causality` can run it against the base tree:
//! the base gates exit 0 over each refused subject below, and these cells are red there by
//! assertion.

#![cfg(test)]

use std::path::Path;
use std::process::{Command, Output};

fn xtask() -> Command {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
}

/// The hook's verdict over a message whose subject is `subject`. The body names a record too: a
/// body is not judged.
fn commit_msg(case: &str, subject: &str) -> Output {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("commit-msg-record-{case}-{}", std::process::id()));
    std::fs::write(&path, format!("{subject}\n\nThe body cites ADR 0011, which is allowed.\n")).expect("the message file");
    xtask()
        .arg("commit-msg")
        .arg(&path)
        .output()
        .expect("execute the real xtask binary")
}

fn check_pr_title(title: &str) -> Output {
    xtask()
        .arg("check-pr-title")
        .arg(title)
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

fn refused(output: &Output) {
    let text = said(output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("names a decision record"), "{text}");
}

#[test]
fn a_feat_subject_naming_a_record_is_refused_and_the_same_text_as_docs_is_not() {
    refused(&commit_msg("feat", "feat(config): add the knob, as ADR 0011 decides"));
    for kept in [
        "docs(adr): ADR 0011 says why the knob exists",
        "feat(config): add the knob",
        "feat: an ADRESS column and an ADR 11 note",
    ] {
        let output = commit_msg("kept", kept);
        assert!(output.status.success(), "{kept}: {}", said(&output));
    }
}

#[test]
fn a_fix_a_breaking_and_a_path_subject_naming_a_record_is_refused() {
    for subject in [
        "fix(http): stop the leak that docs/adr/0007-x.md describes",
        "refactor!: rename the port of ADR-0015",
        "chore(ci)!: drop the runner that ADR 0016 names",
    ] {
        refused(&commit_msg("other", subject));
    }
}

#[test]
fn a_pull_request_title_naming_a_record_is_refused_for_a_feat_and_not_for_docs() {
    refused(&check_pr_title("feat: the knob of ADR 0011"));
    let kept = check_pr_title("docs: the knob of ADR 0011");
    assert!(kept.status.success(), "{}", said(&kept));
}
