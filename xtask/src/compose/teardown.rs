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

use super::docker::Listing;

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
///
/// **A sum and not a struct, because one bit governs both halves.** Whether the runtime answered
/// decides what may be said about the target *and* about the neighbours, so recording it on one
/// field left the other free to overstate: an empty spared list read as "nothing else was running"
/// on a host that had said nothing, and an absent target read as "this worktree has no project
/// running" on the same host. Two variants also delete the two combinations that cannot occur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Plan {
    /// The runtime answered, so what exists is known.
    Considered {
        /// This worktree's project, where the listing contained it. `None` means it is not
        /// running - a state, not a failure.
        target: Option<String>,
        /// Everything the selection considered and left alone, in stable order. Empty means
        /// nothing else was running, which is a claim only an answer supports.
        spared: Vec<Spared>,
    },
    /// The runtime refused or never answered. This worktree's project is attempted anyway - the
    /// listing buys the ability to report, not the authority to remove - and nothing at all is
    /// reported about it or about a neighbour.
    Blind {
        /// This worktree's project, which is the only thing a destroy may ever target.
        target: String,
    },
}

impl Plan {
    /// What this destroy would remove, if anything.
    pub(crate) fn target(&self) -> Option<&str> {
        match *self {
            Self::Considered { ref target, .. } => target.as_deref(),
            Self::Blind { ref target } => Some(target),
        }
    }
}

/// Decide what to remove, given this worktree's project and what the runtime reported.
///
/// **Exactly one project can ever be the target**, and it is the one [`sutura_dev::scope::Scope::project`]
/// named - the same function start used. Everything else is spared with a reason, including another
/// worktree's `sutura-dev-` project: those are the ones a wildcard would have taken, and a wildcard
/// is what this function exists instead of.
///
/// **An unanswered listing still targets this worktree's project**, and that is the fail-closed
/// direction rather than an oversight. The listing buys the ability to report what was spared, not
/// the authority to remove - and the alternative was measured: a refused `compose ls` read as
/// "this worktree has no compose project running", so `dev-down` printed `ok` and exited zero with
/// its container still `Up (healthy)`. A `down` on a project that is not there is a no-op; a
/// destroy that reports success having removed nothing is the silent success rule 1 exists against.
pub(crate) fn plan(project: &str, reported: &Listing) -> Plan {
    let existing = match *reported {
        Listing::Answered(ref existing) => existing,
        Listing::Unknown => {
            return Plan::Blind {
                target: String::from(project),
            };
        }
    };
    Plan::Considered {
        target: existing.contains(project).then(|| String::from(project)),
        spared: existing
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
            .collect(),
    }
}

/// Is the plan still the plan?
///
/// Rule 2: **eligibility is re-checked at destroy time, under the lock.** The re-check is not a
/// second opinion about the same inputs - it is the same decision taken again against a listing
/// read *inside* the lock, because another worktree can start between the two moments. A target
/// that has stopped being this worktree's project is a refusal rather than a removal.
pub(crate) fn still_eligible(plan: &Plan, project: &str) -> bool {
    plan.target().is_none_or(|target| target == project)
}

