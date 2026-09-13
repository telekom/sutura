//! Whether this worktree's compose project holds resources - answered by EXIT CODE.
//!
//! **The signal that was missing.** The only way to ask was prose: a caller ran the teardown's dry
//! run and matched the sentence it printed, because every outcome of that task - a project
//! running, none running, a runtime that never answered, a runtime that is not installed - exits
//! 0. `dev-endpoints` cannot answer either: it reads the discovery file rather than the runtime, so
//! a tier removed by hand still reads as present and a tier provisioned before the file was deleted
//! reads as absent.
//!
//! **THE POLARITY IS THE DESIGN, and the task name follows it.** `dev-clear` asks *is this worktree
//! clear of compose resources*, so **0 means safe to proceed** and every other answer refuses. The
//! inverse spelling - a task that exits 0 when a project IS running - forces its caller to write
//! `! just <task>`, and `!` turns the unknown code into a success: the one answer that must never
//! read as "nothing there" would be the one that does. A caller writes `just dev-clear || refuse`
//! and one `||` covers both refusal codes.
//!
//! This removes nothing, so it takes no teardown lock. It does not make teardown narrower either -
//! compose's granularity is the project and the profile, and that is upstream. It only lets a
//! caller DECIDE correctly before arming a destructive trap.

use super::teardown::Plan;
use super::{docker, scope_here, teardown};
use crate::Verdict;

/// The answer to a question that could not be asked: refuse.
///
/// A function rather than three inline `return`s so the polarity is ASSERTED rather than reviewed.
/// Every path that did not get an answer from the runtime routes through here, and
/// `tests::an_unanswered_question_never_reads_as_clear` is what stops a future edit making one of
/// them exit 0.
fn unknown(why: &str) -> Verdict {
    eprintln!("xtask dev-clear: UNKNOWN - {why}");
    eprintln!("  Unknown is not empty: a caller arming a teardown on this answer would remove");
    eprintln!("  whatever is actually there. Refusing instead.");
    Verdict::Inconclusive
}

/// The three states a caller cares about, as verdicts.
///
/// Pure, and over [`Plan`] rather than over a listing, because [`teardown::plan`] already decides
/// what belongs to this worktree and what is a neighbour's. Asking a second time in a second place
/// is how the two answers come to disagree.
fn verdict(plan: &Plan) -> Verdict {
    match *plan {
        // A neighbour's project is not this worktree's to lose, so it is CLEAR here - the spared
        // inventory is printed for a caller that needs to know what else is up.
        Plan::Considered { target: None, .. } => Verdict::Pass,
        Plan::Considered { target: Some(_), .. } => Verdict::Fail,
        Plan::Blind { .. } => unknown("the runtime did not say what exists"),
    }
}

/// `dev-clear` - is this worktree clear of compose resources?
///
/// **Fail closed, which is the whole difference from `dev-down`.** That task reports an absent
/// runtime through `Requirement::from_env`, which SKIPS green on a machine without docker - correct
/// for a teardown that has nothing to do, and wrong for a question whose answer decides whether
/// something destructive may run. A runtime that is not installed, a daemon that is stopped and a
/// daemon that never answers are all *unknown* here, never *clear*.
pub(crate) fn run(_args: &[String]) -> Verdict {
    let Ok(scope) = scope_here() else {
        return unknown("could not determine this worktree's compose project");
    };
    let root = scope.root().to_path_buf();
    if let Err(missing) = docker::presence() {
        eprintln!("  {}", missing.remedy());
        return unknown(&format!("no container runtime ({missing:?})"));
    }
    let project = scope.project();
    let plan = teardown::plan(&project, &docker::projects(&root));
    println!("xtask dev-clear: {} in {}", project, root.display());
    // The inventory, in the teardown's own words, so the exit code and the prose cannot disagree.
    teardown::describe(&plan);
    verdict(&plan)
}

#[cfg(test)]
mod tests {
    use super::{Plan, Verdict, unknown, verdict};
    use crate::compose::teardown::Spared;

    fn spared(project: &str) -> Spared {
        Spared {
            project: String::from(project),
            because: "not this worktree's",
        }
    }

    #[test]
    fn a_worktree_with_no_project_is_clear() {
        let plan = Plan::Considered {
            target: None,
            spared: Vec::new(),
        };
        assert_eq!(verdict(&plan), Verdict::Pass);
    }

    #[test]
    fn a_neighbours_project_is_not_this_worktree_s_to_lose() {
        let plan = Plan::Considered {
            target: None,
            spared: vec![spared("sutura-dev-beef1234")],
        };
        assert_eq!(
            verdict(&plan),
            Verdict::Pass,
            "another worktree's project is reported, not counted against this one"
        );
    }

    #[test]
    fn a_project_this_worktree_owns_refuses() {
        let plan = Plan::Considered {
            target: Some(String::from("sutura-dev-0badcafe")),
            spared: Vec::new(),
        };
        assert_eq!(verdict(&plan), Verdict::Fail);
    }

    /// The fail-closed property, over EVERY answer that is not an answer.
    ///
    /// `Blind` is the runtime refusing or never replying; `unknown` is also what an unresolvable
    /// scope and an absent runtime return in [`super::run`]. Asserted as *not `Pass`* rather than
    /// as a particular code, because what must never happen is a caller reading any of them as
    /// permission to proceed.
    #[test]
    fn an_unanswered_question_never_reads_as_clear() {
        let blind = Plan::Blind {
            target: String::from("sutura-dev-0badcafe"),
        };
        assert_ne!(verdict(&blind), Verdict::Pass, "a runtime that did not answer must refuse");
        assert_ne!(unknown("planted"), Verdict::Pass, "every unanswered path must refuse");
        // And the three states are distinguishable, which is what makes the signal usable by a
        // caller that has to tell "nothing there" from "I could not look".
        let owned = Plan::Considered {
            target: Some(String::from("sutura-dev-0badcafe")),
            spared: Vec::new(),
        };
        let none = Plan::Considered {
            target: None,
            spared: Vec::new(),
        };
        assert_ne!(verdict(&owned), verdict(&blind), "held and unknown must not share a code");
        assert_ne!(verdict(&none), verdict(&owned));
        assert_ne!(verdict(&none), verdict(&blind));
    }
}
