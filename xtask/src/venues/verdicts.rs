//! What a VERDICT cell has to be consistent with, once `page` has classified it and `rows` has
//! established that the row it sits over can be judged at all.
//!
//! The fourth seam `venues.rs` has been split on, and the same 1000-line gate forced it: the
//! anchor rule below took the file over the unexemptable limit, and nothing under `xtask/` may be
//! listed in `devco/max-lines-ignore`. Split HERE because these two functions are the only ones
//! there that read a verdict against the TREE - the invocation set and the test-task set - rather
//! than against the page.
//!
//! # Why each of these rules exists, and the limit each does not reach
//!
//! * **A test that is WRITTEN and has never run says `unrun`, and its venue's section has to use
//!   that word.** The verdict exists because `can` was carrying two states that a reader cannot
//!   tell apart and a citation must: *the standing test lives in another venue* and *the standing
//!   test lives here and nothing has run it*. The second is not evidence for anything, and the
//!   first is. Before this, a venue in that state had to say `can` and explain itself in prose -
//!   which put the difference exactly where nothing reads it.
//! * **A venue CI INVOKES may not say `unrun`**, which is the half review found missing and the
//!   reason [`super::sources::invoked`] exists. The three page-shaped rules above hold a spelling; not one of them
//!   reads whether a run has happened, so the transition out of `unrun` was still held by recall -
//!   and the concrete failure was cheap to reach: wire the leg into a job, watch it go green on
//!   every push, and the page goes on telling its next reader that nothing has run it while
//!   `just validate` stays green. So the venue's `Reached by` task is now resolved against every
//!   `just <task>` and `nix run .#<app>` the workflows, the local composite actions and the shared
//!   `nix/` shell invoke - the same three places `crate::workflows` scans, through the one walk
//!   both gates share, for the reason `crate::workflows::sources` gives - and an `unrun` cell
//!   whose venue is among them is refused.
//!
//!   **The limit, stated where the claim is:** what this reads is an INVOCATION, not a green run.
//!   A wired job that always skips still reddens the cell, and a run somebody did by hand is
//!   invisible to it - so *`unrun` is no longer honest* is mechanical, and *`yes` is earned* stays
//!   review's, with the run named beside it. That is one half of a two-sided rule, and it is the
//!   half whose failure was silent.
//! * **A venue CI DOES invoke, whose run nobody has seen, says `wired`** - the state the rule above
//!   left with nowhere to go. `unrun` and `yes` were the only two ends of that transition, and the
//!   change that wires a leg into a job is not the change that can produce its first green run: the
//!   run happens after the push. So a correct wiring had exactly two moves, one refused by the rule
//!   above and one an overstatement, which is a gate that can only be satisfied by a lie. Measured
//!   on telekom/sutura#287: wiring `nix run .#bigquery-two-principals` into `ci.yml` reddens
//!   `check-venues`, and the leg cannot be green because the five values it is pointed at do not
//!   exist yet. `wired` says *something reaches this and no run has been observed*, and it is
//!   **not citable** - it counts exactly as `unrun` does towards a row having a reason to exist,
//!   and exactly as `unrun` does towards a claim being answered, which is not at all.
//!
//!   **Its two rules are `unrun`'s pointing the other way**, so the pair cannot both be satisfied
//!   and neither can be satisfied vacuously: the section has to use the word, and a `wired` cell
//!   whose venue CI does **not** invoke is refused - because the honest state for a venue nothing
//!   reaches is `unrun`, and a `wired` that nothing reaches would be the softer spelling this whole
//!   vocabulary exists to refuse.
//!
//! **The split moved no assertion**, which is what `.agents/skills/sutura/gates` prescribes: the
//! causality gate never reverts a file that adds a `#[test]`, so moving tests out of `venues.rs`
//! would orphan them and read as green-against-base. Every `#[test]` stayed there; only these two
//! readers moved.

use std::collections::BTreeSet;

use super::PAGE;
use super::page::{ON_DEMAND, Venue, section_body};
use super::sources::cited_invocations;

