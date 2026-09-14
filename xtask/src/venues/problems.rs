//! Everything wrong with the page, read against the tree - the seam every other module in `venues`
//! feeds.
//!
//! The fifth split this file has taken at the unexemptable 1000-line gate - nothing under `xtask/`
//! may be listed in `devco/max-lines-ignore` - and the same reason as the four before it:
//! `page`, `rows`, `verdicts` and `sources` each say what a PART of the page must hold; this is
//! where those answers are spent, over the actual table, matrix and sections. `venues.rs` keeps
//! `run()`, the module wiring and every `#[test]` - the causality gate never reverts a file that
//! adds a `#[test]`, so moving the harness here would orphan it and read as green-against-base.
//! Only the orchestrator moved.

use std::collections::BTreeSet;

use super::PAGE;
use super::page::{NOT_BUILT, STRUCTURAL, VERDICTS, cited_tests, claims, key, venues, verdict};
use super::pairing;
use super::rows::row_problems;
use super::verdicts::{anchor_problems, transition_problems, yes_problems};

/// Everything wrong with the page.
pub(super) fn page_problems(
    text: &str,
    tests: &BTreeSet<String>,
    invoked: &BTreeSet<String>,
    runs_tests: &BTreeSet<String>,
) -> Vec<String> {
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

    problems.extend(pairing::problems(&listed, &columns));

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
    // The subset of `citable` that states the word `yes` rather than `can` - `verdicts::yes_problems`
    // ties this word specifically to its venue's own section; a `can` cell points at evidence that
    // lives in ANOTHER venue's section, so nothing here is what that rule would read.
    let mut yes: BTreeSet<&str> = BTreeSet::new();
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
                    if word == "yes" {
                        yes.insert(venue.name.as_str());
                    }
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
    problems.extend(yes_problems(text, &listed, &yes));

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
