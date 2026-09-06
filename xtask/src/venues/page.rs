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
//! nothing is orphaned. **The expected cost is a verdict**: this file adds no test, so it is
//! revertible while the file declaring `mod page;` is held, and the base tree cannot build. That
//! reads `INCONCLUSIVE`, which is the gate being honest, and a mutation stands in for it.

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
pub(super) const VERDICTS: &[&str] = &["only here", "redundant", "unrun", "wired", "yes", "can", "no", "-"];

/// What a venue that runs nowhere says in its `Reached by` cell.
pub(super) const NOT_BUILT: &str = "not built";

/// One row of the venues table.
pub(super) struct Venue {
    /// The name, with the emphasis removed.
    pub(super) name: String,
    /// The `Reached by` cell.
    pub(super) reached: String,
}

impl Venue {
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

/// The verdict a cell states, or `None` when it states something else.
pub(super) fn verdict(cell: &str) -> Option<&'static str> {
    let normalized = cell.replace(['*', '_'], "").trim().to_lowercase();
    VERDICTS.iter().copied().find(|word| states(&normalized, word))
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
