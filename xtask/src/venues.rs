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
//!   which put the difference exactly where nothing reads it, and left *the change that carries
//!   the first green run moves this cell* held by recall.
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
//! citation in a page and a second owner would be a second answer; what is read here is only
//! whether the cell says `not built`, because that decides what the column may claim.

use std::collections::BTreeSet;
use std::path::Path;

use crate::Verdict;
use crate::repo;

// The acceptance venue's half. Its limit is a property of `.github/workflows/ci.yml` rather than of
// the page, so it reads a different file with a different parser - and every assertion about it
// lives beside that parser, where the fixture it needs is.
mod acceptance;

/// The map. One page: a second copy of a venue table is the drift this gate is about.
const PAGE: &str = "docs/where-identity-is-proven.md";

/// The venues table's header, matched whole so a fourth column cannot silently change what the
/// last cell means.
const VENUES_HEADER: &str = "| Venue | Where it runs | What it costs | Reached by |";

/// The claims matrix's header start; the rest of the row is the venue columns, compared not fixed.
const CLAIMS_HEADER: &str = "| Claim |";

/// The `##` sections that are not a venue. An allowlist, so a venue section that is not a row
/// stops being invisible and a fourth structural section is a deliberate edit here.
const STRUCTURAL: &[&str] = &["The venues", "Which venue answers which claim", "Keeping this page honest"];

/// Every verdict a cell may state, longest first so `only here` is not read as two words. `-` is
/// *not answered here*, `redundant` *could and need not*, `can` *capable, and the standing test is
/// in another venue*, `unrun` *the standing test is HERE and nothing has run it*, `only here` the
/// column's reason to exist. Anything else is a sentence, and a sentence is what the matrix is
/// there instead of.
///
/// **`unrun` is not a softer `can`.** A `can` cell points at evidence somewhere else; an `unrun`
/// cell points at none. Only `yes` and `can` may be cited for a claim, and only `unrun` and those
/// two count as a venue having a reason to be in the table - see [`page_problems`].
const VERDICTS: &[&str] = &["only here", "redundant", "unrun", "yes", "can", "no", "-"];

/// What a venue that runs nowhere says in its `Reached by` cell.
const NOT_BUILT: &str = "not built";

/// One row of the venues table.
struct Venue {
    /// The name, with the emphasis removed.
    name: String,
    /// The `Reached by` cell.
    reached: String,
}

impl Venue {
    /// Does anything run this venue?
    fn is_built(&self) -> bool {
        self.reached != NOT_BUILT
    }
}

/// The cells of a markdown table row, emphasis kept and whitespace trimmed.
fn cells(row: &str) -> Vec<String> {
    let inner = row.trim().trim_start_matches('|').trim_end_matches('|');
    inner.split('|').map(|cell| cell.trim().to_owned()).collect()
}

/// Is this line the separator under a table header?
fn is_separator(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|') && trimmed.contains("---")
}

/// The header cells and the rows under the one header line `matches` accepts, or why the table
/// could not be read.
///
/// The HEADER is returned as well as the rows because in the claims matrix it is the venue list -
/// reading the first data row as the header is how a comparison ends up between a claim's cells
/// and itself, which is what this returned separately.
///
/// FAILS CLOSED in the four ways a text scan goes quiet: no header, two headers, a header with no
/// separator row - markdown renders such a table as one paragraph, so the page would read as prose
/// while this gate read an empty table - and no rows at all.
type Table = (Vec<String>, Vec<Vec<String>>);

fn table(text: &str, what: &str, matches: impl Fn(&str) -> bool) -> Result<Table, String> {
    let mut lines = text.lines().peekable();
    let mut found: Option<Table> = None;

    while let Some(line) = lines.next() {
        if !matches(line.trim()) {
            continue;
        }
        if found.is_some() {
            return Err(format!("{PAGE} has two {what} tables - which one is the map?"));
        }
        let header = cells(line);
        match lines.next() {
            Some(separator) if is_separator(separator) => {}
            _ => return Err(format!("{PAGE}: the {what} table has no separator row")),
        }
        let mut rows = Vec::new();
        while let Some(row) = lines.next_if(|next| next.trim().starts_with('|')) {
            rows.push(cells(row));
        }
        found = Some((header, rows));
    }

    match found {
        Some((header, rows)) if !rows.is_empty() => Ok((header, rows)),
        Some(_) => Err(format!("{PAGE}: the {what} table has no rows")),
        None => Err(format!("{PAGE} has no {what} table - the map is not stated")),
    }
}

/// The venues table, or why it could not be read.
fn venues(text: &str) -> Result<Vec<Venue>, String> {
    let (_, rows) = table(text, "venues", |line| line == VENUES_HEADER)?;
    let mut out = Vec::new();
    for row in rows {
        let [name, .., reached] = row.as_slice() else {
            return Err(format!("{PAGE}: a venues row has fewer cells than the header: {row:?}"));
        };
        let name = name.replace('*', "").trim().to_owned();
        if name.is_empty() {
            return Err(format!("{PAGE}: a venues row names no venue"));
        }
        out.push(Venue {
            name,
            reached: reached.clone(),
        });
    }
    Ok(out)
}

