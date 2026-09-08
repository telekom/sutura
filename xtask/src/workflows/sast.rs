//! An accepted Scorecard zero is a claim about what stands in its place, so the stand-in is read.
//!
//! `docs/adr/0025` accepts Scorecard's `SAST = 0` rather than answering it, and the argument it
//! makes is not *this does not matter* - it is **the property is held here by something Scorecard
//! does not look for**: `checks.clippy` under `-D warnings` and `zizmor` over the workflows, both
//! inside the one required context. That is a load-bearing public statement, published beside a
//! score somebody will read, and until this module **nothing held either half of it**.
//!
//! The failure it guards against is not a red run. It is the ADR going on asserting a stand-in
//! after the stand-in stopped existing - `-D warnings` dropped from `cargoClippyExtraArgs`, or the
//! Clippy step lifted out of `ci.yml` the way `cross-link.yml` was lifted out of it at the
//! 1000-line cap. Either one leaves every gate in this repository green and turns a recorded
//! decision into an overstated control, which `AGENTS.md` calls the defect itself.
//!
//! # The three refusals, and why each is the shape it is
//!
//! * **`cargoClippyExtraArgs` must still carry `-D warnings`.** The attribute is read off
//!   `flake.nix`'s CODE half, so the prose in `Cargo.toml` and `crate::default_features` that
//!   *describes* the gate cannot satisfy it; the flag itself is read off the raw line, because
//!   [`super::code_lines`] blanks string literals and the flag lives inside the quotes. Without it
//!   clippy reports and exits 0, which is the dead-check shape: the step stays green, the lints
//!   still print, and nothing fails.
//! * **CI must still invoke that check.** Asked of the reference set [`super::gather`] already
//!   walked rather than of a substring, for the reason `super::step`'s header gives - a
//!   commented-out step and a step moved to a job nobody requires both satisfy `contains`. This
//!   gate does NOT read whether the job is required, because no file in this tree carries that;
//!   `devco/required-contexts` is the dated read and `crate::workflows::contexts` the closest
//!   mechanism.
//! * **No workflow may add a Scorecard-recognised SAST tool while the ADR still accepts the
//!   zero.** The direction is deliberate and it is the one that keeps the record honest: adding
//!   `CodeQL` later is a good change, and it makes `docs/adr/0025` wrong the moment it lands. So the
//!   gate refuses the combination rather than the tool, and its message names the file to edit.
//!
//! # What this does not hold
//!
//! * **Not that clippy finds anything.** A lint set is not a taint analysis, and `docs/adr/0025`
//!   is explicit that no mechanism here tracks a value from a caller to a sink. This module holds
//!   that the mechanism the ADR cites still runs - not that citing it was the right call.
//! * **Not the zizmor half, beyond its invocation.** `nix/lint-workflows.sh` is reached through
//!   the same reference set, so a removed `zizmor` line is caught by [`super::run`]'s existing
//!   missing-output refusal only if the flake app goes with it. What is held here is the script's
//!   own call, and `crate::action_shell` holds the surface it cannot reach.
//! * **Not the recognised list's currency.** [`RECOGNISED`] is a copy of a third party's source at
//!   a named revision, so a tool `OpenSSF` adds later reads as absent here until the constant is
//!   updated - the safe direction, and the reason the revision is written beside it.

use std::collections::BTreeSet;
use std::path::Path;

use super::{Kind, Reference};

/// The record that accepts the zero, and therefore the file every refusal here points at.
const RECORD: &str = "docs/adr/0025-what-a-scorecard-zero-says-about-this-repository.md";

/// The flake check whose invocation the ADR cites.
const CHECK: &str = "clippy";

/// The argument that makes that check refuse rather than report.
const DENY: &str = "-D warnings";

/// The `flake.nix` attribute carrying it.
const ARGS: &str = "cargoClippyExtraArgs";

