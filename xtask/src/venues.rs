//! Where each identity claim is proven, held against the tree instead of by recall.
//!
//! `docs/where-identity-is-proven.md` is the map `AGENTS.md` routes a reader to before citing a
//! green run as proof of impersonation, and `docs/adr/0017`'s fifth amendment is the decision it
//! renders. Until this gate, **every part of it was prose nothing read** - and the failure it
//! guards against is not a red run but a green one read as proving more than it did, which is the
//! page's own opening argument. So the page's own rules are the rules here.
//!
//! # What is mechanical
//!
//! * **The two tables are one table**, paired by position: the matrix's Nth column is a short form
//!   of the venues table's Nth row. A column is where a venue states its limit per claim, so a
//!   venue with no column has no limit, and a reorder in one table only moves every limit.
//! * **Every cell states a verdict from a closed vocabulary.** An unclassifiable cell is a venue
//!   that has silently lost its limit for that claim. Two were in that state when this was
//!   written - see `a_cell_that_states_a_sentence_instead_of_a_verdict_fails`.
//! * **The three rules over the two un-citable verdicts and the one over a citable one** live in
//!   `mod verdicts`, beside the code, and its header carries why each exists and the limit each
//!   does not reach: `unrun` expires when CI reaches a venue, `wired` is refused until it does,
//!   and a citable verdict needs an invocation that CI runs AND that runs tests.
//! * **A venue's `Where it runs` cell states a place from a closed vocabulary**, because it decides
//!   whether the row may be cited at all and was free prose. [`page::RUN_SITES`] has the measurement.
//! * **A venue nothing runs claims nothing**, and a venue that IS reached and answers no claim is
//!   a row with no reason to be there.
//! * **Every venue has its own section**, and every `##` section is a venue's or one of the three
//!   structural ones - so a section for a venue absent from the table fails too.
//! * **Every test the page cites exists and is a test.** The page says those names are the list to
//!   check a claim against, which a rename turns into a citation of nothing.
//! * **The acceptance venue's limit is a workflow property**, so it is read out of the workflow -
//!   see the `acceptance` module, whose own header lists what it reads, what it does NOT reach, and
//!   the seven ways two earlier drafts of it read a regression as compliance. **No count here**: the
//!   same properties have been called five, seven and nine in one branch, which is what
//!   `AGENTS.md` means by *write the command and the date, or delete the number*.
//!
//! # What is not
//!
//! **Whether a venue's prose limit is TRUE.** This reads a vocabulary, a pairing and a workflow;
//! whether *a key proves `SharedServiceUser` only* is still the right sentence is review's. It
//! also cannot see the two venues that run nowhere, which is the point of their rows.
//!
//! **Which `just` task reaches a venue.** `.github/scripts/check-task-citations.sh` owns a `just`
//! citation in a page and a second owner would be a second answer; what is read here is whether
//! the cell says `not built` - because that decides what the column may claim - and whether CI
//! invokes the name it cites, which is a different question from whether the task exists.
//!
//! **Whether a run was GREEN, or happened at all.** [`invoked`] reads a workflow, not a run: the
//! authority for *did this pass* is the GitHub API, which is unreachable from `checks.hygiene` for
//! the same reason `nix eval` is unreachable from `check-workflows`. So the rule is one-sided by
//! construction, and the side it holds is the one that fails silently.

use std::collections::BTreeSet;

use crate::Verdict;
use crate::repo;

// The acceptance venue's half. Its limit is a property of `.github/workflows/ci.yml` rather than of
// the page, so it reads a different file with a different parser - and every assertion about it
// lives beside that parser, where the fixture it needs is.
mod acceptance;

// The same venue's ENVIRONMENT contract, one layer out: `acceptance` reads what the job DOES with
// each value, and this reads whether the environment can be made to carry it at all. Three lists
// of names - the sync script, the page's table, the workflow's own references - and until this
// nothing reconciled any two of them. Its own file for the reason `acceptance` is: a different
// parser over different files, with its fixtures beside it.
mod environment;

// What the PAGE is - a venues row, a claims cell, a verdict word, a `##` section - as against what
// it must HOLD, which is this file. The same seam `acceptance` draws over `mod shape;`, and the
// 1000-line gate is what forced it here too: every `#[test]` stayed, only the parsing moved.
mod page;

use page::{NOT_BUILT, STRUCTURAL, VERDICTS, cited_tests, claims, key, venues, verdict};

// What the TREE holds - which tests exist, and which tasks CI really runs - as against what the
// page says about it. The third side of the seam `mod page;` opens, and the 1000-line gate is what
// forced it: every `#[test]` stayed here, only the readers moved.
// What a venues ROW must HOLD before a verdict rule may read one. Split out when this file
// reached the 1000-line gate a third time; every `#[test]` stayed here.
mod rows;

use rows::row_problems;

// What a VERDICT has to be consistent with, once a row can be judged at all. The fourth seam, and
// the 1000-line gate forced this one too; every `#[test]` stayed here.
mod verdicts;

use verdicts::{anchor_problems, transition_problems};

mod sources;

use sources::{invoked, test_names, test_tasks};

/// The map. One page: a second copy of a venue table is the drift this gate is about.
const PAGE: &str = "docs/where-identity-is-proven.md";

