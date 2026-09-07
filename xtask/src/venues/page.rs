//! What the PAGE is, as against what it must hold.
//!
//! The same distinction `acceptance`'s `shape` draws against its `properties`, one file over: this
//! module says what a venues row, a claims cell, a verdict word and a `##` section ARE, and
//! `super::page_problems` spends those answers on the page's rules. Nothing here decides anything.
//!
//! Split out when `venues.rs` reached the unexemptable 1000-line gate - nothing under `xtask/` may
//! be listed in `devco/max-lines-ignore` - and split THIS way for the reason
//! `.agents/skills/sutura/gates` states: the causality gate never reverts a file that adds a
//! `#[test]`, so moving the harness keeps every assertion in the file that already had them and
//! nothing is orphaned.
//!
//! **What that costs the causality gate, measured rather than predicted.** An earlier version of
//! this paragraph said the base tree would fail to build and the verdict would be `INCONCLUSIVE`.
//! It is not: `venues.rs` carries implementation AND tests, so the plan is
//! `NotSeparable { files: ["xtask/src/venues.rs"] }`, this file is never reverted, and no base
//! build is attempted at all. The verdict is `NOT MECHANICALLY SEPARABLE`. Either way a mutation
//! stands in for the proof - but the two verdicts ask for different things, and describing one as
//! the other is how an author fails to recognise their own situation.

use std::collections::BTreeSet;

use super::PAGE;

/// The venues table's header, matched whole so a fourth column cannot silently change what the
/// last cell means.
const VENUES_HEADER: &str = "| Venue | Where it runs | What it costs | Reached by |";

/// The claims matrix's header start; the rest of the row is the venue columns, compared not fixed.
const CLAIMS_HEADER: &str = "| Claim |";

/// The `##` sections that are not a venue. An allowlist, so a venue section that is not a row
/// stops being invisible and a fourth structural section is a deliberate edit here.
pub(super) const STRUCTURAL: &[&str] = &["The venues", "Which venue answers which claim", "Keeping this page honest"];

/// Every verdict a cell may state, longest first so `only here` is not read as two words. `-` is
/// *not answered here*, `redundant` *could and need not*, `can` *capable, and the standing test is
/// in another venue*, `unrun` *the standing test is HERE and nothing has run it*, `wired` *and now
/// something reaches it, and no run has been observed*, `only here` the column's reason to exist.
/// Anything else is a sentence, and a sentence is what the matrix is there instead of.
///
/// **Neither `unrun` nor `wired` is a softer `can`.** A `can` cell points at evidence somewhere
/// else; those two point at none. Only `yes` and `can` may be cited for a claim, and `unrun`,
/// `wired` and those two are what count as a venue having a reason to be in the table - see
/// [`page_problems`].
///
/// **`wired` is not a softer `unrun` either, and the two are exclusive by mechanism rather than by
/// convention:** `unrun` is refused once CI invokes the venue's `Reached by` task and `wired` is
/// refused until it does, so exactly one of them is available for any given tree.
///
/// **What that exclusivity rests on, said where the claim is:** the `Reached by` cell, which is
/// PROSE ON THE SAME PAGE and editable in the same diff. Three readers narrow what it can get away
/// with. [`Venue::runs_nowhere`] refuses `yes`, `can` and `wired` from a venue whose own row says it
/// runs nowhere; a built venue whose cell names no resolvable invocation is refused outright; and
/// the CI scan now reads a COMMAND rather than a substring. None of them makes the cell ground
/// truth, and review is still what decides that a row describes the thing it names.
pub(super) const VERDICTS: &[&str] = &["only here", "redundant", "unrun", "wired", "yes", "can", "no", "-"];

/// What a venue that runs nowhere says in its `Reached by` cell.
pub(super) const NOT_BUILT: &str = "not built";

/// The word a venue's `Where it runs` cell uses when it runs nowhere.
pub(super) const NOWHERE: &str = "nowhere";

/// Every place a venue may say it runs, longest first for [`VERDICTS`]' reason.
///
/// **A CLOSED vocabulary, one review after [`VERDICTS`] became one, and for the same cell's sake.**
/// [`Venue::runs_nowhere`] matched the SUBSTRING `nowhere` in free prose, and that predicate is the
/// only remaining guard on `yes` for the row carrying leg 2. Measured on the merged tree at
/// `d5bd307`, one edit, page restored after: spell `nowhere yet` as `not anywhere yet`, point the
/// exchange venue's `Reached by` at a real CI-invoked lint, and *whether two subjects read two
/// different row sets* publishes **yes** at exit 0. With the word kept, exit 1. A free-prose cell
/// cannot be the guard on a citation, so a cell stating none of these is refused.
///
/// **What a vocabulary does NOT reach, stated where the claim is:** it holds that a cell means
/// something, never that it is TRUE. `in process, every run` written on the exchange venue passes
/// this - and that is the same limit the page already states beside `Reached by`, where pointing a
/// row at an unrelated CI-invoked app is review's to catch. What is closed is the SYNONYM, which is
/// the form that reads as a copy-edit rather than as a claim.
pub(super) const RUN_SITES: &[&str] = &["a github environment", "in process", NOWHERE];

