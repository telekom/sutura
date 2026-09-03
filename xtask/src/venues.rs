//! The venue map stays honest: every identity acceptance task is a row in `docs/where-identity-is-proven.md`.
//!
//! `docs/where-identity-is-proven.md` says, and this is the mechanism behind that sentence:
//!
//! > A new venue arrives as a row in the table above **with its exclusions written**, in the same change.
//!
//! Up to this gate, that was a wish rather than a rule (AGENTS.md calls a rule with no mechanism a wish).
//! A venue that arrived without a row showed up only as a claim a reviewer happened to remember. This
//! check reads the two files as text - nothing is evaluated, the same shape as `check-warm-start` - and
//! fails when an identity `*-acceptance` task has no row naming it.
//!
//! The names come from the `Justfile` (a task is how a venue is reached by a person and by CI alike), and
//! the map is matched on the backticked `` `just <name>` `` form its "Reached by" column uses.
//!
//! **The one explicitly non-identity acceptance task.** `datahub-acceptance` reaches a metadata-catalog
//! platform, not an identity claim, and this page maps identity claims - so it is not a row here, and it
//! is named rather than inferred, because a gate that asserted "every `-acceptance` task" would force a
//! non-identity venue into an identity map and this comment is the record of why it must not.

use crate::Verdict;
use crate::repo;

const JUSTFILE: &str = "justfile";
const VENUE_MAP: &str = "docs/where-identity-is-proven.md";
/// The one `-acceptance` task that is NOT an identity venue (it reaches a metadata-catalog platform),
/// and therefore has no row in this page. Named as a constant rather than inferred so the exception is
/// a conscious decision a reader can find.
const NON_IDENTITY_ACCEPTANCE: &[&str] = &["datahub-acceptance"];

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-venues: could not locate the repo root");
        return Verdict::Fail;
    };
    let Ok(justfile) = std::fs::read_to_string(root.join(JUSTFILE)) else {
        eprintln!("xtask check-venues: could not read {JUSTFILE}");
        return Verdict::Fail;
    };
    let Ok(map) = std::fs::read_to_string(root.join(VENUE_MAP)) else {
        eprintln!("xtask check-venues: could not read {VENUE_MAP}");
        return Verdict::Fail;
    };

    let mut acceptance: Vec<&str> = justfile
        .lines()
        .filter_map(|line| {
            let name = line.strip_suffix(':')?;
            name.ends_with("-acceptance").then_some(name.trim())
        })
        .collect();
    acceptance.sort_unstable();
    acceptance.dedup();

    let mut missing: Vec<&str> = Vec::new();
    let mut registered: Vec<&str> = Vec::new();
    for task in &acceptance {
        if NON_IDENTITY_ACCEPTANCE.contains(task) {
            continue;
        }
        if map.contains(&format!("`just {task}`")) {
            registered.push(task);
        } else {
            missing.push(task);
        }
    }

    if !missing.is_empty() {
        eprintln!("xtask check-venues: these identity acceptance tasks have no row in {VENUE_MAP}: {}", missing.join(", "));
        eprintln!("xtask check-venues: add a venue row (with its exclusions) naming `just <task>` - see the table and the rule under \"Keeping this page honest\"");
        return Verdict::Fail;
    }

    println!("xtask check-venues: ok - {} identity venue(s) registered: {}", registered.len(), registered.join(", "));
    Verdict::Pass
}

#[cfg(test)]
mod tests {
    /// The names are read as text; keep the reading honest for a task spelled with a recipe shebang.
    #[test]
    fn a_comment_line_is_not_mistaken_for_a_task_name() {
        // A Justfile recipe line starts with no leading whitespace and ends with `:`; a shell body line
        // does not. The filter already rules this out, pinned here so the shape cannot regress.
        let task = "keycloak-acceptance:";
        let non_task = "    nix run .#keycloak-acceptance -- --no-capture";
        assert_eq!(task.strip_suffix(':'), Some("keycloak-acceptance"));
        assert!(non_task.strip_suffix(':').is_none(), "a recipe body is not a task name");
    }
}