/// Where the workflows live. Read directly rather than through [`super::sources`], because a SAST
/// workflow does not run on a pull request - `scorecard.yml` deliberately does not - so the
/// pull-request closure that gate walks would not contain the thing this rule looks for.
const WORKFLOWS: &str = ".github/workflows";

/// The tools Scorecard's SAST check recognises in a `uses:` step.
///
/// Read from `ossf/scorecard`'s `checks/raw/sast.go` on 2026-09-08: the regexes are
/// `^github/codeql-action/analyze$`, `^snyk/actions/.*`, `^facebook/pysa-action$`,
/// `^JetBrains/qodana-action$` and `^hadolint/hadolint-action$`. Matched here as prefixes, which is
/// wider than the anchored forms and therefore fails closed - a fork or a subpath still trips it.
///
/// The check-run names in that same file - `github-advanced-security`, `github-code-scanning`,
/// `lgtm-com`, `sonarcloud`, `sonarqubecloud` - are NOT here and cannot be: they are check runs a
/// forge reports, not text in this tree.
const RECOGNISED: &[&str] = &[
    "github/codeql-action",
    "snyk/actions/",
    "facebook/pysa-action",
    "JetBrains/qodana-action",
    "hadolint/hadolint-action",
];

/// Every way the stand-in the ADR cites can have stopped being one.
pub(super) fn problems(root: &Path, flake: &str, references: &[Reference]) -> Vec<String> {
    let mut found = Vec::new();

    // The record first: refusals two and three below enforce a decision, and a decision nobody
    // wrote down is a rule with no reason rather than a rule with no mechanism.
    match std::fs::read_to_string(root.join(RECORD)) {
        Ok(text) if text.trim().is_empty() => found.push(format!(
            "{RECORD} is empty - the accepted SAST zero needs a dated reason, not a blank file"
        )),
        Err(error) => found.push(format!(
            "{RECORD} could not be read: {error} - that record is where accepting Scorecard's SAST zero is argued, and this gate enforces its claims"
        )),
        Ok(_) => {}
    }

    if !denies_warnings(flake) {
        found.push(format!(
            "flake.nix declares no `{ARGS}` carrying `{DENY}` - {RECORD} cites that flag as what stands in for a recognised SAST tool, and without it clippy reports its findings and exits 0"
        ));
    }

    if !invokes_check(references) {
        found.push(format!(
            "no workflow CI reaches builds `checks.<system>.{CHECK}` - {RECORD} cites it as running inside the required context, and a check nothing invokes measures nothing"
        ));
    }

    for tool in recognised_tools(root) {
        found.push(format!(
            "{tool} is a SAST tool Scorecard recognises, and {RECORD} still records the SAST zero as accepted because none is configured - the record is now wrong, so update it rather than this gate"
        ));
    }

    found
}

/// Whether `flake.nix` still passes [`DENY`] to clippy.
///
/// **Two projections of the same line, and the first draft of this used only one.**
/// [`super::code_lines`] BLANKS string literals - that is what `crate::devenv_shell` keys on to
/// tell a wrapped body from a bare one - and `-D warnings` lives inside the quotes. So the code
/// half answers *is this attribute assigned here, rather than discussed in a comment*, and the raw
/// line answers *does the value still carry the flag*. Reading either alone is wrong in a different
/// direction: the code half cannot see the flag at all, and the raw half passes on the commented-out
/// line that `a_comment_naming_the_flag_does_not_satisfy_it` writes.
///
/// The two are index-aligned because `code_lines` maps `str::lines` one to one, which is the same
/// correlation `super::scan_block` relies on for its own source capture.
fn denies_warnings(flake: &str) -> bool {
    let raw: Vec<&str> = flake.lines().collect();
    super::code_lines(flake)
        .into_iter()
        .enumerate()
        .any(|(number, line)| line.code.contains(ARGS) && raw.get(number).is_some_and(|text| text.contains(DENY)))
}

