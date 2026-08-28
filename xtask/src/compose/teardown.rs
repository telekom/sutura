//! What a destroy would remove, and what it deliberately leaves alone.
//!
//! The selection is the part that goes wrong, so it is a pure function over a project listing and
//! this worktree's own project name - which is what makes the neighbour-killing case assertable on
//! the plan rather than by running docker and looking at the damage.
//!
//! Rule 1 of the teardown contract in `sutura_dev::scope`: **destructive cleanup is dry-runnable.**
//! A destroy whose only mode is "do it" is a destroy nobody can review. So [`plan`] produces a
//! value, printing it is one function, and executing it is another.
//!
//! Rule 2's second half: **what is deliberately spared is reported as its own category.** A
//! neighbour's project appears in the plan as [`Spared`], never as silence - silence there is
//! indistinguishable from "there was nothing to consider", which is precisely the case where a
//! reader needs to know the safety mechanism fired.

use std::collections::BTreeSet;

/// The prefix every project this tool creates carries, so a stray one is identifiable as ours and
/// anything else is visibly not.
const OURS: &str = "sutura-dev-";

/// A project this destroy will not touch, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Spared {
    /// The compose project name.
    pub(crate) project: String,
    /// Why it survived the selection. Printed; a spared item is a success of the check and has to
    /// read as one.
    pub(crate) because: &'static str,
}

/// What a destroy would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Plan {
    /// This worktree's project, if the runtime has one. `None` means there is nothing to remove -
    /// which is a state, not a failure.
    pub(crate) target: Option<String>,
    /// Everything the selection considered and left alone, in stable order.
    pub(crate) spared: Vec<Spared>,
}

/// Decide what to remove, given this worktree's project and every project the runtime reports.
///
/// **Exactly one project can ever be the target**, and it is the one [`sutura_dev::scope::Scope::project`]
/// named - the same function start used. Everything else is spared with a reason, including another
/// worktree's `sutura-dev-` project: those are the ones a wildcard would have taken, and a wildcard
/// is what this function exists instead of.
pub(crate) fn plan(project: &str, existing: &BTreeSet<String>) -> Plan {
    let target = existing.contains(project).then(|| String::from(project));
    let spared = existing
        .iter()
        .filter(|name| name.as_str() != project)
        .map(|name| Spared {
            project: name.clone(),
            because: if name.starts_with(OURS) {
                "another worktree's - in use, left alone"
            } else {
                "not this tool's project"
            },
        })
        .collect();
    Plan { target, spared }
}

/// Is the plan still the plan?
///
/// Rule 2: **eligibility is re-checked at destroy time, under the lock.** The re-check is not a
/// second opinion about the same inputs - it is the same decision taken again against a listing
/// read *inside* the lock, because another worktree can start between the two moments. A target
/// that has stopped being this worktree's project is a refusal rather than a removal.
pub(crate) fn still_eligible(plan: &Plan, project: &str) -> bool {
    plan.target.as_deref().is_none_or(|target| target == project)
}

/// The arguments that remove one project's containers, network and named volumes.
///
/// `--volumes` is the reason this is worth its own function: a named volume outlives a container,
/// so a destroy that skipped it leaves a stale data directory that the NEXT provision mounts. A
/// Postgres whose fixture came from another branch is a bug nobody can debug from the symptom.
pub(crate) fn down_args() -> Vec<&'static str> {
    vec!["down", "--volumes", "--remove-orphans"]
}

/// Print the plan. Called for a dry run and for a real one, so what a reader is shown before a
/// destroy is byte-identical to what they would have been shown by `--dry-run`.
pub(crate) fn describe(plan: &Plan) {
    match plan.target.as_deref() {
        Some(project) => println!("  remove   {project} (containers, network, named volumes)"),
        None => println!("  remove   nothing - this worktree has no compose project running"),
    }
    if plan.spared.is_empty() {
        println!("  spared   nothing else was running");
    } else {
        for spared in &plan.spared {
            println!("  spared   {} - {}", spared.project, spared.because);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{Plan, Spared, down_args, plan, still_eligible};

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| String::from(*n)).collect()
    }

    #[test]
    fn teardown_names_only_this_worktrees_project() {
        // The neighbour-killing case, asserted on the selection rather than by running docker and
        // looking at what is gone. Three other projects are present and every one of them survives.
        let mine = "sutura-dev-aaaa1111";
        let existing = set(&[mine, "sutura-dev-bbbb2222", "sutura-dev-cccc3333", "someone-elses-app"]);

        let chosen = plan(mine, &existing);
        assert_eq!(chosen.target.as_deref(), Some(mine));
        assert_eq!(chosen.spared.len(), 3, "{:?}", chosen.spared);
        for spared in &chosen.spared {
            assert_ne!(spared.project, mine);
        }
    }

    #[test]
    fn a_spared_neighbour_is_a_category_and_never_silence() {
        // Silence is indistinguishable from "there was nothing to consider", which is exactly the
        // case where a reader needs to know the check fired.
        let existing = set(&["sutura-dev-aaaa1111", "sutura-dev-bbbb2222"]);
        let chosen = plan("sutura-dev-aaaa1111", &existing);
        assert_eq!(
            chosen.spared,
            vec![Spared {
                project: String::from("sutura-dev-bbbb2222"),
                because: "another worktree's - in use, left alone",
            }]
        );
    }

    #[test]
    fn nothing_running_is_a_state_rather_than_a_failure() {
        let chosen = plan("sutura-dev-aaaa1111", &BTreeSet::new());
        assert_eq!(
            chosen,
            Plan {
                target: None,
                spared: Vec::new()
            }
        );
    }

    #[test]
    fn a_target_that_stopped_being_ours_is_no_longer_eligible() {
        // The re-check under the lock. If the project name a destroy is about to pass to docker is
        // not the one this worktree derives NOW, the removal targets somebody else.
        let chosen = plan("sutura-dev-aaaa1111", &set(&["sutura-dev-aaaa1111"]));
        assert!(still_eligible(&chosen, "sutura-dev-aaaa1111"));
        assert!(!still_eligible(&chosen, "sutura-dev-bbbb2222"));
        // Nothing to remove is always eligible: there is no wrong thing to take.
        let empty = plan("sutura-dev-aaaa1111", &BTreeSet::new());
        assert!(still_eligible(&empty, "sutura-dev-bbbb2222"));
    }

    #[test]
    fn a_destroy_removes_the_named_volumes_too() {
        // A named volume outlives its container, so a destroy that skipped it leaves a data
        // directory the next provision mounts - a fixture from another branch, with no symptom
        // pointing at it.
        assert!(down_args().contains(&"--volumes"));
        assert!(down_args().contains(&"--remove-orphans"));
    }
}
