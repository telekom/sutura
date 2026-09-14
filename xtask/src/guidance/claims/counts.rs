//! A number in prose, derived from the tree rather than recounted by hand.
//!
//! **Split out of `claims.rs` under the 1000-line cap, and the seam is the one the parent's own
//! header already named: `claims` and `counts` are two checks that share nothing but the flattened
//! view a claim and a count are both found in.** `CONTRADICTED` grows by ENTRY - one claim is about
//! twenty lines - so the file holding it is the one that has to have room, which is what this move
//! bought and then ran out of: the table has since moved to `claims/contradicted.rs`, and the
//! parent's own note there records why a reserve measured in spare lines expires.
//!
//! The tests for what is here are in the parent's `tests` module - **not** beside the table they
//! read, which is where `remedies`' tests are and is the half of that sentence that stopped being
//! true when the table moved with the check. The reason they stay is the other half, and it is
//! unchanged: a file that adds no `#[test]` is one the causality gate may revert, so moving them
//! here would take this module's own declaration with them and orphan them.
//!
//! Why the check exists: `39 SQL goldens read LIMIT 10001` was written when there were 39 and
//! stayed written at 63, and no reviewer notices that twice. What makes it a gate rather than a
//! walk of the tree is that **both sides are read** - the count is derived from the tree, and at
//! least one page has to state it, because a number nobody writes down is compared to nothing.

use std::path::Path;

use super::flatten;
use crate::repo::matches_any;

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
/// The second shape of the same idea, and the surviving one: the version shape read its value
/// from a line in a file and was deleted, because a rule that refuses the copy cannot also
/// require one to compare. This one
/// DERIVES its value by counting, which is the only honest way to hold a number nobody is going to
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
    /// Text at or after this marker is excluded from the count. Empty counts the whole file.
    ///
    /// The seam a file can share between production code and a test fixture that repeats the same
    /// literal on purpose: `crates/sutura-http/src/wire/refusal.rs`'s own `#[cfg(test)]` module
    /// asserts each `422` arm's status against the literal it just matched, so
    /// `grep -c StatusCode::UNPROCESSABLE_ENTITY` reports fourteen where six production arms
    /// reach it. **The limit next to that claim: this is a text cut, not a parse.** A second
    /// `#[cfg(test)]` string earlier in the file - inside a doc comment explaining this very
    /// mechanism, say - would truncate there instead, so this field wants the boundary to be
    /// unique in the file it is set for.
    pub(super) stop_before: &'static str,
    /// Files, or occurrences within them.
    pub(super) granularity: Granularity,
    /// Where the number may be stated. **At least one file here has to state it**, or the entry
    /// counts the tree and compares it to nothing - see [`count_mismatches`]. Matched against the
    /// GUIDANCE-scoped listing, same as every other prose check - see [`also_stated_in`](Counted::also_stated_in)
    /// for the one exception this table needs.
    pub(super) mentioned_in: &'static [&'static str],
    /// Exact paths - not the doc/config glob [`mentioned_in`](Counted::mentioned_in) reads - where this number may ALSO be
    /// stated in Rust.
    ///
    /// `.rs` is out of the guidance scope everywhere else in this gate, for the reason
    /// `guidance::in_scope` gives: a `CONTRADICTED` wording registered in Rust would contain the
    /// very phrase it forbids. A `Counted` marker carries no forbidden sentence, so that risk is
    /// absent here - but the scope is still widened by naming exact files rather than a blanket
    /// `.rs` glob, so a new source file does not silently become a place this number may be
    /// asserted without anyone deciding that. Empty means no Rust file states it.
    ///
    /// The other half of that trade: a `.rs` site not named here is not read AT ALL, so a wrong
    /// number stated in a file that never earned a place on this list passes as silently as one
    /// that carries no number ever does. Naming a file here is what turns it into a place the
    /// gate compares to the tree - the file listing itself decides nothing.
    pub(super) also_stated_in: &'static [&'static str],
    /// The noun phrase the number belongs to. The count is the integer IMMEDIATELY BEFORE it.
    ///
    /// Deliberately that narrow, for the reason the `versions` check reads only the token
    /// IMMEDIATELY beside a pinned name: the sentence carrying
    /// `39 SQL goldens read LIMIT 10001` also carries `10001`, and a check that read every number
    /// on the line would report the row cap as a wrong golden count.
    ///
    /// Found in the FLATTENED view and at every occurrence - see [`stated_numbers`], which records
    /// the two ways the per-line predecessor of this field read agreement out of prose it had not
    /// managed to look at.
    pub(super) marker: &'static str,
}

