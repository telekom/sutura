//! What a Markdown page's own SHAPE says, held against the page rather than against a reader.
//!
//! **Why this exists: `github.com/telekom/sutura#288`.** Three branches appended to one ADR's
//! amendment list in two days, each correctly keeping to its own region, and the result was two
//! *Fifth* amendments - the second of them after the *Sixth* - plus an unnumbered one between
//! numbered ones. Every cross-reference in the tree cites those by ordinal, and two of them
//! resolved to different sections while spelling the same word. **`mkdocs build --strict` builds
//! that clean**, because a heading is prose to a renderer; so did every gate here, because a
//! sequence is not a phrase, a version or a count.
//!
//! The second rule is the other silent-rendering failure the same record already had, measured on
//! `github.com/telekom/sutura#283`: a rebase dropped the blank line above a table header and the
//! table stopped being a table - four columns of pipes rendered as one paragraph, again with a
//! green `--strict` build. It is a ratchet rather than a cleanup, and saying so is the point: the
//! tree had none of those the day it landed, so what it buys is that the next one is loud.
//!
//! **Why a module under `check-guidance` and not a gate of its own.** The theme there is *a claim
//! in prose is only as good as the thing that verifies it*, and an ordinal IS a claim - *this
//! section is the one another page cites as the seventh*. The parent already walks every published
//! `.md` file, so this costs one more pass over text it has read and no new entry point.
//!
//! # What it does not reach, before anybody trusts it
//!
//! * **Chronology.** Ordinals running consecutively says nothing about whether an amendment sits
//!   where it belongs, and nothing here reads a date. That stays a review question.
//! * **A cross-reference.** The sequence is held inside one page; a sentence elsewhere citing *the
//!   seventh amendment* is prose nothing resolves, so a renumber still has to be carried by hand.
//! * **`##` only.** A `###` amendment is a subsection of one, which is how this tree writes an
//!   addendum, and folding both levels into one sequence would number an addendum as an amendment.
//! * **A heading whose ordinal is not its first word.** `## **Second amendment**` is invisible
//!   here - the first word decides, and a decorated heading reads as no amendment at all rather
//!   than as a wrong one.
//! * **A table after a fenced block.** [`crate::markdown::prose`] blanks code, so the line above
//!   such a table reads as blank and the rule under-claims there. The same blanking is why an HTML
//!   comment above a table is not reported: the renderer still tables it, and the lexer has already
//!   emptied the line.
//! * **Which lines end a block is a MEASURED list, not a grammar.** [`ends_a_block`] names the
//!   shapes this repository's own renderer still tables, taken from it rather than from
//!   `CommonMark`; a shape nobody probed is treated as a paragraph, which reports rather than
//!   misses. The probe and its result are in that function's own documentation.

use std::path::Path;

use crate::markdown;

/// The ordinal words a heading may carry. The index is the position the word names, minus one.
///
/// Twenty because a record needing a twenty-first amendment has a bigger problem than its
/// numbering, and an unknown word is reported as *not an amendment heading* rather than guessed at.
const ORDINALS: &[&str] = &[
    "first",
    "second",
    "third",
    "fourth",
    "fifth",
    "sixth",
    "seventh",
    "eighth",
    "ninth",
    "tenth",
    "eleventh",
    "twelfth",
    "thirteenth",
    "fourteenth",
    "fifteenth",
    "sixteenth",
    "seventeenth",
    "eighteenth",
    "nineteenth",
    "twentieth",
];

/// Is this word the noun, whatever punctuation the heading writes after it?
///
/// `## Amendment, 2026-08-30:` and `## Second amendment:` both end the word in punctuation, and
/// the trailing colon or comma is the heading's, not the noun's.
fn is_amendment(word: &str) -> bool {
    word.trim_end_matches([',', ':', ';', '.']).eq_ignore_ascii_case("amendment")
}

/// What a heading claims about its place in the sequence.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Heading {
    /// `## Amendment...` - no ordinal at all.
    Unnumbered,
    /// `## Nth amendment...` - claiming that position.
    Numbered(usize),
}