/// Whether the reference set CI was walked for contains the clippy check.
fn invokes_check(references: &[Reference]) -> bool {
    references
        .iter()
        .any(|reference| reference.kind == Kind::Check && reference.name == CHECK)
}

/// Every recognised SAST action a workflow declares, as `<file>:<line> <action>`.
///
/// Only `uses:` lines, and only outside a comment: `scorecard.yml` discusses what Scorecard reads
/// at length, and a gate that matched its prose would fail on the file that explains it.
fn recognised_tools(root: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let Ok(entries) = std::fs::read_dir(root.join(WORKFLOWS)) else {
        // A missing directory is not this rule's failure to report. `super::run` has already
        // refused an unreadable CI closure by the time this is called.
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
        if !extension.eq_ignore_ascii_case("yml") && !extension.eq_ignore_ascii_case("yaml") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                continue;
            }
            let Some(uses) = trimmed.strip_prefix("- uses:").or_else(|| trimmed.strip_prefix("uses:")) else {
                continue;
            };
            let action = uses.split_whitespace().next().unwrap_or_default();
            for tool in RECOGNISED {
                if action.starts_with(tool) {
                    found.insert(format!("{WORKFLOWS}/{name}:{} {action}", index.saturating_add(1)));
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::super::{Kind, Reference};
    use std::path::{Path, PathBuf};

    const GOOD_FLAKE: &str = "checks = {\n  clippy = craneLib.cargoClippy (ciArgs // {\n    cargoClippyExtraArgs = \"--workspace --all-targets --all-features -- -D warnings\";\n  });\n}\n";

    fn clippy_reference() -> Reference {
        Reference {
            workflow: String::from("ci.yml"),
            line: 321,
            kind: Kind::Check,
            name: String::from("clippy"),
        }
    }

    /// A synthetic root that satisfies every refusal, so a test can break exactly one.
    ///
    /// Named per test rather than shared: the workflow directory is written into, and two tests
    /// sharing one would pass or fail depending on which ran first.
    fn sound_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("sutura-sast-{}-{tag}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("a leftover temp tree is removable");
        }
        std::fs::create_dir_all(root.join(super::WORKFLOWS)).expect("temp workflows dir");
        let record = root.join(super::RECORD);
        std::fs::create_dir_all(record.parent().expect("the record sits in a directory")).expect("temp adr dir");
        std::fs::write(&record, "the decision, argued\n").expect("write the record");
        std::fs::write(
            root.join(super::WORKFLOWS).join("ci.yml"),
            "jobs:\n  ci:\n    steps:\n      - uses: actions/checkout@v7\n",
        )
        .expect("write a workflow");
        root
    }

    fn drop_root(root: &Path) {
        std::fs::remove_dir_all(root).expect("the temp tree this test created is removable");
    }

    /// THE PRODUCTION ENTRY POINT, against the real tree. Every refusal below breaks one input to
    /// this same call, so `&& false` on any of them reddens one of these tests rather than none -
    /// which is the difference between testing the predicate and testing the refusal.
    #[test]
    fn this_tree_satisfies_every_stand_in_rule() {
        let root = crate::repo::root().expect("repo root");
        let flake = std::fs::read_to_string(root.join("flake.nix")).expect("flake.nix is readable");
        let found = super::problems(&root, &flake, &[clippy_reference()]);
        assert!(
            found.is_empty(),
            "docs/adr/0025 accepts the SAST zero on these mechanisms and one is gone: {found:?}"
        );
    }

    /// The dead-check shape, and the reason this module exists: an attribute that still names
    /// clippy while the flag that makes it refuse is gone.
    #[test]
    fn clippy_without_deny_warnings_is_refused() {
        let root = sound_root("no-deny");
        let flake = "checks = {\n  clippy = craneLib.cargoClippy (ciArgs // {\n    cargoClippyExtraArgs = \"--workspace --all-targets --all-features\";\n  });\n}\n";
        let found = super::problems(&root, flake, &[clippy_reference()]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("cargoClippyExtraArgs"), "{found:?}");
        assert!(super::problems(&root, GOOD_FLAKE, &[clippy_reference()]).is_empty());
        drop_root(&root);
    }

    /// Prose describing the flag is not the flag, and this is the half a raw-text scan gets wrong.
    /// `Cargo.toml` and two xtask modules discuss `-D warnings` in comments.
    #[test]
    fn a_comment_naming_the_flag_does_not_satisfy_it() {
        let root = sound_root("comment");
        let flake = "checks = {\n  # cargoClippyExtraArgs was \"-- -D warnings\" here once.\n  clippy = craneLib.cargoClippy ciArgs;\n}\n";
        let found = super::problems(&root, flake, &[clippy_reference()]);
        assert_eq!(found.len(), 1, "a commented-out assignment is not an assignment: {found:?}");
        drop_root(&root);
    }

    /// A check nothing builds measures nothing, and the ADR cites this one as running.
    #[test]
    fn a_reference_set_without_the_clippy_check_is_refused() {
        let root = sound_root("no-ref");
        let found = super::problems(&root, GOOD_FLAKE, &[]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("clippy"), "{found:?}");

        // The right name in the wrong namespace. `nix run .#clippy` is an app; the ADR cites the
        // CHECK, which is what `ci.yml` builds.
        let app = Reference {
            kind: Kind::Runnable,
            ..clippy_reference()
        };
        assert_eq!(super::problems(&root, GOOD_FLAKE, &[app]).len(), 1);
        drop_root(&root);
    }

    /// The direction that keeps the record honest: adding `CodeQL` is a good change and it makes the
    /// ADR wrong, so the gate refuses the COMBINATION and names the file to edit.
    #[test]
    fn a_recognised_sast_tool_makes_the_record_stale() {
        let root = sound_root("codeql");
        std::fs::write(
            root.join(super::WORKFLOWS).join("codeql.yml"),
            "jobs:\n  analyze:\n    steps:\n      - uses: github/codeql-action/analyze@v3\n",
        )
        .expect("write");
        let found = super::problems(&root, GOOD_FLAKE, &[clippy_reference()]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("codeql.yml:4"), "{found:?}");
        assert!(found[0].contains(super::RECORD), "{found:?}");
        drop_root(&root);
    }

    /// `scorecard.yml` names Scorecard's own reads in prose, and a scan that matched a comment
    /// would fail on the file that documents the rule.
    #[test]
    fn a_commented_out_action_is_not_a_configured_tool() {
        let root = sound_root("prose");
        std::fs::write(
            root.join(super::WORKFLOWS).join("prose.yml"),
            "# uses: github/codeql-action/analyze@v3 was weighed and refused\njobs:\n  a:\n    steps:\n      - uses: actions/checkout@v7\n",
        )
        .expect("write");
        assert!(super::problems(&root, GOOD_FLAKE, &[clippy_reference()]).is_empty());
        drop_root(&root);
    }

    /// The refusals above enforce a decision, and a decision nobody wrote down is a rule with no
    /// reason. An EMPTY record fails too - `devco/scorecard-publication` learned that one file over.
    #[test]
    fn the_record_itself_has_to_exist_and_say_something() {
        let root = sound_root("record");
        let record = root.join(super::RECORD);

        std::fs::remove_file(&record).expect("remove the record");
        let missing = super::problems(&root, GOOD_FLAKE, &[clippy_reference()]);
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert!(missing[0].contains("could not be read"), "{missing:?}");

        std::fs::write(&record, "   \n\n").expect("blank the record");
        let blank = super::problems(&root, GOOD_FLAKE, &[clippy_reference()]);
        assert_eq!(blank.len(), 1, "{blank:?}");
        assert!(blank[0].contains("is empty"), "{blank:?}");
        drop_root(&root);
    }
}