/// The table is short on purpose: a number is worth a gate when it carries an ARGUMENT.
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
///   at all - is held by the `no crate declares a feature` entry in [`CONTRADICTED`](crate::guidance::claims::CONTRADICTED), whose
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
        stop_before: "",
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
        also_stated_in: &[],
        marker: "SQL goldens read",
    },
    Counted {
        name: "`pub trait` declarations under `crates/*/src`",
        over: &["crates/*/src/**/*.rs"],
        holds: "pub trait ",
        stop_before: "",
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
        also_stated_in: &[],
        marker: "`pub trait` declarations under",
    },
    Counted {
        name: "needles the compose tier's wait gate keys on",
        // The gate's own module, because the table IS the definition. There is nowhere else for
        // the number to come from, and that is what makes it derivable rather than recounted.
        over: &["xtask/src/bounded_wait.rs"],
        // The constructor, not the type name: `Blocks` and `Needle` both appear in that module's
        // prose and in its tests, and this count reads raw text, so a literal its own prose can
        // contribute to is a number nobody can predict. Measured when this landed: a comment over
        // there naming the constructor made it `8` against 7 rows, and the gate said so.
        holds: "Needle::new(",
        stop_before: "",
        // OCCURRENCES: every row is one needle, and they share a file by construction.
        granularity: Granularity::Occurrences,
        mentioned_in: &[".agents/skills/**", "AGENTS.md", "docs/**"],
        also_stated_in: &[],
        marker: "needles the wait gate keys on",
    },
    Counted {
        name: "refusal reasons that land on `422`",
        // github.com/telekom/sutura#603, #670, #676. Three prior fixes each hand-corrected the
        // number in every prose site they could find and missed at least one: #667 moved four of
        // the six sites from four to five and left `docs/serving.md`'s own `422 - where four of
        // them land` untouched in `routes/v1/query.rs`; #661 then added a sixth arm
        // (`DeadlineExceeded`) an hour fifty-eight minutes later and every site still said five.
        // Registering the new number a fourth time would be the same fix again - so this entry
        // reads the arms instead.
        over: &["crates/sutura-http/src/wire/refusal.rs"],
        holds: "StatusCode::UNPROCESSABLE_ENTITY",
        // Six is `refused`'s own match; fourteen is `grep -c` on the whole file, because
        // `every_reason()` in `#[cfg(test)]` below repeats the literal once per case it asserts -
        // #676's own "counting these by grep is a trap".
        stop_before: "#[cfg(test)]",
        granularity: Granularity::Occurrences,
        // The two Rust sites are `also_stated_in`, not here: `.rs` is outside `mentioned_in`'s
        // scope everywhere in this gate. The doc/config side reads the same glob every sibling
        // entry does, not just `docs/serving.md`: narrowed to one path, a fifth site stating this
        // number in the exact registered wording anywhere else under `docs/**` would be unread,
        // even though the claim scope already covers it.
        mentioned_in: &[".agents/skills/**", "AGENTS.md", "docs/**", "README.md"],
        also_stated_in: &[
            "crates/sutura-http/src/wire/refusal.rs",
            "crates/sutura-http/src/routes/v1/query.rs",
        ],
        marker: "refusal reasons land on `422`",
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
/// asking the flattened view for every occurrence - the same view [`contradicted_claims`](crate::guidance::claims::contradicted_claims) has
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
///
/// `unreadable` for the reason this whole check exists one level down: a file this cannot open
/// lowers the tally, and a lowered tally is compared against the prose as if it were the tree. The
/// zero floor in [`count_mismatches`] catches losing EVERY file and nothing catches losing one.
pub(super) fn tally(root: &Path, all: &[String], counted: &Counted, unreadable: &mut Vec<String>) -> u64 {
    let mut total = 0_u64;
    for rel in all {
        if !matches_any(counted.over, rel) {
            continue;
        }
        let Some(text) = crate::repo::read_subject(root, rel, unreadable) else {
            continue;
        };
        // Everything at or after `stop_before` is a test fixture repeating the same literal on
        // purpose, not a second production instance - see the field's own doc for the measured
        // case (six arms, fourteen occurrences).
        let counted_text = if counted.stop_before.is_empty() {
            text.as_str()
        } else {
            text.split(counted.stop_before).next().unwrap_or(&text)
        };
        total = total.saturating_add(counted.granularity.count_in(counted_text, counted.holds));
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

/// Every place this entry's number is actually written, among `files` matching `patterns`.
///
/// One implementation of *where is this number stated*, because the mismatch check and the
/// vacuity check are two questions about one answer and a second walk of the tree would be a
/// second thing to keep in step. `patterns` is a parameter rather than always
/// `counted.mentioned_in`, because [`mismatches_for`] asks this same question twice - once over
/// the doc/config scope and once over [`Counted::also_stated_in`]'s named Rust files - and the two
/// scopes are different lists filtered from different starting sets.
pub(super) fn statements(
    root: &Path,
    files: &[String],
    patterns: &[&'static str],
    counted: &Counted,
    unreadable: &mut Vec<String>,
) -> Vec<Stated> {
    let mut found = Vec::new();
    for rel in files {
        if !matches_any(patterns, rel) {
            continue;
        }
        let Some(text) = crate::repo::read_subject(root, rel, unreadable) else {
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

/// One entry's mismatches: the tree's actual count against every place `counted` says it is
/// stated - `mentioned_in`'s docs and config, plus `also_stated_in`'s named Rust files.
///
/// Split out of [`count_mismatches`] so a test can exercise one entry against a fixture tree
/// without depending on [`COUNTS`] or the real repo - the shape `an_unreadable_subject_in_scope…`
/// already uses for the checks beside this one.
pub(super) fn mismatches_for(root: &Path, all: &[String], files: &[String], counted: &Counted, problems: &mut Vec<String>) {
    let actual = tally(root, all, counted, problems);
    if actual == 0 {
        // Zero means the thing counted moved, not that the prose is right. A count check that
        // silently agreed with nothing would pass vacuously, which is worse than failing.
        problems.push(format!(
            "nothing in the tree matches the {} count (`{}` under {:?}) - the count moved, so \
             this entry in COUNTS is measuring nothing",
            counted.name, counted.holds, counted.over
        ));
        return;
    }
    let mut stated = statements(root, files, counted.mentioned_in, counted, problems);
    if !counted.also_stated_in.is_empty() {
        stated.extend(statements(root, all, counted.also_stated_in, counted, problems));
    }
    for page in &stated {
        if page.number != actual {
            problems.push(format!(
                "{}:{}: says {} {} - the tree has {actual}",
                page.file, page.line, page.number, counted.name
            ));
        }
    }
    if stated.is_empty() {
        // The OTHER way this check goes vacuously green, and the one that actually happened: the
        // count is measured, nothing states it, and the gate agrees with silence. The row-cap
        // sentence lived in `AGENTS.md` until the router rewrite carried the invariants table
        // into `.agents/skills/`; `mentioned_in` did not follow, and a gate that had caught `39`
        // at 63 was left holding nothing. Failing here is the only way that is visible, because a
        // deleted sentence looks exactly like a correct one.
        problems.push(format!(
            "nothing under {:?} (or {:?}) states the {} count - the number is derived and \
             compared to nothing, so this entry in COUNTS is a gate over silence. State it \
             before the marker `{}`, or delete the entry",
            counted.mentioned_in, counted.also_stated_in, counted.name, counted.marker
        ));
    }
}

pub(in crate::guidance) fn count_mismatches(root: &Path, all: &[String], files: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    for counted in COUNTS {
        mismatches_for(root, all, files, counted, &mut problems);
    }
    problems
}
