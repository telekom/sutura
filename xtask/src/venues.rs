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
//! * **A test that is WRITTEN and has never run says `unrun`, and its venue's section has to use
//!   that word.** The verdict exists because `can` was carrying two states that a reader cannot
//!   tell apart and a citation must: *the standing test lives in another venue* and *the standing
//!   test lives here and nothing has run it*. The second is not evidence for anything, and the
//!   first is. Before this, a venue in that state had to say `can` and explain itself in prose -
//!   which put the difference exactly where nothing reads it.
//! * **A venue CI INVOKES may not say `unrun`**, which is the half review found missing and the
//!   reason [`invoked`] exists. The three page-shaped rules above hold a spelling; not one of them
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
use std::path::Path;

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

use page::{NOT_BUILT, STRUCTURAL, VERDICTS, Venue, cited_tests, claims, key, section_body, venues, verdict};

/// The map. One page: a second copy of a venue table is the drift this gate is about.
const PAGE: &str = "docs/where-identity-is-proven.md";

/// Every test function in the workspace, by name.
///
/// A test rather than any function, because the page's claim is that these names ARE the standing
/// tests: a helper renamed into one of them would satisfy an existence check and prove nothing.
fn test_names(root: &Path, files: &[String]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let rust = files
        .iter()
        .filter(|f| Path::new(f).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("rs")));
    for rel in rust {
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if !(trimmed.starts_with("#[") && trimmed.contains("test")) {
                continue;
            }
            for ahead in lines.iter().skip(i.saturating_add(1)).take(4) {
                let candidate = ahead.trim().trim_start_matches("pub ").trim_start_matches("async ");
                if let Some(rest) = candidate.strip_prefix("fn ")
                    && let Some(name) = rest.split('(').next()
                {
                    out.insert(name.trim().to_owned());
                    break;
                }
            }
        }
    }
    out
}

/// The task or app name one invocation names, or `None` when the span is not an invocation.
///
/// `just bigquery-two-principals` and `nix run .#bigquery-two-principals` both resolve to
/// `bigquery-two-principals`, which is what makes a page citing the TASK comparable against a
/// workflow invoking the APP. That the two names coincide is this repository's convention and not a
/// derived fact - a task whose flake app is named differently would not be seen, and that is a
/// false NEGATIVE, which is the direction this check can afford.
fn invocation(span: &str) -> Option<&str> {
    let rest = span
        .strip_prefix("nix run .#just -- ")
        .or_else(|| span.strip_prefix("nix run .#"))
        .or_else(|| span.strip_prefix("just "))?;
    let name: &str = rest.split_whitespace().next()?;
    let name = name.trim_end_matches(&[',', '.', ';', ':'][..]);
    let shaped = !name.is_empty()
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    shaped.then_some(name)
}

/// Every `just` task and `nix run .#` app CI invokes, by bare name.
///
/// **Read out of the same three places `crate::workflows` scans, through the same walk**, because
/// the recorded way a reference leaves a gate's sight is a step moving out of a workflow - and a
/// second walk here would be a second answer to *where does CI invoke things from*.
///
/// A comment is not an invocation, matching `crate::workflows::collect`'s own rule: `ci.yml` and
/// `docs.yml` both discuss tasks in prose, and a gate that read those would refuse an `unrun` cell
/// because somebody explained the job in a comment.
///
/// `None` only when the scan itself is broken, which its caller turns into a failure rather than an
/// empty set: a set that found nothing would make every `unrun` cell pass, and *a scan that passes
/// by finding nothing* is the failure mode a text gate is most prone to here.
fn invoked(root: &Path) -> Option<BTreeSet<String>> {
    let read = crate::workflows::sources::ci_sources(root)?;
    let mut out = BTreeSet::new();
    for source in &read {
        for line in source.text.lines() {
            if line.trim_start().starts_with('#') {
                continue;
            }
            for needle in ["just ", "nix run .#"] {
                let mut rest = line;
                while let Some(at) = rest.find(needle) {
                    let from = rest.get(at..).unwrap_or_default();
                    if let Some(name) = invocation(from) {
                        out.insert(name.to_owned());
                    }
                    rest = rest.get(at.saturating_add(needle.len())..).unwrap_or_default();
                }
            }
        }
    }
    Some(out)
}

