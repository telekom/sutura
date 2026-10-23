//! Does `security-audit.yml` run on every change the classifier calls supply-chain?
//!
//! `cargo xtask classify` selects the `deps` area for a change to one of `DEPS_PATTERNS`, and CI's
//! `Supply chain` step reads that area. `security-audit.yml` is a second reader of the same question
//! with its own `paths:` lists under `push` and `pull_request`, and nothing related the two: dropping
//! the vendored driver's lock from both lists kept every gate green, so a change to only that lock
//! skipped the audit. Every pattern the classifier reads must be listed under both triggers.
//!
//! **What this does not reach:** a line scan of the quoted `- "path"` entries under each trigger's
//! `paths:`. A path that is quoted differently, or listed under a trigger by an anchor, reads as
//! absent and is refused rather than accepted; a path listed for a trigger that no longer fires
//! the job (a changed `branches:` filter) still counts.

use std::collections::BTreeSet;
use std::path::Path;

use crate::changes::DEPS_PATTERNS;

const WORKFLOW: &str = ".github/workflows/security-audit.yml";
const TRIGGERS: [&str; 2] = ["push", "pull_request"];

/// Every classifier pattern a trigger's `paths:` list lacks, over one repository root.
pub(super) fn problems(root: &Path) -> Vec<String> {
    match std::fs::read_to_string(root.join(WORKFLOW)) {
        Ok(text) => judge(&text, &DEPS_PATTERNS),
        Err(error) => vec![format!("{WORKFLOW} could not be read: {error}")],
    }
}

fn judge(text: &str, patterns: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    for trigger in TRIGGERS {
        let listed = listed_under(text, trigger);
        for pattern in patterns {
            if !listed.contains(*pattern) {
                found.push(format!(
                    "{WORKFLOW} lists no `{pattern}` under `{trigger}.paths`, which the classifier's `deps` area reads"
                ));
            }
        }
    }
    found
}

/// The quoted entries of `on.<trigger>.paths`.
fn listed_under(text: &str, trigger: &str) -> BTreeSet<String> {
    let mut listed = BTreeSet::new();
    let (mut in_trigger, mut in_paths) = (false, false);
    for line in text.lines().filter(|line| !line.trim_start().starts_with('#')) {
        let indent = line.len() - line.trim_start().len();
        let content = line.trim();
        if content.is_empty() {
            continue;
        }
        if indent == 2 {
            in_trigger = content.strip_suffix(':').is_some_and(|key| key == trigger);
            in_paths = false;
        } else if in_trigger && indent == 4 {
            in_paths = content == "paths:";
        } else if in_trigger
            && in_paths
            && indent >= 6
            && let Some(entry) = content.strip_prefix("- ")
        {
            listed.insert(entry.trim().trim_matches('"').to_owned());
        }
    }
    listed
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORKFLOW_TEXT: &str = "on:\n  push:\n    branches: [main]\n    paths:\n      - \"Cargo.lock\"\n      - \"nix/clickhouse-adbc.Cargo.lock\"\n  pull_request:\n    paths:\n      - \"Cargo.lock\"\n      - \"nix/clickhouse-adbc.Cargo.lock\"\n  schedule:\n    - cron: \"17 6 * * 1\"\n";

    #[test]
    fn the_committed_workflow_lists_every_pattern_the_classifier_reads() {
        let Some(root) = crate::repo::root() else { return };
        assert_eq!(problems(&root), Vec::<String>::new());
    }

    #[test]
    fn a_lock_listed_under_both_triggers_is_accepted() {
        assert_eq!(
            judge(WORKFLOW_TEXT, &["Cargo.lock", "nix/clickhouse-adbc.Cargo.lock"]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_lock_dropped_from_one_trigger_is_refused_by_name_and_trigger() {
        let dropped = WORKFLOW_TEXT.replacen("      - \"nix/clickhouse-adbc.Cargo.lock\"\n", "", 1);
        let found = judge(&dropped, &["Cargo.lock", "nix/clickhouse-adbc.Cargo.lock"]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].contains("push.paths") && found[0].contains("nix/clickhouse-adbc.Cargo.lock"),
            "{found:?}"
        );
    }

    #[test]
    fn a_path_that_is_only_commented_out_is_not_listed() {
        let commented = WORKFLOW_TEXT.replace(
            "      - \"nix/clickhouse-adbc.Cargo.lock\"",
            "      # - \"nix/clickhouse-adbc.Cargo.lock\"",
        );
        assert_eq!(judge(&commented, &["nix/clickhouse-adbc.Cargo.lock"]).len(), 2);
    }
}