/// The arguments that remove one project's containers, network and named volumes.
///
/// `--volumes` is the reason this is worth its own function: a named volume outlives a container,
/// so a destroy that skipped it leaves a stale data directory that the NEXT provision mounts. A
/// Postgres whose fixture came from another branch is a bug nobody can debug from the symptom.
pub(crate) fn down_args() -> Vec<&'static str> {
    vec!["down", "--volumes", "--remove-orphans"]
}
/// The arguments that remove named services while leaving other project services.
///
/// Deliberately no `--volumes`: Compose applies that flag to EVERY named volume declared by the
/// project, not merely to the named services. The selected volume is removed by its exact
/// project-scoped name after this command succeeds.
pub(crate) fn scoped_down_args<'a>(services: &'a [&'a str]) -> Option<Vec<&'a str>> {
    if services.is_empty() {
        return None;
    }
    let mut args = vec!["down"];
    args.extend_from_slice(services);
    Some(args)
}

/// The one sentence that names what removes this worktree's tier.
///
/// Here because this is the module that removes it, and shared because two places print it: a
/// provisioning call that was killed with containers running, and a readiness gate that failed
/// after `up --detach` had already returned. One wording, one place to change it - and NOT a
/// citation mechanism: `check-guidance`'s advice scan reads every production `.rs` line for a
/// backtick span starting `just `, so a renamed task fails on every copy rather than on one.
pub(in crate::compose) const REMOVES_THIS_WORKTREE: &str =
    "`just dev-down` removes this worktree's project, its network and its named volumes";

/// The scoped cleanup for a partial or failed exclusive demo lifecycle.
pub(in crate::compose) const REMOVES_DEMO: &str =
    "`just dev-down-demo` removes the demo container, its named volume and its discovery entry";

/// Print the plan. Called for a dry run and for a real one, so what a reader is shown before a
/// destroy is byte-identical to what they would have been shown by `--dry-run`.
pub(crate) fn describe(plan: &Plan) {
    // The two lines this replaces were both printed under `docker compose ls failed`, which is the
    // one host where neither can be true: `remove nothing - this worktree has no compose project
    // running`, and `spared nothing else was running`.
    let (target, spared) = match *plan {
        Plan::Blind { ref target } => {
            println!("  remove   {target} (attempting blind - the runtime did not say what exists)");
            println!("  spared   unknown - the runtime did not answer");
            return;
        }
        Plan::Considered { ref target, ref spared } => (target, spared),
    };
    match target.as_deref() {
        Some(project) => println!("  remove   {project} (containers, network, named volumes)"),
        None => println!("  remove   nothing - this worktree has no compose project running"),
    }
    if spared.is_empty() {
        println!("  spared   nothing else was running");
    } else {
        for spared in spared {
            println!("  spared   {} - {}", spared.project, spared.because);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Listing, Plan, Spared, down_args, plan, scoped_down_args, still_eligible};

    /// What the runtime answered, out of a list of project names. `&[]` is an answer of nothing,
    /// which is the case that has to stay distinguishable from no answer at all.
    fn answered(names: &[&str]) -> Listing {
        Listing::Answered(names.iter().map(|n| String::from(*n)).collect())
    }

    #[test]
    fn teardown_names_only_this_worktrees_project() {
        // The neighbour-killing case, asserted on the selection rather than by running docker and
        // looking at what is gone. Three other projects are present and every one of them survives.
        let mine = "sutura-dev-aaaa1111";
        let existing = answered(&[mine, "sutura-dev-bbbb2222", "sutura-dev-cccc3333", "someone-elses-app"]);

        let chosen = plan(mine, &existing);
        assert_eq!(chosen.target(), Some(mine));
        let Plan::Considered { ref spared, .. } = chosen else {
            panic!("an answered listing must produce a considered plan: {chosen:?}");
        };
        assert_eq!(spared.len(), 3, "{spared:?}");
        for spared in spared {
            assert_ne!(spared.project, mine);
        }
    }

    #[test]
    fn a_spared_neighbour_is_a_category_and_never_silence() {
        // Silence is indistinguishable from "there was nothing to consider", which is exactly the
        // case where a reader needs to know the check fired.
        let existing = answered(&["sutura-dev-aaaa1111", "sutura-dev-bbbb2222"]);
        let chosen = plan("sutura-dev-aaaa1111", &existing);
        assert_eq!(
            chosen,
            Plan::Considered {
                target: Some(String::from("sutura-dev-aaaa1111")),
                spared: vec![Spared {
                    project: String::from("sutura-dev-bbbb2222"),
                    because: "another worktree's - in use, left alone",
                }]
            }
        );
    }

    #[test]
    fn nothing_running_is_a_state_rather_than_a_failure() {
        let chosen = plan("sutura-dev-aaaa1111", &answered(&[]));
        assert_eq!(
            chosen,
            Plan::Considered {
                target: None,
                spared: Vec::new()
            }
        );
    }

    #[test]
    fn a_target_that_stopped_being_ours_is_no_longer_eligible() {
        // The re-check under the lock. If the project name a destroy is about to pass to docker is
        // not the one this worktree derives NOW, the removal targets somebody else.
        let chosen = plan("sutura-dev-aaaa1111", &answered(&["sutura-dev-aaaa1111"]));
        assert!(still_eligible(&chosen, "sutura-dev-aaaa1111"));
        assert!(!still_eligible(&chosen, "sutura-dev-bbbb2222"));
        // Nothing to remove is always eligible: there is no wrong thing to take.
        let empty = plan("sutura-dev-aaaa1111", &answered(&[]));
        assert!(still_eligible(&empty, "sutura-dev-bbbb2222"));
    }

    #[test]
    fn a_runtime_that_did_not_answer_spares_nothing_and_still_targets_this_worktree() {
        // Both halves of the same conflation, and both were measured on a host with a refused
        // `compose ls`: the destroy printed `spared nothing else was running` two lines under the
        // failure report, and it printed `remove nothing` and `ok` with its own container still
        // `Up (healthy)`. An empty answer supports the first sentence; no answer supports neither.
        let chosen = plan("sutura-dev-aaaa1111", &Listing::Unknown);
        assert_eq!(
            chosen,
            Plan::Blind {
                target: String::from("sutura-dev-aaaa1111")
            },
            "a host that did not answer must not be reported as a host with nothing running, and \
             the listing buys the report rather than the authority to remove - a destroy that \
             cannot see the project must still attempt it instead of reporting success having \
             done nothing"
        );
        // Distinguishable from an answer of nothing, which is the whole point: that one removes
        // nothing and says so, and this one attempts the removal and claims nothing.
        assert_ne!(chosen, plan("sutura-dev-aaaa1111", &answered(&[])));
        // And the re-check under the lock still passes on it: two unanswered reads agree, so a
        // destroy is not turned into a refusal by a runtime that is silent throughout.
        assert!(still_eligible(&chosen, "sutura-dev-aaaa1111"));
        assert!(!still_eligible(&chosen, "sutura-dev-bbbb2222"));
    }

    #[test]
    fn a_destroy_removes_the_named_volumes_too() {
        // A named volume outlives its container, so a destroy that skipped it leaves a data
        // directory the next provision mounts - a fixture from another branch, with no symptom
        // pointing at it.
        assert!(down_args().contains(&"--volumes"));
        assert!(down_args().contains(&"--remove-orphans"));
    }

    #[test]
    fn a_scoped_destroy_names_a_service_and_never_requests_every_volume() {
        let args = scoped_down_args(&["demo"]).expect("one service is a scoped destroy");
        assert_eq!(args, vec!["down", "demo"]);
        assert!(!args.contains(&"--volumes"));
        assert!(
            scoped_down_args(&[]).is_none(),
            "an empty selection would mean the whole project"
        );
    }
}