/// One row of the venues table.
pub(super) struct Venue {
    /// The name, with the emphasis removed.
    pub(super) name: String,
    /// The `Where it runs` cell.
    ///
    /// Read since a review found the two columns able to disagree: the exchange venue said
    /// *nowhere yet* in this cell while its `Reached by` named a task, and only the second was
    /// read, so `is_built` was true and the sole guard on `yes`/`can` for the row carrying leg 2
    /// was gone. Two facts the gate already had, now both read.
    pub(super) runs: String,
    /// The `Reached by` cell.
    pub(super) reached: String,
}

impl Venue {
    /// Which of [`RUN_SITES`] this row's `Where it runs` cell states, or `None` for a sentence.
    pub(super) fn site(&self) -> Option<&'static str> {
        stated(&self.runs, RUN_SITES)
    }

    /// Does the row say this venue runs nowhere?
    ///
    /// **Separate from [`Self::is_built`] on purpose, and the difference is a real state:** a venue
    /// can have a written test and a task that reaches it and still run nowhere, because nothing
    /// can point that task at anything. That is exactly what `unrun` is for - so this does not
    /// refuse `unrun`, and it does refuse every verdict that claims a run happened or is about to.
    ///
    /// Through [`Self::site`] rather than a `contains`, because a substring of free prose is a
    /// guard one synonym turns off - see [`RUN_SITES`] for the measurement.
    pub(super) fn runs_nowhere(&self) -> bool {
        self.site() == Some(NOWHERE)
    }

    /// Does anything run this venue?
    pub(super) fn is_built(&self) -> bool {
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
pub(super) fn venues(text: &str) -> Result<Vec<Venue>, String> {
    let (_, rows) = table(text, "venues", |line| line == VENUES_HEADER)?;
    let mut out = Vec::new();
    for row in rows {
        let [name, runs, .., reached] = row.as_slice() else {
            return Err(format!("{PAGE}: a venues row has fewer cells than the header: {row:?}"));
        };
        let name = name.replace('*', "").trim().to_owned();
        if name.is_empty() {
            return Err(format!("{PAGE}: a venues row names no venue"));
        }
        out.push(Venue {
            name,
            runs: runs.clone(),
            reached: reached.clone(),
        });
    }
    Ok(out)
}

/// The venue name with a leading article dropped and case flattened, so a table row and its own
/// section heading can be compared without demanding the same wording of both.
pub(super) fn key(name: &str) -> String {
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

/// Which word of `vocabulary` this cell states, or `None` when it states something else.
///
/// One normalization for the two closed vocabularies on this page, because *a cell states a word
/// from a list* was about to be written twice - and the copy is what decides whether `**nowhere**`
/// and `nowhere` are the same answer.
fn stated(cell: &str, vocabulary: &[&'static str]) -> Option<&'static str> {
    let normalized = cell.replace(['*', '_'], "").trim().to_lowercase();
    vocabulary.iter().copied().find(|word| states(&normalized, word))
}

/// The verdict a cell states, or `None` when it states something else.
pub(super) fn verdict(cell: &str) -> Option<&'static str> {
    stated(cell, VERDICTS)
}

/// Every test name the page cites: a backticked `snake_case` identifier long enough not to be a
/// field or a type. Bounded deliberately - a short `kid` or `aud` in backticks is prose.
pub(super) fn cited_tests(text: &str) -> BTreeSet<String> {
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
pub(super) fn section_body(text: &str, venue: &str) -> Option<String> {
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

/// The claims matrix: its header row, which IS the venue list, and one row per claim.
///
/// A named reader rather than [`table`] plus [`CLAIMS_HEADER`] at the call site, because the seam
/// is *what the page is*: a caller that had to be handed the header constant to read one table
/// would be spelling this module's job in `super`.
pub(super) fn claims(text: &str) -> Result<Table, String> {
    table(text, "claims", |line| line.starts_with(CLAIMS_HEADER))
}