/// Everything the two un-citable verdicts have to be consistent with, for the venues stating them.
///
/// Together rather than beside the other page rules, because these are the whole difference
/// between `unrun`/`wired` being tokens and being caveats, and they fail in opposite directions:
/// one catches a cell whose section never explains it, the others a cell that has stopped being
/// true - in either direction, since `unrun` expires when CI reaches a venue and `wired` is not yet
/// earned until it does.
pub(super) fn transition_problems(
    text: &str,
    listed: &[Venue],
    unrun: &BTreeSet<&str>,
    wired: &BTreeSet<&str>,
    invoked: &BTreeSet<String>,
) -> Vec<String> {
    let mut problems = Vec::new();
    // The shared half: a cell stating one of these has to say the same word in its own SECTION.
    // One loop over both, because the argument is identical and two copies of it would be two
    // places for the wording to drift - which is the failure this whole module is about.
    for (word, venues) in [("unrun", unrun), ("wired", wired)] {
        for venue in listed.iter().filter(|venue| venues.contains(venue.name.as_str())) {
            if !section_body(text, &venue.name).is_some_and(|body| body.contains(word)) {
                problems.push(format!(
                    "{PAGE}: `{}` says `{word}` in the matrix and its own section never uses that \
                     word - the cell is where the state is read and the section is where it is \
                     explained, and a section that explains it in other words is how the two came \
                     apart",
                    venue.name
                ));
            }
        }
    }
    for venue in listed.iter().filter(|venue| unrun.contains(venue.name.as_str())) {
        // **A venue CI INVOKES may not say `unrun`**, which is the half the page-shaped rules do
        // not hold. They read a spelling; none of them reads whether a run has happened, so a leg
        // wired into a job could go green on every push while this page went on saying nothing had
        // run it - green `just validate` included. The `Reached by` cell is the venue's own answer
        // to *what reaches me*, so it is what gets resolved.
        //
        // THE LIMIT, and it is why this is stated here as well as in the header: an INVOCATION is
        // not a green run. A wired job that always skips reddens this too, and a hand-run is
        // invisible to it. What is mechanical is that `unrun` stops being honest; that `yes` is
        // EARNED stays review's.
        let reached_by_ci: BTreeSet<String> = cited_invocations(&venue.reached)
            .into_iter()
            .filter(|name| invoked.contains(name))
            .collect();
        if !reached_by_ci.is_empty() {
            problems.push(format!(
                "{PAGE}: `{}` says `unrun` and CI invokes {reached_by_ci:?} - `unrun` is a test \
                 that EXISTS and has not been run, and a venue a job reaches is no longer in that \
                 state. Either the run has happened and the cell is `yes` with the run named in \
                 the section, or nobody has seen one yet and the cell is `wired`, or the job does \
                 not reach this venue and its `Reached by` cell ({}) is not what reaches it. What \
                 is read here is the INVOCATION and not a green run: a job that always skips \
                 reddens this too, and a hand-run is invisible to it",
                venue.name, venue.reached
            ));
        }
    }
    // And the mirror. `wired` says *CI reaches this and no run has been observed*, so a cell
    // stating it over a venue no job invokes is claiming a wiring that is not there - which would
    // make `wired` a spelling of `unrun` that skips `unrun`'s own expiry rule.
    for venue in listed.iter().filter(|venue| wired.contains(venue.name.as_str())) {
        // A job reaches it, so it does not run nowhere. Two cells of one row contradicting each
        // other is the shape the `yes`/`can` guard above was found missing.
        if venue.runs_nowhere() {
            problems.push(format!(
                "{PAGE}: `{}` says `wired` and its own `Where it runs` cell says `{}` - `wired` is \
                 *a job reaches this*, so the two cells of this row contradict each other. A venue \
                 that runs nowhere and has a written test is `unrun`",
                venue.name, venue.runs
            ));
        }
        let reached_by_ci: BTreeSet<String> = cited_invocations(&venue.reached)
            .into_iter()
            .filter(|name| invoked.contains(name))
            .collect();
        if reached_by_ci.is_empty() {
            problems.push(format!(
                "{PAGE}: `{}` says `wired` and CI invokes nothing its `Reached by` cell ({}) \
                 names - `wired` is *a job reaches this and no run has been observed*, so a venue \
                 no job reaches is `unrun` instead. Either the wiring is missing from the \
                 workflows or the `Reached by` cell does not name what reaches it",
                venue.name, venue.reached
            ));
        }
    }
    problems
}