/// The task and app names a `Reached by` cell cites, by bare name.
///
/// Backticked spans only, because that is how the page writes an invocation and because the cell's
/// prose ("a GitHub environment, on demand") would otherwise contribute words.
fn cited_invocations(reached: &str) -> BTreeSet<String> {
    reached
        .split('`')
        .skip(1)
        .step_by(2)
        .filter_map(|span| invocation(span.trim()))
        .map(str::to_owned)
        .collect()
}

/// Everything the two un-citable verdicts have to be consistent with, for the venues stating them.
///
/// Together rather than beside the other page rules, because these are the whole difference
/// between `unrun`/`wired` being tokens and being caveats, and they fail in opposite directions:
/// one catches a cell whose section never explains it, the others a cell that has stopped being
/// true - in either direction, since `unrun` expires when CI reaches a venue and `wired` is not yet
/// earned until it does.
fn transition_problems(
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

/// Everything wrong with the page.
fn page_problems(text: &str, tests: &BTreeSet<String>, invoked: &BTreeSet<String>) -> Vec<String> {
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
                    if !venue.is_built() {
                        problems.push(format!(
                            "{PAGE}: `{}` is `{NOT_BUILT}` and claims `{word}` about `{claim}` - a \
                             venue nothing runs cannot be cited for anything, which is the \
                             overstatement this page exists to prevent",
                            venue.name
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

    problems.extend(transition_problems(text, &listed, &unrun, &wired, invoked));

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
    let Some(repo::RepoFiles { root, files }) = repo::all_files() else {
        eprintln!("xtask check-venues: could not determine the repo root");
        return Verdict::Fail;
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

    let mut problems = page_problems(&page, &tests, &invoked);
    problems.extend(acceptance::problems(&workflow));
    let contract = environment::problems(&root);
    problems.extend(contract.problems);

    if problems.is_empty() {
        println!(
            "xtask check-venues: ok - {PAGE} and the `{}` job, {} cited test(s), {} CI invocation(s) resolved, {} environment name(s) reconciled across three lists",
            acceptance::JOB,
            cited_tests(&page).len(),
            invoked.len(),
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

    use super::{PAGE, VERDICTS, invocation, page_problems, verdict};

    /// A page with two venues too few is refused, so the fixtures below carry three.
    const MAP: &str = "\
## The venues

| Venue | Where it runs | What it costs | Reached by |
| --- | --- | --- | --- |
| **A fake at the port** | in process | nothing | `just test` |
| **A real dataset under a shared key** | an environment | a key | `just bigquery-acceptance` |
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

    /// A CI that invokes nothing the fixture map cites, so every assertion below is about the page.
    fn invokes_nothing() -> BTreeSet<String> {
        std::iter::once("xtask".to_owned()).collect()
    }

    fn problems(page: &str) -> Vec<String> {
        page_problems(page, &known(), &invokes_nothing())
    }

    fn problems_with_ci(page: &str, invoked: &[&str]) -> Vec<String> {
        let invoked: BTreeSet<String> = invoked.iter().map(|name| (*name).to_owned()).collect();
        page_problems(page, &known(), &invoked)
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
        assert_eq!(problems(&map_with_an_unrun_cell()), Vec::<String>::new());
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
            problems_with_ci(&map_with_an_unrun_cell(), &["deny", "crap", "xtask"]),
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
            problems_with_ci(&map_with_a_wired_cell(), &["bigquery-acceptance"]),
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
        let cited = super::cited_invocations("`just test`, `just validate` - not on a fork");
        assert_eq!(cited, ["test".to_owned(), "validate".to_owned()].into_iter().collect());
        assert!(super::cited_invocations("not built").is_empty());
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
        assert_eq!(verdict("**wired** - a job reaches it and no run has been seen"), Some("wired"));
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
    fn the_real_page_is_what_this_gate_is_for() {
        // The gate over the tree rather than over a fixture: the assertion that goes red when
        // somebody edits the page, which the fixtures above cannot do.
        let root = crate::repo::root().expect("the repo root");
        let crate::repo::RepoFiles { files, .. } = crate::repo::all_files().expect("could not list the repo");
        let page = std::fs::read_to_string(root.join(PAGE)).expect(PAGE);
        let tests = super::test_names(&root, &files);
        let invoked = super::invoked(&root).expect("the CI scan reads .github/workflows");
        assert!(tests.len() >= 100, "found {} tests - the scan is broken", tests.len());
        assert_eq!(page_problems(&page, &tests, &invoked), Vec::<String>::new());
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