/// One amendment heading, and the line a reader finds it on.
struct Found {
    /// 1-based, as [`crate::markdown::prose`] numbers them.
    line: usize,
    /// What it claims.
    heading: Heading,
}

/// The amendment a line declares, or `None` when the line is not one.
///
/// `"## "` matches level two and nothing else - `"### Amendment"` does not start with it, which is
/// the `##`-only limit in the header rather than a second check.
pub(super) fn amendment(line: &str) -> Option<Heading> {
    let mut words = line.strip_prefix("## ")?.split_whitespace();
    let first = words.next()?;
    if is_amendment(first) {
        return Some(Heading::Unnumbered);
    }
    let position = ORDINALS.iter().position(|word| first.eq_ignore_ascii_case(word))?;
    is_amendment(words.next()?).then_some(Heading::Numbered(position.saturating_add(1)))
}

/// The amendment headings of one page, in file order, against the positions they occupy.
///
/// The first may be unnumbered: records here opened their sequence before there was one, and *the
/// first amendment* is unambiguous with or without the word. Every later one must carry the ordinal
/// it actually occupies, which is the whole of the rule.
pub(super) fn sequence_problems(rel: &str, lines: &[String]) -> (Vec<String>, usize) {
    let found: Vec<Found> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            amendment(line).map(|heading| Found {
                line: index.saturating_add(1),
                heading,
            })
        })
        .collect();
    let mut problems = Vec::new();
    for (index, one) in found.iter().enumerate() {
        let position = index.saturating_add(1);
        let (line, named) = (one.line, name_of(position));
        match one.heading {
            Heading::Unnumbered if position == 1 => {}
            Heading::Unnumbered => problems.push(format!(
                "{rel}:{line}: an amendment with no ordinal, {position} of {} in file order - a reader \
                 cannot cite it, so write it as the {named} amendment",
                found.len()
            )),
            Heading::Numbered(claimed) if claimed == position => {}
            Heading::Numbered(claimed) => {
                let says = name_of(claimed);
                problems.push(format!(
                    "{rel}:{line}: reads as the {says} amendment and is the {named} in file order - a \
                     citation of either ordinal now resolves to two sections"
                ));
            }
        }
    }
    (problems, found.len())
}

/// The word for a position, past the end of [`ORDINALS`] included.
fn name_of(position: usize) -> &'static str {
    ORDINALS.get(position.saturating_sub(1)).copied().unwrap_or("later")
}

/// Does this line END a block, so that a table may start on the next one with no blank between?
///
/// **Measured against this repository's own renderer rather than reasoned about**, which is the
/// rule `check-workflows` records for a gate re-implementing part of a tool. The first version of
/// this check treated every non-blank line as a paragraph and reddened three shapes that render
/// as real tables - a heading, a thematic break, and an admonition or collapsible opener - while
/// printing *it renders as that paragraph*, a sentence that is false for all three. A ratchet that
/// reddens a correct tree is a gate somebody switches off.
///
/// What is here is exactly what `python-markdown` with `mkdocs.yml`'s extension list still tables,
/// and nothing more: a list item, a blockquote and a definition term all genuinely swallow the
/// pipes, so they stay reportable. A pipe line is the table's own continuation.
fn ends_a_block(above: &str) -> bool {
    if above.starts_with(['#', '|']) || above.starts_with("!!!") || above.starts_with("???") {
        return true;
    }
    // A thematic break: three or more of one of `-`, `*`, `_`, spaces allowed between them.
    let bare: String = above.chars().filter(|c| !c.is_whitespace()).collect();
    let mut marks = bare.chars();
    marks
        .next()
        .filter(|c| matches!(c, '-' | '*' | '_'))
        .is_some_and(|first| bare.len() >= 3 && marks.all(|c| c == first))
}