/// The venue name with a leading article dropped and case flattened, so a table row and its own
/// section heading can be compared without demanding the same wording of both.
fn key(name: &str) -> String {
    let lower = name.to_lowercase();
    for article in ["a ", "an ", "the "] {
        if let Some(rest) = lower.strip_prefix(article) {
            return rest.to_owned();
        }
    }
    lower
}

/// Is `word` the verdict this cell states, rather than a prefix of some other word?
fn states(normalized: &str, word: &str) -> bool {
    let Some(rest) = normalized.strip_prefix(word) else {
        return false;
    };
    rest.is_empty() || rest.starts_with([' ', ',', '-', '(', '.', ';', ':'])
}

/// The verdict a cell states, or `None` when it states something else.
fn verdict(cell: &str) -> Option<&'static str> {
    let normalized = cell.replace(['*', '_'], "").trim().to_lowercase();
    VERDICTS.iter().copied().find(|word| states(&normalized, word))
}

/// Every test name the page cites: a backticked `snake_case` identifier long enough not to be a
/// field or a type. Bounded deliberately - a short `kid` or `aud` in backticks is prose.
fn cited_tests(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in text.lines() {
        for span in line.split('`').skip(1).step_by(2) {
            let name = span.trim();
            let shaped = name.len() >= 16
                && name.matches('_').count() >= 3
                && name.starts_with(|c: char| c.is_ascii_lowercase())
                && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            if shaped {
                out.insert(name.to_owned());
            }
        }
    }
    out
}

/// The body of one venue's `##` section: every line from its heading to the next `## `.
///
/// `None` when no heading matches, which is a separate failure the section check already reports -
/// so this returning `None` cannot make an `unrun` cell pass. Bounded at the next `## ` rather than
/// read to the end of the page, because a check satisfied by the word appearing ANYWHERE below is
/// the dead-gate shape this file's own header is about.
fn section_body(text: &str, venue: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|line| {
        line.strip_prefix("## ")
            .is_some_and(|heading| key(heading.trim()) == key(venue))
    })?;
    Some(
        lines
            .iter()
            .skip(start.saturating_add(1))
            .take_while(|line| !line.starts_with("## "))
            .copied()
            .collect::<Vec<&str>>()
            .join(
                "
",
            ),
    )
}

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

/// Everything wrong with the page.
fn page_problems(text: &str, tests: &BTreeSet<String>) -> Vec<String> {
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

    let (header, claims) = match table(text, "claims", |line| line.starts_with(CLAIMS_HEADER)) {
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
                Some("unrun") => {
                    claimed.insert(venue.name.as_str());
                    unrun.insert(venue.name.as_str());
                    if !venue.is_built() {
                        problems.push(format!(
                            "{PAGE}: `{}` is `{NOT_BUILT}` and says `unrun` about `{claim}` - \
                             `unrun` is a test that EXISTS and has not been run, so a venue \
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
                 `can` or `unrun`. Either it answers a claim and the column does not say so, a \
                 test is written for it and the cell should say `unrun`, or the row has no reason \
                 to be there",
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

    // A venue whose cell says `unrun` has to say it in its own SECTION too, in that word. The
    // exclusion a reader needs is *no run has happened*, and a section that explains a `can` in
    // prose is how the two drifted apart before this verdict existed.
    for venue in &listed {
        if !unrun.contains(venue.name.as_str()) {
            continue;
        }
        if !section_body(text, &venue.name).is_some_and(|body| body.contains("unrun")) {
            problems.push(format!(
                "{PAGE}: `{}` says `unrun` in the matrix and its own section never uses that word \
                 - the cell is where the state is read and the section is where it is explained, \
                 and a section that explains it in other words is how the two came apart",
                venue.name
            ));
        }
    }

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

    let mut problems = page_problems(&page, &tests);
    problems.extend(acceptance::problems(&workflow));

    if problems.is_empty() {
        println!(
            "xtask check-venues: ok - {PAGE} and the `{}` job, {} cited test(s)",
            acceptance::JOB,
            cited_tests(&page).len()
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

    use super::{PAGE, VERDICTS, page_problems, verdict};

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

    fn problems(page: &str) -> Vec<String> {
        page_problems(page, &known())
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
    }

    #[test]
    fn the_real_page_is_what_this_gate_is_for() {
        // The gate over the tree rather than over a fixture: the assertion that goes red when
        // somebody edits the page, which the fixtures above cannot do.
        let root = crate::repo::root().expect("the repo root");
        let crate::repo::RepoFiles { files, .. } = crate::repo::all_files().expect("could not list the repo");
        let page = std::fs::read_to_string(root.join(PAGE)).expect(PAGE);
        let tests = super::test_names(&root, &files);
        assert!(tests.len() >= 100, "found {} tests - the scan is broken", tests.len());
        assert_eq!(page_problems(&page, &tests), Vec::<String>::new());
    }
}
