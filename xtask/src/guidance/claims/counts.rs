//! A number in prose, derived from the tree rather than recounted by hand.
//!
//! **Split out of `claims.rs` under the 1000-line cap, and the seam is the one the parent's own
//! header already named: `claims` and `counts` are two checks that share nothing but the flattened
//! view a claim and a count are both found in.** `CONTRADICTED` grows by ENTRY - one claim is about
//! twenty lines - so the file holding it is the one that has to have room, which is what this move
//! bought.
//!
//! The tests for what is here are in the parent's `tests` module, beside the table they read - the
//! same reason `remedies` gives: a file that adds no `#[test]` is one the causality gate may
//! revert, so moving them here would take this module's own declaration with them and orphan them.
//!
//! Why the check exists: `39 SQL goldens read LIMIT 10001` was written when there were 39 and
//! stayed written at 63, and no reviewer notices that twice. What makes it a gate rather than a
//! walk of the tree is that **both sides are read** - the count is derived from the tree, and at
//! least one page has to state it, because a number nobody writes down is compared to nothing.

use std::path::Path;

use super::super::matches_any;
use super::flatten;

/// What one file contributes to a count: itself, or every occurrence in it.
///
/// A required field on [`Counted`] rather than a default, because the wrong answer is SILENTLY
/// green. This used to be a sentence in `holds`'s documentation - *"files, not occurrences"* - and
/// a sentence is held by recall: the row-cap entry wants files and says why, and the `pub trait`
/// entry below is right at file granularity only for as long as no two declarations share a file.
/// A closed enum matched exhaustively is what makes the next entry answer the question instead of
/// inheriting an answer.
#[derive(Clone, Copy)]
pub(super) enum Granularity {
    /// One per file, however often the literal appears in it.
    Files,
    /// Every occurrence, non-overlapping - what `grep -o` counts.
    Occurrences,
}

impl Granularity {
    /// What one file's `text` contributes to the total.
    pub(super) fn count_in(self, text: &str, holds: &str) -> u64 {
        match self {
            Self::Files => u64::from(text.contains(holds)),
            // Saturating rather than `as`: a `usize` count cannot exceed `u64::MAX` on any target
            // this builds for, and the cast lint is on for the case where that reasoning is wrong.
            Self::Occurrences => u64::try_from(text.matches(holds).count()).unwrap_or(u64::MAX),
        }
    }
}

/// A number in prose that counts something in the tree.
///
/// The third shape of the same idea. [`Pin`](super::super::Pin) reads its value from a line in a file; this one
/// DERIVES it by counting, which is the only honest way to hold a number nobody is going to
/// recount by hand. `39 SQL goldens read LIMIT 10001` was written when there were 39 and stayed
/// written when there were 63, and no reviewer is going to notice that twice.
pub(in crate::guidance) struct Counted {
    /// Human name, for the message.
    pub(super) name: &'static str,
    /// Files to count over. Globs against repo-relative paths.
    pub(super) over: &'static [&'static str],
    /// The literal that is counted. Whether a second one in the same file counts is
    /// [`Counted::granularity`]'s question, not this field's.
    pub(super) holds: &'static str,
    /// Files, or occurrences within them.
    pub(super) granularity: Granularity,
    /// Where the number may be stated. **At least one file here has to state it**, or the entry
    /// counts the tree and compares it to nothing - see [`count_mismatches`].
    pub(super) mentioned_in: &'static [&'static str],
    /// The noun phrase the number belongs to. The count is the integer IMMEDIATELY BEFORE it.
    ///
    /// Deliberately that narrow, for the reason [`contradicts`](super::super::contradicts) is narrow: the sentence carrying
    /// `39 SQL goldens read LIMIT 10001` also carries `10001`, and a check that read every number
    /// on the line would report the row cap as a wrong golden count.
    ///
    /// Found in the FLATTENED view and at every occurrence - see [`stated_numbers`], which records
    /// the two ways the per-line predecessor of this field read agreement out of prose it had not
    /// managed to look at.
    pub(super) marker: &'static str,
}