/// Every citable or wired venue whose citation is anchored in nothing the tree runs a test with.
///
/// **The gap this closes, and it was the last one on the page a false leg-2 `yes` fits through.**
/// `Reached by` earning a verdict meant *this cell names a task*, and for `wired` also *CI invokes
/// that task* - and CI invokes lints, builds, docs and release jobs. Both halves were recorded on
/// the page as review's, with the measurement beside them: pointing the two-keys row at a real,
/// CI-invoked, entirely unrelated lint and moving its cell passed at exit 0, and a `yes` with no
/// run ever performed passed at exit 0 because only `unrun` and `wired` read the invocation set.
///
/// **Two rules, and the SECOND one is conditioned on the run site**, which is what makes that
/// closed vocabulary do mechanical work rather than only be spell-checked:
///
/// 1. Every citable or `wired` verdict needs a cited task that RUNS TESTS. A task that runs no
///    test is not what reaches a venue whose venue-hood is a test.
/// 2. A venue that runs in [`ON_DEMAND`] additionally needs CI to invoke that task, because *on
///    demand* means a job is the only thing that can demand a run there.
///
/// **Why rule 2 is not universal, measured rather than assumed.** Requiring it everywhere refused
/// the two `in process` venues, whose claims are answered on every push: this workspace's suite
/// runs in CI as a nix CHECK, and [`super::sources::invoked`] resolves `just <task>` and
/// `nix run .#<app>` - so `just test` is genuinely run by CI and is invisible to that reader. A
/// gate that reddens correct work gets disabled, and the run-site token already states the
/// difference the rule needs: *in process, every run* against *a GitHub environment, on demand*.
///
/// **What that leaves, said where the claim is.** For an `in process` venue the anchor is rule 1
/// alone, so what holds `yes` there is that the suite really is the venue - which its own token
/// asserts and nothing here proves. And for every venue, what is held is that the named task runs
/// tests, never that it runs THIS venue's tests: repointing a row at another venue's test task
/// still passes.
pub(super) fn anchor_problems(
    listed: &[Venue],
    citable: &BTreeSet<&str>,
    invoked: &BTreeSet<String>,
    runs_tests: &BTreeSet<String>,
) -> Vec<String> {
    let stating = || listed.iter().filter(|venue| citable.contains(venue.name.as_str()));
    let mut problems: Vec<String> = stating()
        .filter(|venue| !cited_invocations(&venue.reached).iter().any(|name| runs_tests.contains(name)))
        .map(|venue| {
            format!(
                "{PAGE}: `{}` states a verdict only a venue something RUNS may state, and its \
                 `Reached by` cell ({}) names no task that runs a test - a lint, a build or a docs \
                 job is something CI invokes and is not something a venue is proven in. Either the \
                 cell does not name what reaches this venue, or the honest cell is `unrun`. What is \
                 held is that the named task runs tests, never that it runs THIS venue's tests",
                venue.name, venue.reached
            )
        })
        .collect();
    problems.extend(
        stating()
            .filter(|venue| venue.site() == Some(ON_DEMAND))
            .filter(|venue| {
                !cited_invocations(&venue.reached)
                    .iter()
                    .any(|name| invoked.contains(name) && runs_tests.contains(name))
            })
            .map(|venue| {
                format!(
                    "{PAGE}: `{}` says it runs in `{}` - ON DEMAND - and states a verdict only a \
                     venue something RUNS may state, while no job invokes a test-running task its \
                     `Reached by` cell ({}) names. A job is the only thing that can demand a run \
                     there, so until one does the honest cell is `unrun`. An `in process` venue is \
                     run by the suite itself and is not held to this",
                    venue.name, venue.runs, venue.reached
                )
            }),
    );
    problems
}