/// A table header that no blank line separates from the PARAGRAPH above it.
///
/// GFM starts a table at a line of pipes preceded by a block boundary; without one the pipes join
/// the paragraph and the whole table renders as a run-on sentence, which is what a dropped line in
/// a rebase produces and what nothing else here notices. [`ends_a_block`] carries which lines are
/// a boundary and how that was measured.
pub(super) fn table_problems(rel: &str, lines: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(previous) = index.checked_sub(1).and_then(|before| lines.get(before)) else {
            continue;
        };
        let (row, above) = (line.trim(), previous.trim());
        if row.starts_with('|') && !above.is_empty() && !ends_a_block(above) {
            problems.push(format!(
                "{rel}:{}: a table starts against the paragraph above it, so its pipes join that \
                 paragraph instead of rendering as a table - put a blank line before the header row",
                index.saturating_add(1)
            ));
        }
    }
    problems
}

/// What one sweep read, for the caller to print.
///
/// Returned rather than folded into a message, because a floor nobody can see is a floor nobody
/// checks: `check-guidance`'s success line states these, the way `check-api-links` states its own.
pub(super) struct PageCounts {
    /// Markdown pages in the caller's list, counted BEFORE the loop's own filter.
    pub(super) offered: usize,
    /// Pages this actually lexed.
    pub(super) read: usize,
    /// Amendment headings found on them.
    pub(super) headings: usize,
}

/// Both rules, over every Markdown page in scope.
///
/// FAIL CLOSED on a page that cannot be lexed, for [`crate::api_links`]'s reason: an unclosed fence
/// makes every line below it ambiguous, and a scan that reads nothing reports nothing wrong.
///
/// **Three floors, and each is a pair of numbers taken from a different place**, because one number
/// in one message is what a narrowed walk moves along with itself:
///
/// * `read` against `offered`. `offered` is counted over the caller's WHOLE list by a different
///   predicate than the loop's, which is the point: a filter narrowed inside the loop moves `read`
///   and leaves `offered` where it was. Counting both off one filter is how a scan restricted to
///   one directory passed at exit 0 with a real defect outside it.
/// * The sequence rule's own walk against this one. `sequence_problems` returns the length of the
///   vector it iterated, and it is compared against a count taken here - so a walk truncated inside
///   that function is a mismatch rather than a shorter list of problems.
/// * `headings` over the tree. This tree writes amendment headings, so zero means the rule stopped
///   reading rather than that they went - a blanking lexer or a `"## "` prefix that stopped
///   matching leaves both equalities above intact and this at zero.
pub(super) fn page_problems(root: &Path, files: &[String]) -> (Vec<String>, PageCounts) {
    let offered = files.iter().filter(|f| f.to_ascii_lowercase().ends_with(".md")).count();
    let mut problems = Vec::new();
    let mut read = 0_usize;
    let mut headings = 0_usize;
    for rel in files.iter().filter(|f| super::has_ext(f, &["md"])) {
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            problems.push(format!("{rel}: could not be read, so nothing on it was judged"));
            continue;
        };
        let lines = match markdown::prose(&text) {
            Ok(lines) => lines,
            Err(why) => {
                problems.push(format!("{rel}: could not be lexed, so nothing on it was read - {why}"));
                continue;
            }
        };
        read = read.saturating_add(1);
        let on_page = lines.iter().filter(|line| amendment(line).is_some()).count();
        headings = headings.saturating_add(on_page);
        let (sequence, walked) = sequence_problems(rel, &lines);
        problems.extend(sequence);
        if walked != on_page {
            problems.push(format!(
                "{rel}: the sequence rule walked {walked} amendment heading(s) where the page holds \
                 {on_page} - it judged less than it read, so its verdict covers less than it appears to"
            ));
        }
        problems.extend(table_problems(rel, &lines));
    }
    if read != offered {
        problems.push(format!(
            "read {read} of {offered} Markdown page(s) - the rest were skipped, so this verdict covers \
             less than it appears to"
        ));
    }
    if headings == 0 {
        problems.push(format!(
            "found no amendment heading on any of {read} page(s) - this tree writes them, so the \
             sequence rule read nothing rather than finding nothing"
        ));
    }
    (problems, PageCounts { offered, read, headings })
}