/// TWO entries, and the table is short on purpose: a number is worth a gate when it carries an
/// ARGUMENT.
///
/// The row cap's does - it says the SQL leg is pinned across the corpus rather than in one
/// snapshot, and `39` was written when there were 39 and read as current at 63. The `pub trait`
/// one is a COMPARISON: the port table in `.agents/skills/engineering/rust/SKILL.md` lists what
/// the hexagon's interior owns, the tree holds more than that, and the gap is the whole point of
/// the paragraph - which a reader cannot see without both numbers.
///
/// Two numbers that are NOT here, both deliberately:
///
/// * `docs/crap.md` said `its 120 unit tests` at a real 170. DELETED from the prose rather than
///   gated, because it carried no argument - the sentence is about tests living in the crate they
///   cover, true at any count - and gating it would fail every branch that adds a test.
/// * *how many crates declare `[features]`* - proposed as an entry, declined for the same reason.
///   The sentence it would serve is *the `--all-features` habit is load-bearing now rather than
///   cheap foresight*, which is true at any count above zero, so the number adds nothing the word
///   *several* does not carry. The direction that would falsify it - no crate declaring a feature
///   at all - is held by the `no crate declares a feature` entry in [`CONTRADICTED`](super::CONTRADICTED), whose
///   evidence is a manifest holding `[features]`.
pub(in crate::guidance) const COUNTS: &[Counted] = &[
    Counted {
        name: "SQL goldens carrying the row cap",
        // EVERY crate's snapshot directory, not just the one that had them when this entry was
        // written. `sutura-cli`'s 22 rendered-statement goldens carry the row cap and were outside
        // the glob, so the sentence said *pinned across the corpus* over 93 of 115 files and 22
        // goldens could have lost the cap with the gate silent. The claim is about the corpus, so
        // the glob is too - and a new crate's snapshots are inside it the day they land.
        over: &["crates/*/tests/snapshots/**"],
        holds: "LIMIT 10001",
        // FILES, and here that is a decision rather than the default it used to be: what is
        // claimed is how many goldens carry the row cap, and a golden carrying it twice is still
        // one golden.
        granularity: Granularity::Files,
        // `.agents/skills/**` is listed because that is where the sentence LIVES now. It was in
        // `AGENTS.md` until the router rewrite moved the invariants table into the tree, and this
        // list did not follow - so the only gated count in the repo was left counting the corpus
        // and comparing it to nothing. `PINS` was carried and this was not, which is this module's
        // own founding cause one layer up: the correction landed on one table and not its sibling.
        // The stated-side check is what makes the next one of those a failure.
        mentioned_in: &[".agents/skills/**", "AGENTS.md", "docs/**", "README.md"],
        marker: "SQL goldens read",
    },
    Counted {
        name: "`pub trait` declarations under `crates/*/src`",
        over: &["crates/*/src/**/*.rs"],
        holds: "pub trait ",
        // OCCURRENCES, and this is the entry that could not be held honestly before. Every
        // declaration sits in a file of its own today, so counting files would report the right
        // number BY COINCIDENCE and go green on the wrong one the day two of them share a file.
        //
        // The literal is the one the page cites, character for character, so the gate and the
        // command a reader is told to run answer the same question - a `pub trait ` inside a
        // comment or a string counts for both. Blanking comments would make the gate disagree with
        // the page, which is worse than the mention it would exclude.
        granularity: Granularity::Occurrences,
        mentioned_in: &[".agents/skills/**", "AGENTS.md", "CONTRIBUTING.md", "docs/**", "README.md"],
        marker: "`pub trait` declarations under",
    },
];

/// The integer at the end of `head`, if it ends in one.
fn trailing_number(head: &str) -> Option<u64> {
    let mut digits: Vec<char> = head.trim_end().chars().rev().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    digits.reverse();
    digits.into_iter().collect::<String>().parse().ok()
}