/// Everything wrong with the page.
fn page_problems(text: &str, tests: &BTreeSet<String>, invoked: &BTreeSet<String>, runs_tests: &BTreeSet<String>) -> Vec<String> {
    let listed = match venues(text) {
        Ok(listed) => listed,
        Err(problem) => return vec![problem],
    };
    if listed.len() < 3 {
        return vec![format!(
            "{PAGE} lists {} venue(s); a map that small would pass anything",
            listed.len()
        )];
    }

    let (header, claims) = match claims(text) {
        Ok(read) => read,
        Err(problem) => return vec![problem],
    };
    let columns: Vec<&String> = header.iter().skip(1).collect();
    let mut problems = Vec::new();

    if columns.len() != listed.len() {
        problems.push(format!(
            "{PAGE}: {} venue(s) in the venues table and {} column(s) in the claims matrix - a \
             venue with no column states no limit, and a column with no row is a venue nothing \
             describes",
            listed.len(),
            columns.len()
        ));
        return problems;
    }

    for (venue, column) in listed.iter().zip(&columns) {
        let name = key(&venue.name);
        let unmatched: Vec<&str> = column
            .split_whitespace()
            .map(|word| word.trim_matches(|c: char| !c.is_ascii_alphanumeric()))
            .filter(|word| !word.is_empty() && !name.contains(&word.to_lowercase()))
            .collect();
        if !unmatched.is_empty() {
            problems.push(format!(
                "{PAGE}: the claims column `{column}` does not name the venue in the same position \
                 (`{}`) - {unmatched:?} appears in neither. The two tables are paired by ORDER, so \
                 a reorder in one of them silently moves every limit",
                venue.name
            ));
        }
    }

    if claims.len() < 5 {
        problems.push(format!(
            "{PAGE}: the claims matrix has {} claim(s) - a matrix that small is not the map",
            claims.len()
        ));
    }

    // `claimed` is *does this row have a reason to be in the table*, which `yes`, `can` and
    // `unrun` all earn - a written test is a reason for a row, and is not evidence. **What is NOT
    // held here, said plainly: *may this venue be CITED for that claim*, which only `yes` and
    // `can` earn.** That distinction is carried by [`VERDICTS`]'s own documentation and by the
    // `unrun` arm below refusing a venue nothing reaches; a set of citable venues collected here
    // would have been read by nothing, which `clippy::collection_is_never_read` said out loud.
    let mut claimed: BTreeSet<&str> = BTreeSet::new();
    // The venues that state a verdict only something RUNNING them may state - `yes` and `can`
    // because they are the two citable ones, `wired` because it asserts a job reaches this venue.
    // `unrun` is excluded by its own rule, which refuses a venue CI invokes at all.
    let mut citable: BTreeSet<&str> = BTreeSet::new();
    let mut unrun: BTreeSet<&str> = BTreeSet::new();
    let mut wired: BTreeSet<&str> = BTreeSet::new();
    for row in &claims {
        let claim = row.first().map_or("", String::as_str);
        if row.len() != listed.len().saturating_add(1) {
            problems.push(format!(
                "{PAGE}: the claim `{claim}` has {} cell(s) for {} venue(s) - a missing cell is a \
                 venue whose limit for that claim is unstated",
                row.len().saturating_sub(1),
                listed.len()
            ));
            continue;
        }
        for (venue, cell) in listed.iter().zip(row.iter().skip(1)) {
            match verdict(cell) {
                None => problems.push(format!(
                    "{PAGE}: `{}` says `{cell}` about `{claim}`, which is not one of {VERDICTS:?}. \
                     A cell is where a venue states its limit for one claim, so a sentence there \
                     is a limit nothing can read",
                    venue.name
                )),
                Some(word @ ("yes" | "can")) => {
                    claimed.insert(venue.name.as_str());
                    citable.insert(venue.name.as_str());
                    if !venue.is_built() {
                        problems.push(format!(
                            "{PAGE}: `{}` is `{NOT_BUILT}` and claims `{word}` about `{claim}` - a \
                             venue nothing runs cannot be cited for anything, which is the \
                             overstatement this page exists to prevent",
                            venue.name
                        ));
                    }
                    // **The second half of that same guard, and it was missing.** `is_built` reads
                    // ONE cell, so a venue gained the right to be cited the moment its `Reached by`
                    // named a task - even while its own `Where it runs` still said it runs nowhere.
                    // Measured: with the exchange venue in that state, the row carrying leg 2 could
                    // be edited from `only here` to `yes` and `check-venues` exited 0. A venue that
                    // runs nowhere may have a written test, which is what `unrun` is for; it may
                    // not be evidence for anything.
                    else if venue.runs_nowhere() {
                        problems.push(format!(
                            "{PAGE}: `{}` claims `{word}` about `{claim}` while its own `Where it \
                             runs` cell says `{}` - a venue that runs nowhere cannot be cited, \
                             whatever its `Reached by` names. A written test that nothing can point \
                             at is `unrun`",
                            venue.name, venue.runs
                        ));
                    }
                }
                // The two un-citable verdicts, which differ only in whether CI has reached the
                // venue yet - so they earn a row for exactly the same reason and are refused for a
                // venue nothing runs for exactly the same reason. `transition_problems` is where
                // they part.
                Some(word @ ("unrun" | "wired")) => {
                    claimed.insert(venue.name.as_str());
                    if word == "unrun" {
                        unrun.insert(venue.name.as_str());
                    } else {
                        wired.insert(venue.name.as_str());
                        citable.insert(venue.name.as_str());
                    }
                    if !venue.is_built() {
                        problems.push(format!(
                            "{PAGE}: `{}` is `{NOT_BUILT}` and says `{word}` about `{claim}` - \
                             `{word}` is a test that EXISTS and has not been run, so a venue \
                             nothing reaches cannot be in that state: either something reaches it \
                             and the `Reached by` cell should say so, or the cell is `-`",
                            venue.name
                        ));
                    }
                }
                Some(_) => {}
            }
        }
    }

    for venue in listed.iter().filter(|venue| venue.is_built()) {
        if !claimed.contains(venue.name.as_str()) {
            problems.push(format!(
                "{PAGE}: `{}` is reached by `{}` and claims nothing in the matrix - not one `yes`, \
                 `can`, `unrun` or `wired`. Either it answers a claim and the column does not say \
                 so, a test is written for it and the cell should say `unrun` (or `wired`, once a \
                 job reaches it), or the row has no reason to be there",
                venue.name, venue.reached
            ));
        }
    }

    let sections: BTreeSet<String> = text
        .lines()
        .filter_map(|line| line.strip_prefix("## "))
        .map(|heading| heading.trim().to_owned())
        .collect();
    let expected: BTreeSet<String> = listed.iter().map(|venue| key(&venue.name)).collect();
    for venue in &listed {
        if !sections.iter().any(|heading| key(heading) == key(&venue.name)) {
            problems.push(format!(
                "{PAGE}: `{}` has no `##` section - a venue states what it cannot answer in its \
                 own section, and a row with no section states nothing",
                venue.name
            ));
        }
    }
    for heading in &sections {
        if !expected.contains(&key(heading)) && !STRUCTURAL.contains(&heading.as_str()) {
            problems.push(format!(
                "{PAGE}: the section `{heading}` is neither a venue in the table nor one of \
                 {STRUCTURAL:?} - a venue described in a section and absent from the table is a \
                 venue with no limit"
            ));
        }
    }

    problems.extend(row_problems(&listed));
    problems.extend(transition_problems(text, &listed, &unrun, &wired, invoked));
    problems.extend(anchor_problems(&listed, &citable, invoked, runs_tests));

    for name in cited_tests(text) {
        if !tests.contains(&name) {
            problems.push(format!(
                "{PAGE} cites `{name}`, which is no test in this workspace - the names on this \
                 page are what a claim is checked against, so a renamed test leaves a citation of \
                 nothing"
            ));
        }
    }

    problems
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let (root, files) = match repo::all_files().and_then(|census| census.into_listing(repo::Unmigrated::Venues)) {
        Ok(listing) => listing,
        Err(why) => {
            eprintln!("xtask check-venues: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };
    let Ok(page) = std::fs::read_to_string(root.join(PAGE)) else {
        eprintln!("xtask check-venues: {PAGE} is not readable - it IS the map");
        return Verdict::Fail;
    };
    let Ok(workflow) = std::fs::read_to_string(root.join(acceptance::WORKFLOW)) else {
        eprintln!("xtask check-venues: {} is not readable", acceptance::WORKFLOW);
        return Verdict::Fail;
    };

    let tests = test_names(&root, &files);
    if tests.len() < 100 {
        eprintln!(
            "xtask check-venues: found {} test(s) in the workspace - the scan is broken, not the page",
            tests.len()
        );
        return Verdict::Fail;
    }

    let Some(invoked) = invoked(&root) else {
        eprintln!("xtask check-venues: the CI scan is broken, not the page");
        return Verdict::Fail;
    };
    // An empty or near-empty set would make every `unrun` cell pass by finding nothing, which is
    // the way a text scan goes quiet. CI reaches well over a dozen tasks and apps; a handful means
    // the walk broke rather than that the workflows changed.
    if invoked.len() < 10 {
        eprintln!(
            "xtask check-venues: CI appears to invoke {} task(s) or app(s) - the scan is broken, not the page",
            invoked.len()
        );
        return Verdict::Fail;
    }

    // The second half of the anchor, resolved out of the justfile rather than the workflows: CI
    // invokes lints and builds too, so *CI invokes it* alone never said a venue was RUN.
    let Some(runs_tests) = test_tasks(&root) else {
        eprintln!("xtask check-venues: no `just` task in this workspace runs a test - the justfile scan is broken, not the page");
        return Verdict::Fail;
    };

    let mut problems = page_problems(&page, &tests, &invoked, &runs_tests);
    problems.extend(acceptance::problems(&workflow));
    let contract = environment::problems(&root);
    problems.extend(contract.problems);

    if problems.is_empty() {
        println!(
            "xtask check-venues: ok - {PAGE} and the `{}` job, {} cited test(s), {} CI invocation(s) resolved, {} task(s) that run tests, {} environment name(s) reconciled across three lists",
            acceptance::JOB,
            cited_tests(&page).len(),
            invoked.len(),
            runs_tests.len(),
            contract.names
        );
        return Verdict::Pass;
    }

    eprintln!("xtask check-venues: FAILED");
    for problem in &problems {
        eprintln!("  {problem}");
    }
    eprintln!();
    eprintln!("A venue that cannot state its limit is how `verified` drifts - {PAGE} says so at");
    eprintln!("its own foot. Fix the page or the job; if the rule is wrong, change it in");
    eprintln!("xtask/src/venues.rs with a reason.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::sources::{cited_invocations, invocation, starts_a_command};
    use super::{PAGE, VERDICTS, page_problems, verdict};

    /// A page with two venues too few is refused, so the fixtures below carry three.
    const MAP: &str = "\
## The venues

| Venue | Where it runs | What it costs | Reached by |
| --- | --- | --- | --- |
| **A fake at the port** | in process | nothing | `just test` |
| **A real dataset under a shared key** | a GitHub environment | a key | `just bigquery-acceptance` |
| **A real token exchange** | nowhere yet | a pool | not built |

## Which venue answers which claim

| Claim | Fake at the port | Real dataset, shared key | Real exchange |
| --- | --- | --- | --- |
| A refusal is a result | **yes** | - | - |
| A caller cannot state its identity | **yes** (a type) | - | - |
| Two subjects, two credentials | **yes**, at the port | - | redundant |
| A statement is accepted | no | **yes** | - |
| An exchange endpoint accepts it | no | no | **only here** |

## The fake at the port

`a_refusal_is_a_result_and_reachable` is the standing test.

## A real dataset under a shared key

Nothing about who asked.

## A real token exchange

Not built.
";

    fn known() -> BTreeSet<String> {
        std::iter::once("a_refusal_is_a_result_and_reachable".to_owned()).collect()
    }

    /// The CI [`MAP`] describes: it invokes what the map's two citable venues cite, and one task
    /// no venue cites at all. Both halves matter - the anchor rule refuses a citable venue whose
    /// task no job invokes, so a fixture CI that invoked NOTHING would fail every assertion below
    /// for the wrong reason, and one that invoked everything would hide the `unrun` expiry.
    fn invokes_the_cited_tasks() -> BTreeSet<String> {
        ["test", "bigquery-acceptance", "xtask"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    /// The fixture's tasks that RUN tests - every task the map cites, plus one that is not a task
    /// at all. So the anchor rule turns here on whether CI invokes the name, which is the half each
    /// assertion is about; the other half has its own two arms below.
    fn task_runs_tests() -> BTreeSet<String> {
        ["test", "bigquery-acceptance", "bigquery-exchanged-identity"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    fn problems(page: &str) -> Vec<String> {
        page_problems(page, &known(), &invokes_the_cited_tasks(), &task_runs_tests())
    }

    fn problems_with_ci(page: &str, invoked: &[&str]) -> Vec<String> {
        let invoked: BTreeSet<String> = invoked.iter().map(|name| (*name).to_owned()).collect();
        page_problems(page, &known(), &invoked, &task_runs_tests())
    }

    /// [`problems`] with the map's own CI and a stated set of test-running tasks, which is the one
    /// input the two arms below vary.
    fn problems_with_test_tasks(page: &str, runs_tests: &[&str]) -> Vec<String> {
        let runs_tests: BTreeSet<String> = runs_tests.iter().map(|name| (*name).to_owned()).collect();
        page_problems(page, &known(), &invokes_the_cited_tasks(), &runs_tests)
    }

    #[test]
    fn the_fixture_map_passes_so_every_failure_below_is_the_change_and_not_the_fixture() {
        assert_eq!(problems(MAP), Vec::<String>::new());
    }

    #[test]
    fn a_cell_that_states_a_sentence_instead_of_a_verdict_fails() {
        // RED WHEN WRITTEN, against the real page: `see below` and `painful to script` sat in the
        // real provider's column, so that venue's limit for two claims was a pointer and a cost
        // rather than an answer. Both are corrected in the same change.
        let vague = MAP.replace(
            "| A statement is accepted | no | **yes** | - |",
            "| A statement is accepted | see below | **yes** | - |",
        );
        let found = problems(&vague);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("see below"), "{found:?}");
    }

    #[test]
    fn a_venue_nothing_runs_may_not_claim_yes() {
        let overstated = MAP.replace(
            "| An exchange endpoint accepts it | no | no | **only here** |",
            "| An exchange endpoint accepts it | no | no | **yes** |",
        );
        let found = problems(&overstated);
        assert!(found.iter().any(|p| p.contains("cannot be cited for anything")), "{found:?}");
    }

    /// [`MAP`] with the shared-key venue's one answer downgraded to `unrun`, and the word added to
    /// its section - which is the whole state the verdict exists for: a test written here, no run.
    fn map_with_an_unrun_cell() -> String {
        MAP.replace(
            "| A statement is accepted | no | **yes** | - |",
            "| A statement is accepted | no | **unrun** | - |",
        )
        .replace(
            "## A real dataset under a shared key\n\nNothing about who asked.\n",
            "## A real dataset under a shared key\n\nNothing about who asked, and the leg is unrun.\n",
        )
    }

    #[test]
    fn a_venue_whose_only_verdict_is_unrun_still_has_a_reason_to_be_in_the_table() {
        // The state `can` used to have to carry: the standing test is HERE and nothing has run it.
        // It is not evidence, so it is not `yes` or `can` - and it IS a reason for the row, so the
        // *claims nothing in the matrix* arm must not fire.
        // With `bigquery-acceptance` absent from the fixture CI, because that is the state an
        // `unrun` cell is honest in - the anchor rule and the expiry rule read the same name.
        assert_eq!(problems_with_ci(&map_with_an_unrun_cell(), &["test"]), Vec::<String>::new());
    }

    #[test]
    fn a_venue_nothing_runs_may_not_say_unrun() {
        // RED WHEN WRITTEN: `unrun` is a test that exists and has not been run, so a venue nothing
        // reaches cannot be in that state - it would read as *written, waiting* over a venue with
        // nothing to run it.
        let overstated = MAP.replace(
            "| An exchange endpoint accepts it | no | no | **only here** |",
            "| An exchange endpoint accepts it | no | no | **unrun** |",
        );
        let found = problems(&overstated);
        assert!(found.iter().any(|p| p.contains("cannot be in that state")), "{found:?}");
    }

    #[test]
    fn an_unrun_cell_whose_section_never_uses_the_word_fails() {
        // The tie that makes `unrun` a token rather than a caveat: the cell is where the state is
        // read and the section is where it is explained, and prose explaining it in other words is
        // exactly how the two came apart while `can` carried both meanings.
        let unexplained = MAP.replace(
            "| A statement is accepted | no | **yes** | - |",
            "| A statement is accepted | no | **unrun** | - |",
        );
        let found = problems(&unexplained);
        assert!(found.iter().any(|p| p.contains("never uses that word")), "{found:?}");
    }

    #[test]
    fn a_venue_ci_invokes_may_not_say_unrun() {
        // RED WHEN WRITTEN, and the finding was that NOTHING was red: the three rules `unrun`
        // arrived with read a spelling, so wiring the leg into a job and watching it go green on
        // every push left this page saying nothing had run it, with `just validate` green. The
        // fixture's shared-key venue is `Reached by` `just bigquery-acceptance`, so a CI that
        // invokes that name - as `nix run .#bigquery-acceptance`, which is how CI spells it - is
        // the exact state the cell may not be in.
        let found = problems_with_ci(&map_with_an_unrun_cell(), &["bigquery-acceptance"]);
        assert!(
            found.iter().any(|p| p.contains("CI invokes")),
            "an `unrun` cell whose venue a job reaches has to fail: {found:?}"
        );
    }

    #[test]
    fn a_venue_ci_does_not_invoke_may_still_say_unrun() {
        // The other direction, which is the state this cell is legitimately in until the job that
        // runs it lands: a written test, nothing wired, so `unrun` is honest and must pass. Without
        // this, the arm above would be satisfied by refusing every `unrun` cell there is.
        assert_eq!(
            problems_with_ci(&map_with_an_unrun_cell(), &["deny", "crap", "xtask", "test"]),
            Vec::<String>::new()
        );
    }

    /// [`MAP`] with the shared-key venue's one answer at `wired`, and the word in its section -
    /// the state a venue is in between the change that wires its job and the first run anybody has
    /// seen.
    fn map_with_a_wired_cell() -> String {
        MAP.replace(
            "| A statement is accepted | no | **yes** | - |",
            "| A statement is accepted | no | **wired** | - |",
        )
        .replace(
            "## A real dataset under a shared key\n\nNothing about who asked.\n",
            "## A real dataset under a shared key\n\nNothing about who asked, and the leg is wired and unseen.\n",
        )
    }

    #[test]
    fn a_venue_ci_invokes_says_wired_rather_than_unrun() {
        // RED WHEN WRITTEN, and the finding is what `unrun`'s expiry rule left with nowhere to go:
        // the change that WIRES a leg cannot also produce its first green run, because the run
        // happens after the push. So the two moves available were a cell the gate refuses and a
        // `yes` nobody has earned. This is the third, and it passes only while CI really does
        // invoke the name the `Reached by` cell states.
        assert_eq!(
            problems_with_ci(&map_with_a_wired_cell(), &["bigquery-acceptance", "test"]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_venue_no_job_invokes_may_not_say_wired() {
        // The mirror of `a_venue_ci_invokes_may_not_say_unrun`, and without it `wired` would be a
        // spelling of `unrun` that skips `unrun`'s own expiry: a cell could claim a wiring that is
        // not in any workflow and never be asked for it again.
        let found = problems_with_ci(&map_with_a_wired_cell(), &["deny", "crap", "xtask"]);
        assert!(
            found.iter().any(|p| p.contains("CI invokes nothing")),
            "a `wired` cell over a venue no job reaches has to fail: {found:?}"
        );
    }

    #[test]
    fn a_wired_cell_whose_section_never_uses_the_word_fails() {
        // The same tie `unrun` carries, held for the new word by the same loop rather than by a
        // second copy of the rule - a cell is where the state is read, a section is where it is
        // explained, and prose explaining it in other words is how those two came apart before.
        let unexplained = MAP.replace(
            "| A statement is accepted | no | **yes** | - |",
            "| A statement is accepted | no | **wired** | - |",
        );
        let found = problems_with_ci(&unexplained, &["bigquery-acceptance"]);
        assert!(found.iter().any(|p| p.contains("never uses that")), "{found:?}");
    }

    #[test]
    fn a_venue_nothing_runs_may_not_say_wired() {
        // `not built` and `wired` are contradictory in the one direction that matters: a venue
        // nothing runs at all cannot be one a job reaches.
        let overstated = MAP.replace(
            "| An exchange endpoint accepts it | no | no | **only here** |",
            "| An exchange endpoint accepts it | no | no | **wired** |",
        );
        let found = problems_with_ci(&overstated, &["bigquery-acceptance"]);
        assert!(found.iter().any(|p| p.contains("cannot be in that state")), "{found:?}");
    }

    #[test]
    fn a_venue_whose_only_verdict_is_wired_still_has_a_reason_to_be_in_the_table() {
        // The same accounting `unrun` earns: not evidence, and still a reason for the row. Without
        // this the *claims nothing in the matrix* arm would fire on every wired-and-unseen venue,
        // which is the state this verdict exists to name.
        let found = problems_with_ci(&map_with_a_wired_cell(), &["bigquery-acceptance"]);
        assert!(!found.iter().any(|p| p.contains("claims nothing in the matrix")), "{found:?}");
    }

    /// [`MAP`] with the exchange venue given a task that reaches it while its own `Where it runs`
    /// still says nowhere - the exact state #382 put the real page in, and the one that turned the
    /// guard on `yes` off.
    fn map_with_a_venue_that_runs_nowhere_but_names_a_task() -> String {
        MAP.replace(
            "| **A real token exchange** | nowhere yet | a pool | not built |",
            "| **A real token exchange** | nowhere yet | a pool | `just bigquery-exchanged-identity` |",
        )
        .replace(
            "| An exchange endpoint accepts it | no | no | **only here** |",
            "| An exchange endpoint accepts it | no | no | **unrun** |",
        )
        .replace(
            "## A real token exchange\n\nNot built.\n",
            "## A real token exchange\n\nThe leg is written and unrun.\n",
        )
    }

    #[test]
    fn a_venue_that_runs_nowhere_may_not_be_cited_however_its_reached_by_reads() {
        // **RED WHEN WRITTEN, and the finding is that nothing was red.** `is_built` reads ONE cell,
        // so the moment a venue's `Reached by` named a task it earned the right to say `yes` - even
        // with its own `Where it runs` still saying it runs nowhere. Measured on the real page: the
        // row carrying *two subjects read two different row sets* could be edited from `only here`
        // to `yes` and `check-venues` exited 0. Both cells are read now.
        let overstated = map_with_a_venue_that_runs_nowhere_but_names_a_task().replace(
            "| An exchange endpoint accepts it | no | no | **unrun** |",
            "| An exchange endpoint accepts it | no | no | **yes** |",
        );
        let found = problems(&overstated);
        assert!(found.iter().any(|p| p.contains("says `nowhere yet`")), "{found:?}");
    }

    #[test]
    fn a_venue_that_runs_nowhere_may_still_say_unrun_with_a_task_that_reaches_it() {
        // The other direction, and it is a real state rather than a loophole: a written test, a
        // task that reaches it, and nothing that can point the task at anything. Without this the
        // rule above would be satisfied by refusing every venue in that position, which is the one
        // this page most needs to be able to describe.
        assert_eq!(
            problems(&map_with_a_venue_that_runs_nowhere_but_names_a_task()),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_synonym_for_nowhere_is_not_a_place_this_page_may_state() {
        // **RED WHEN WRITTEN.** `runs_nowhere` matched the SUBSTRING `nowhere`, and it is the only
        // remaining guard on `yes` for the row carrying leg 2. Measured on the real page: spell the
        // cell `not anywhere yet`, point `Reached by` at a real CI-invoked lint, and *whether two
        // subjects read two different row sets* published `yes` at exit 0.
        let reworded = map_with_a_venue_that_runs_nowhere_but_names_a_task()
            .replace("| nowhere yet | a pool |", "| not anywhere yet | a pool |")
            .replace(
                "| An exchange endpoint accepts it | no | no | **unrun** |",
                "| An exchange endpoint accepts it | no | no | **yes** |",
            );
        let found = problems_with_ci(&reworded, &["bigquery-exchanged-identity"]);
        assert!(found.iter().any(|p| p.contains("about where it runs")), "{found:?}");

        // And the direction that gets a gate deleted, which is why `runs_nowhere` reads the
        // classified site: a venue that DOES run, whose cell spells the word while saying so, is
        // not a venue that runs nowhere.
        let explained = MAP.replace("| in process |", "| in process, nowhere near a warehouse |");
        assert_eq!(problems(&explained), Vec::<String>::new());
    }

    #[test]
    fn a_venue_that_runs_nowhere_may_not_say_wired_either() {
        // `wired` is *a job reaches this*, so it contradicts a row that says the venue runs
        // nowhere. Two cells of one row disagreeing is the shape the `yes` guard was found missing.
        let contradictory = map_with_a_venue_that_runs_nowhere_but_names_a_task().replace(
            "| An exchange endpoint accepts it | no | no | **unrun** |",
            "| An exchange endpoint accepts it | no | no | **wired** |",
        );
        let found = problems_with_ci(&contradictory, &["bigquery-exchanged-identity"]);
        assert!(found.iter().any(|p| p.contains("contradict each other")), "{found:?}");
    }

    #[test]
    fn a_reached_by_that_names_no_invocation_leaves_the_venue_unjudgeable() {
        // **The cheapest bypass on the page, and it predates `wired`.** Every rule that decides a
        // venue's state resolves this cell through `cited_invocations`, which wants the spelling
        // the page uses - so dropping `just ` made the cell resolve to nothing and `unrun`
        // permanent at exit 0, whatever CI ran.
        let unresolvable = map_with_an_unrun_cell().replace(
            "| **A real dataset under a shared key** | a GitHub environment | a key | `just bigquery-acceptance` |",
            "| **A real dataset under a shared key** | a GitHub environment | a key | the `bigquery-acceptance` task |",
        );
        let found = problems_with_ci(&unresolvable, &["bigquery-acceptance"]);
        assert!(found.iter().any(|p| p.contains("names no `just <task>`")), "{found:?}");
    }

    #[test]
    fn a_task_named_inside_a_printed_string_is_not_an_invocation() {
        // **The bypass that made `wired` weaker than `unrun`, measured.** `invoked` scanned each
        // non-comment line for a substring, so a sentence inside `echo` resolved the name and a
        // venue no job runs said `wired` at exit 0 with a byte-identical summary. For `unrun` the
        // incentive to write that line runs against the author; for `wired` it runs with them.
        assert_eq!(
            starts_a_command("          nix run .#bigquery-two-principals -- --no-capture"),
            Some("bigquery-two-principals")
        );
        assert_eq!(starts_a_command("    just lint-ci"), Some("lint-ci"));
        assert_eq!(starts_a_command("    exec just test"), Some("test"));
        assert_eq!(starts_a_command("        - just hygiene"), Some("hygiene"));
        // A wrapper runs the real command after its own argument terminator. Measured: the first
        // version of this reader resolved 18 names where the substring scan resolved 19, and this
        // line in `nix/container-setup.sh` was the one it lost.
        assert_eq!(starts_a_command("devenv shell -- just setup"), Some("setup"));
        assert_eq!(starts_a_command("  nix develop --command just test"), Some("test"));
        // And the prose forms, which are what this exists to refuse.
        assert_eq!(
            starts_a_command("echo to run this leg locally use nix run .#bigquery-two-principals please"),
            None
        );
        assert_eq!(starts_a_command("printf 'run just test first'"), None);
        // And the terminator inside a printed sentence is not a wrapper's, which is why this reads
        // the wrapper as the segment's own command rather than splitting lines on `--`.
        assert_eq!(starts_a_command("echo \"use -- just test to start\""), None);
        assert_eq!(
            starts_a_command("echo \"CI runs it through nix run .#bigquery-acceptance\""),
            None
        );
    }

    #[test]
    fn an_invocation_is_a_task_or_an_app_and_prose_is_neither() {
        assert_eq!(invocation("just bigquery-two-principals"), Some("bigquery-two-principals"));
        assert_eq!(
            invocation("nix run .#bigquery-two-principals"),
            Some("bigquery-two-principals")
        );
        assert_eq!(invocation("nix run .#just -- ci"), Some("ci"));
        assert_eq!(invocation("just --summary"), None);
        assert_eq!(invocation("nix build .#checks.x.nextest"), None);
        assert_eq!(invocation("cargo nextest run"), None);
        assert_eq!(invocation("just "), None);
        // A `Reached by` cell is read from its BACKTICKED spans and not word by word, because the
        // cells that are prose ("a GitHub environment, on demand") would otherwise contribute
        // whatever follows the word `just` in a sentence.
        let cited = cited_invocations("`just test`, `just validate` - not on a fork");
        assert_eq!(cited, ["test".to_owned(), "validate".to_owned()].into_iter().collect());
        assert!(cited_invocations("not built").is_empty());
    }

    #[test]
    fn a_venue_with_no_column_in_the_matrix_fails() {
        let unpaired = MAP.replace(
            "| **A real token exchange** | nowhere yet | a pool | not built |\n",
            "| **A real token exchange** | nowhere yet | a pool | not built |\n| **A real provider** | nowhere yet | a provider | not built |\n",
        );
        let found = problems(&unpaired);
        assert!(found.iter().any(|p| p.contains("states no limit")), "{found:?}");
    }

    #[test]
    fn a_reorder_in_one_table_only_is_what_the_pairing_catches() {
        let swapped = MAP.replace(
            "| Claim | Fake at the port | Real dataset, shared key | Real exchange |",
            "| Claim | Fake at the port | Real exchange | Real dataset, shared key |",
        );
        let found = problems(&swapped);
        assert!(
            found
                .iter()
                .any(|p| p.contains("does not name the venue in the same position")),
            "{found:?}"
        );
    }

    #[test]
    fn a_venue_with_no_section_of_its_own_fails_and_so_does_a_section_with_no_row() {
        let no_section = MAP.replace("## A real token exchange\n\nNot built.\n", "");
        assert!(
            problems(&no_section).iter().any(|p| p.contains("has no `##` section")),
            "{no_section}"
        );

        let stray = format!("{MAP}\n## A real workforce provider\n\nSomething.\n");
        assert!(
            problems(&stray)
                .iter()
                .any(|p| p.contains("neither a venue in the table nor one of")),
            "a section for a venue that is not in the table has no limit anywhere"
        );
    }

    #[test]
    fn a_citation_of_a_test_that_does_not_exist_fails() {
        let renamed = MAP.replace(
            "a_refusal_is_a_result_and_reachable",
            "a_refusal_is_a_result_and_renamed_away",
        );
        let found = problems(&renamed);
        assert!(found.iter().any(|p| p.contains("is no test in this workspace")), "{found:?}");
    }

    #[test]
    fn a_missing_table_is_a_failure_rather_than_an_empty_scan() {
        let gutted = MAP.replace(
            "| Claim | Fake at the port | Real dataset, shared key | Real exchange |",
            "no matrix here",
        );
        let found = problems(&gutted);
        assert!(found.iter().any(|p| p.contains("has no claims table")), "{found:?}");
    }

    #[test]
    fn the_verdict_vocabulary_reads_a_word_and_not_a_prefix() {
        assert_eq!(verdict("**yes**, that *we refuse one*"), Some("yes"));
        assert_eq!(verdict("**can** - the builder takes several audiences"), Some("can"));
        assert_eq!(verdict("**unrun** - the only venue that could"), Some("unrun"));
        assert_eq!(
            verdict("**wired** - a job reaches it and no run has been seen"),
            Some("wired")
        );
        assert_eq!(verdict("no - one key is one identity"), Some("no"));
        assert_eq!(verdict("**only here**"), Some("only here"));
        assert_eq!(verdict("-"), Some("-"));
        assert_eq!(verdict("see below"), None);
        assert_eq!(verdict("painful to script"), None);
        assert_eq!(verdict(""), None);
        // `nothing` starts with no verdict, and `yesterday` is not `yes`.
        assert_eq!(verdict("yesterday"), None);
        assert!(VERDICTS.contains(&"only here"));
        assert!(VERDICTS.contains(&"unrun"));
        assert!(VERDICTS.contains(&"wired"));
    }

    #[test]
    fn a_citable_verdict_needs_a_task_that_runs_a_test() {
        // **RED WHEN WRITTEN, and the finding is that nothing was red.** `Reached by` earned a
        // citation by naming a task, and for `wired` also by CI invoking it - and CI invokes lints,
        // builds, docs and release jobs. Recorded on the page with the measurement: pointing the
        // two-keys row at a real, CI-invoked, entirely unrelated lint and moving its cell passed at
        // exit 0. Here the fixture's shared-key venue keeps its `yes` and its CI invocation, and
        // only *does that task run a test* changes.
        let found = problems_with_test_tasks(MAP, &["test"]);
        assert!(
            found.iter().any(|p| p.contains("that runs a test")),
            "a `yes` anchored on a task that runs no test has to fail: {found:?}"
        );

        // The direction that gets a gate deleted: the same page, the same CI, with the task
        // recognised as one that runs tests, passes. Without this the arm above would be satisfied
        // by refusing every citable cell there is.
        assert_eq!(
            problems_with_test_tasks(MAP, &["test", "bigquery-acceptance"]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn an_on_demand_venue_needs_a_job_to_demand_the_run_and_an_in_process_one_does_not() {
        // The half that closes the two-edit path this issue measured: only `unrun` and `wired` read
        // the invocation set, so a venue nothing invokes could reach `yes` by moving its
        // `Where it runs` cell to an accepted token and then its verdict. *On demand* means a job
        // is the only thing that can demand a run, so for that run site the invocation is required.
        let found = problems_with_ci(MAP, &["test", "xtask"]);
        assert!(
            found
                .iter()
                .any(|p| p.contains("A real dataset under a shared key") && p.contains("ON DEMAND")),
            "a `yes` over an on-demand venue no job invokes has to fail: {found:?}"
        );

        // **And the direction that gets a gate disabled, which a universal rule got wrong here.**
        // This workspace's suite runs in CI as a nix CHECK, so `just test` is genuinely run by CI
        // and invisible to the invocation reader - requiring an invocation of it refused the two
        // `in process` venues, whose claims are answered on every push. Their token says so, and
        // the rule reads it: no complaint about the in-process venue with no CI at all.
        let found = problems_with_ci(MAP, &["bigquery-acceptance"]);
        assert!(
            !found.iter().any(|p| p.contains("A fake at the port")),
            "an `in process` venue is run by the suite itself and must not need an invocation: {found:?}"
        );
    }

    #[test]
    fn a_wired_cell_anchored_on_a_task_that_runs_no_test_is_refused() {
        // The shape the page recorded as review's: `wired` asked only whether CI invoked the name,
        // so an unrelated lint satisfied it. The lint is in the CI set and not in the test-task
        // set, which is exactly the state a lint is really in.
        let repointed = map_with_a_wired_cell().replace(
            "| a GitHub environment | a key | `just bigquery-acceptance` |",
            "| a GitHub environment | a key | `just lint-workflows` |",
        );
        let found = problems_with_ci(&repointed, &["lint-workflows", "test"]);
        assert!(
            found.iter().any(|p| p.contains("runs a test")),
            "a `wired` cell anchored on a lint has to fail: {found:?}"
        );
    }

    #[test]
    fn the_justfile_scan_separates_this_workspace_s_real_test_tasks_from_its_lints() {
        // The anchor without which the three arms above are satisfied by a broken scan, and there
        // is no golden here to compare against - so three oracles that are not this reader.
        // `just --list` names the recipes, `justfile` carries the `cargo nextest` lines, and the
        // page cites the two names below; the reader has to agree with all three.
        let root = crate::repo::root().expect("the repo root");
        let runs_tests = super::test_tasks(&root).expect("the justfile defines tasks that run tests");
        for task in [
            "test",
            "bigquery-acceptance",
            "bigquery-two-principals",
            "bigquery-exchanged-identity",
        ] {
            assert!(
                runs_tests.contains(task),
                "`just {task}` runs `cargo nextest run` in the justfile and the scan resolved {} \
                 task(s) without it - the reader is broken, not the justfile",
                runs_tests.len()
            );
        }
        // And the direction the whole rule exists for: a lint CI really does invoke is not a venue
        // a claim is proven in. `lint-workflows` is `nix run .#actionlint`, which is the exact task
        // the recorded bypass repointed a venue at.
        for lint in ["lint-workflows", "lint-actions", "docs", "hygiene"] {
            assert!(
                !runs_tests.contains(lint),
                "`just {lint}` runs no test and the scan counted it as one - a lint CI invokes \
                 would then anchor any citation on this page"
            );
        }
    }

    #[test]
    fn the_real_page_is_what_this_gate_is_for() {
        // The gate over the tree rather than over a fixture: the assertion that goes red when
        // somebody edits the page, which the fixtures above cannot do.
        let root = crate::repo::root().expect("the repo root");
        let (_root, files) = crate::repo::all_files()
            .and_then(|census| census.into_listing(crate::repo::Unmigrated::Venues))
            .expect("could not list the repo");
        let page = std::fs::read_to_string(root.join(PAGE)).expect(PAGE);
        let tests = super::test_names(&root, &files);
        let invoked = super::invoked(&root).expect("the CI scan reads .github/workflows");
        let runs_tests = super::test_tasks(&root).expect("the justfile defines tasks that run tests");
        assert!(tests.len() >= 100, "found {} tests - the scan is broken", tests.len());
        assert_eq!(page_problems(&page, &tests, &invoked, &runs_tests), Vec::<String>::new());
    }

    #[test]
    fn the_ci_scan_sees_the_venue_ci_actually_invokes() {
        // The anchor without which the arm above is satisfied by a broken scan. A set that found
        // nothing makes every `unrun` cell pass, and the gate's own floor only catches an empty
        // one - so this names the invocation that must be in there: `nix run .#bigquery-acceptance`
        // is the venue on this page that HAS a green run, and if the walk cannot see that one it
        // cannot see the one that matters either.
        let root = crate::repo::root().expect("the repo root");
        let invoked = super::invoked(&root).expect("the CI scan reads .github/workflows");
        assert!(
            invoked.contains("bigquery-acceptance"),
            "the CI scan resolved {} name(s) and none of them is `bigquery-acceptance`, which \
             `.github/workflows/ci.yml` invokes - the walk is broken, not the workflows",
            invoked.len()
        );
    }
}