/// Every statement of a number before `marker` in `text`: the line it starts on, and the number.
///
/// **Flattened, not per line, and that is a fix rather than a refinement.** This was
/// `count_before(line, marker)`, which had two blind spots a reviewer could not see from the
/// output because both of them look exactly like agreement: prose WRAPS, so a statement whose
/// number ended one line above its marker was not found at all; and `line.find` reads the FIRST
/// marker on a line, so a second statement on the same line was never read. Both are closed by
/// asking the flattened view for every occurrence - the same view [`contradicted_claims`](super::contradicted_claims) has
/// used from the start, for the same reason.
///
/// **What was rejected, because it would have failed correct prose.** The other candidate fix was
/// to require every page NAMING the marker to carry a number. `docs/implementation-plan-bigquery.md`
/// names `SQL goldens read` in order to explain the gate, with no number and correctly so, and a
/// gate that fails that sentence is one somebody switches off. Reading every occurrence needs no
/// exemption list.
pub(super) fn stated_numbers(text: &str, marker: &str) -> Vec<(usize, u64)> {
    let (flat, lines) = flatten(text);
    let mut found = Vec::new();
    let mut from = 0_usize;
    while let Some(at) = flat.get(from..).and_then(|rest| rest.find(marker)) {
        let offset = from.saturating_add(at);
        if let Some(head) = flat.get(..offset)
            && let Some(number) = trailing_number(head)
        {
            found.push((lines.get(offset).copied().unwrap_or(1), number));
        }
        from = offset.saturating_add(marker.len().max(1));
    }
    found
}

/// What the tree holds, at the granularity this entry declares.
pub(super) fn tally(root: &Path, all: &[String], counted: &Counted) -> u64 {
    let mut total = 0_u64;
    for rel in all {
        if !matches_any(counted.over, rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        total = total.saturating_add(counted.granularity.count_in(&text, counted.holds));
    }
    total
}

/// One place a count is written down: where, and what it says.
pub(super) struct Stated {
    /// Repo-relative path of the page stating it.
    file: String,
    /// One-based line, so a message names the line a reader will open.
    line: usize,
    /// The number that page states.
    number: u64,
}

/// Every place this entry's number is actually written.
///
/// One implementation of *where is this number stated*, because the mismatch check and the
/// vacuity check are two questions about one answer and a second walk of the tree would be a
/// second thing to keep in step.
pub(super) fn statements(root: &Path, files: &[String], counted: &Counted) -> Vec<Stated> {
    let mut found = Vec::new();
    for rel in files {
        if !matches_any(counted.mentioned_in, rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        for (line, number) in stated_numbers(&text, counted.marker) {
            found.push(Stated {
                file: rel.clone(),
                line,
                number,
            });
        }
    }
    found
}

pub(in crate::guidance) fn count_mismatches(root: &Path, all: &[String], files: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    for counted in COUNTS {
        let actual = tally(root, all, counted);
        if actual == 0 {
            // Zero means the thing counted moved, not that the prose is right. A count check that
            // silently agreed with nothing would pass vacuously, which is worse than failing.
            problems.push(format!(
                "nothing in the tree matches the {} count (`{}` under {:?}) - the count moved, \
                 so this entry in COUNTS is measuring nothing",
                counted.name, counted.holds, counted.over
            ));
            continue;
        }
        let stated = statements(root, files, counted);
        for page in &stated {
            if page.number != actual {
                problems.push(format!(
                    "{}:{}: says {} {} - the tree has {actual}",
                    page.file, page.line, page.number, counted.name
                ));
            }
        }
        if stated.is_empty() {
            // The OTHER way this check goes vacuously green, and the one that actually happened:
            // the count is measured, nothing states it, and the gate agrees with silence. The
            // row-cap sentence lived in `AGENTS.md` until the router rewrite carried the
            // invariants table into `.agents/skills/`; `mentioned_in` did not follow, and a gate
            // that had caught `39` at 63 was left holding nothing. Failing here is the only way
            // that is visible, because a deleted sentence looks exactly like a correct one.
            problems.push(format!(
                "nothing under {:?} states the {} count - the number is derived and compared to \
                 nothing, so this entry in COUNTS is a gate over silence. State it before the \
                 marker `{}`, or delete the entry",
                counted.mentioned_in, counted.name, counted.marker
            ));
        }
    }
    problems
}
