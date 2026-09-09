//! Markdown lexing for the gates that read published pages.
//!
//! **A gate that scans a language must lex it**, and the rule this holds is the one
//! `.agents/skills/sutura/gates/SKILL.md` records from `check-workflows` counting braces over
//! `flake.nix`: blank the non-code half before counting anything, and make an unclosed block an
//! ERROR rather than an answer.
//!
//! The scanner this replaces toggled one boolean on any line beginning with three backticks or
//! three tildes, recording neither the delimiter nor its length. Three consequences, and the
//! first was reproduced on a real page:
//!
//! * a NESTED fence - a four-backtick block containing a three-backtick one, which is how
//!   `docs/publishing.md` documents mermaid - inverted the flag for the rest of the page, so
//!   every link below it went unread and a genuine dead link passed the gate;
//! * a tilde line closed a backtick block, because the two were one delimiter;
//! * a fence still open at end of file produced an answer instead of a failure.
//!
//! What is lexed, and nothing more: fenced code blocks, HTML comments, and inline code spans.
//! [`prose`] returns one entry per SOURCE line, so a line number still means something once the
//! non-prose half is blanked, and it returns [`Unlexable`] rather than a line list when a block
//! is still open at the end.
//!
//! **Both halves, off one walk, and that is what makes the sharing worth having.** [`code`] is
//! [`prose`]'s complement - the interior of every fenced block, with the page around it blanked -
//! because `github.com/telekom/sutura#301` was the same parity toggle in a gate that wants the
//! code rather than the prose. Two functions over one [`Half`] rather than two state machines:
//! whichever half a caller asks for, the delimiter it closes on, the nesting it survives and the
//! unclosed block it refuses over are decided in one place.

use std::fmt;

/// A fenced block's opening delimiter.
struct Fence {
    /// `` ` `` or `~`. A fence closes only on its own character.
    marker: char,
    /// How many of it. A fence closes only on a run at least this long.
    len: usize,
    /// Where it opened, 1-based, for the message.
    line: usize,
}

/// A block this cannot lex, and therefore refuses to answer over.
///
/// Both arms are the same decision: an unclosed block makes every line after it ambiguous, and a
/// scanner that guesses reports either a link that is code or no link at all. Neither is worth
/// having behind a green verdict.
pub(crate) enum Unlexable {
    /// A fenced code block opened and nothing closed it.
    Fence {
        /// The delimiter character, for the message.
        marker: char,
        /// How long the opening run was.
        len: usize,
        /// Where it opened, 1-based.
        line: usize,
    },
    /// An HTML comment opened and nothing closed it.
    Comment {
        /// Where it opened, 1-based.
        line: usize,
    },
}

impl fmt::Display for Unlexable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Fence { marker, len, line } => {
                let run: String = std::iter::repeat_n(marker, len).collect();
                write!(
                    f,
                    "line {line} opens a `{run}` fence that no later line closes - every line below it is either code or prose and this cannot say which, so this scan refuses rather than guessing. Close the fence, or lengthen the outer one if a fence is nested"
                )
            }
            Self::Comment { line } => write!(
                f,
                "line {line} opens an HTML comment that no later line closes - everything below it would be swallowed, so this scan refuses rather than reading none of it"
            ),
        }
    }
}

/// The fence a line opens, if it opens one.
///
/// **Indentation is deliberately not restricted**, where `CommonMark` allows an opening fence at
/// most three columns in. mkdocs-material's admonitions take their body four columns in, so a
/// fenced block inside one opens at column four and mkdocs renders it - the strict rule would read
/// a documented command as prose. The direction is the safe one: treating more of the page as code
/// cannot invent a link, and the unclosed check makes a fence opened by accident loud.
///
/// **[`closes`] diverges the same way, and it is the same decision rather than a second one** -
/// review of `github.com/telekom/sutura#301` reported it as an unspoken divergence, which it was.
/// `CommonMark` allows a closing fence at most three columns in; a fence OPENED four columns in
/// inside an admonition is closed four columns in, so restricting the closer while leaving the
/// opener free would leave that block open to end of file and turn a rendered page into an
/// [`Unlexable`]. Over-closing is the safe half of the trade for both callers: it ends a block
/// early rather than swallowing the page.
///
/// **Measured, so the reason is not overstated:** `grep -rnE '^\s{4,}```' docs` matches nothing
/// on 2026-09-05, and six pages carry an admonition with no fence inside one. So both divergences
/// are for a shape mkdocs renders and this tree does not yet write - a design choice, not a
/// workaround for a page that exists.
fn opens(line: &str, number: usize) -> Option<Fence> {
    let trimmed = line.trim_start();
    let marker = trimmed.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let len = trimmed.chars().take_while(|c| *c == marker).count();
    if len < 3 {
        return None;
    }
    // A backtick fence's info string may not contain a backtick, which is what makes
    // ```` ```mermaid ```` inside a four-backtick block content rather than a second fence.
    let info = trimmed.get(len..)?.trim();
    if marker == '`' && info.contains('`') {
        return None;
    }
    Some(Fence {
        marker,
        len,
        line: number,
    })
}

/// Does this line close `fence`? Only its own character, only a run at least as long, and
/// nothing else on the line - an info string makes it another opener, not a closer.
fn closes(line: &str, fence: &Fence) -> bool {
    let trimmed = line.trim_start();
    let len = trimmed.chars().take_while(|c| *c == fence.marker).count();
    len >= fence.len && trimmed.get(len..).is_some_and(|rest| rest.trim().is_empty())
}

/// Do the characters at `at` spell `needle`?
fn spells(chars: &[char], at: usize, needle: &str) -> bool {
    chars
        .get(at..)
        .is_some_and(|tail| tail.iter().take(needle.chars().count()).copied().eq(needle.chars()))
}

/// Where the next run of EXACTLY `len` backticks starts, at or after `from`.
///
/// Exactly, because that is what closes a code span in `CommonMark`: a two-backtick span may
/// contain a single backtick, and a single-backtick span may not close on a longer run.
fn closing_run(chars: &[char], from: usize, len: usize) -> Option<usize> {
    let mut at = from;
    while at < chars.len() {
        if chars.get(at) == Some(&'`') {
            let run = chars
                .get(at..)
                .map_or(0, |tail| tail.iter().take_while(|c| **c == '`').count());
            if run == len {
                return Some(at);
            }
            at = at.saturating_add(run);
            continue;
        }
        at = at.saturating_add(1);
    }
    None
}

/// One line with its HTML comments and inline code spans removed, and whether a comment is still
/// open at the end of it.
///
/// An HTML comment is carried across lines because that is the shape this tree writes - the two
/// snippet stubs explain themselves in a nine-line comment. An inline code span is line-local on
/// purpose: an UNMATCHED backtick run is literal text in `CommonMark`, so treating it as an opening
/// delimiter would swallow the rest of the page, which is the fail-open direction. What that
/// gives up is a code span wrapped across two lines, which reads as prose and can therefore be a
/// false positive rather than a miss.
fn without_spans(line: &str, in_comment: bool) -> (String, bool) {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut inside = in_comment;
    let mut at = 0_usize;
    while at < chars.len() {
        if inside {
            if spells(&chars, at, "-->") {
                inside = false;
                at = at.saturating_add(3);
            } else {
                at = at.saturating_add(1);
            }
            continue;
        }
        if spells(&chars, at, "<!--") {
            inside = true;
            at = at.saturating_add(4);
            continue;
        }
        if chars.get(at) == Some(&'`') {
            let run = chars
                .get(at..)
                .map_or(0, |tail| tail.iter().take_while(|c| **c == '`').count());
            if let Some(close) = closing_run(&chars, at.saturating_add(run), run) {
                at = close.saturating_add(run);
                continue;
            }
            out.extend(std::iter::repeat_n('`', run));
            at = at.saturating_add(run);
            continue;
        }
        if let Some(c) = chars.get(at) {
            out.push(*c);
        }
        at = at.saturating_add(1);
    }
    (out, inside)
}

/// Which half of a page a caller wants.
///
/// The two are complements over one walk, so a caller cannot get the fence rules of one and the
/// content of the other. A fence DELIMITER is content in neither: an opening line carries an info
/// string and a closing line carries nothing, and neither is a line of the block's language.
#[derive(Clone, Copy)]
enum Half {
    /// What a renderer shows as text: code blocks, HTML comments and code spans blanked.
    Prose,
    /// The interior of every fenced code block, with the page around it blanked.
    Code,
}

/// The prose of `text`: one entry per source line, with every fenced code block, HTML comment and
/// inline code span blanked.
///
/// Blanked rather than dropped so a line number is still a line number, and owned rather than
/// borrowed because blanking a span rewrites the line - the alternative is a second pass over
/// the same text by every caller.
pub(crate) fn prose(text: &str) -> Result<Vec<String>, Unlexable> {
    lex(text, Half::Prose)
}

/// One inline link's destination, and the source line it was written on.
///
/// The line number is why this exists rather than a bare `&str`: a finding on a generated page
/// seven thousand lines long has to name the line, and the destination alone cannot.
pub(crate) struct Destination<'a> {
    /// The source line, 1-based, as [`prose`] numbers them.
    pub(crate) line: usize,
    /// What is between `](` and the matching `)`, trimmed and as written.
    pub(crate) text: &'a str,
}

/// Where the destination starting at `after` ends: the first `)` at nesting depth zero.
///
/// **Parentheses are BALANCED rather than stopped at.** `CommonMark` allows them inside a
/// destination, and taking the first `)` turned `](crate::plan::run())` into the destination
/// `crate::plan::run(` - which is how a gate over these destinations missed the call form
/// entirely while mkdocs published the whole thing. Measured in review of
/// `github.com/telekom/sutura#361`.
///
/// `None` when the run never closes on this line. That is not a link at all in `CommonMark` -
/// an inline link needs its `)` - and inventing one that runs to end of line is what this
/// returned before, so a malformed link became a destination nobody wrote.
fn destination_end(after: &str) -> Option<usize> {
    let mut depth = 0_usize;
    for (at, c) in after.char_indices() {
        match c {
            '(' => depth = depth.saturating_add(1),
            ')' if depth == 0 => return Some(at),
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    None
}

/// Every inline link destination in `lines`, in source order.
///
/// ONE owner for the `](` walk, because two gates want it: `check-docs` resolves a destination
/// against the pages on disk, and `check-api-links` asks what mkdocs would do with it. It reads
/// the PROSE half, so a page documenting a link inside a fence is not making one.
///
/// A destination is returned as written, title and all - `](a.md 'titled')` is one destination of
/// `a.md 'titled'`, and what to do about the title is the caller's rule rather than this one's.
/// [`destination_end`] carries the one place this is not literal.
pub(crate) fn destinations(lines: &[String]) -> Vec<Destination<'_>> {
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let number = index.saturating_add(1);
        let mut rest = line.as_str();
        while let Some(at) = rest.find("](") {
            let after = rest.get(at.saturating_add(2)..).unwrap_or("");
            let Some(end) = destination_end(after) else {
                break;
            };
            if let Some(text) = after.get(..end) {
                found.push(Destination {
                    line: number,
                    text: text.trim(),
                });
            }
            rest = after.get(end..).unwrap_or("");
        }
    }
    found
}

/// The fenced code of `text`: one entry per source line, with everything outside a fenced block
/// blanked, the delimiters included.
///
/// The reader for a gate that judges what a page tells somebody to RUN. It carries every property
/// [`prose`] has and needs each one: `github.com/telekom/sutura#301`'s defect was a nested fence
/// inverting a boolean, which both lost a declaration and read a MENTION below the block as one.
///
/// An HTML comment is blanked here too. A commented-out command is not an instruction, and a
/// renderer shows neither it nor the fence markers inside it.
pub(crate) fn code(text: &str) -> Result<Vec<String>, Unlexable> {
    lex(text, Half::Code)
}

/// One walk, one set of fence rules, and `half` decides only which lines keep their content.
fn lex(text: &str, half: Half) -> Result<Vec<String>, Unlexable> {
    let mut out: Vec<String> = Vec::new();
    let mut fence: Option<Fence> = None;
    let mut comment: Option<usize> = None;
    for (index, line) in text.lines().enumerate() {
        let number = index.saturating_add(1);
        if let Some(open) = fence.as_ref() {
            let closing = closes(line, open);
            if closing {
                fence = None;
            }
            out.push(match half {
                Half::Code if !closing => String::from(line),
                Half::Code | Half::Prose => String::new(),
            });
            continue;
        }
        // A fence cannot open inside a comment, and a comment cannot open inside a fence: the
        // outer state wins, or the two would interleave into a third thing neither renderer has.
        if comment.is_none()
            && let Some(opened) = opens(line, number)
        {
            fence = Some(opened);
            out.push(String::new());
            continue;
        }
        let (visible, still_open) = without_spans(line, comment.is_some());
        if still_open {
            comment = comment.or(Some(number));
        } else {
            comment = None;
        }
        // Walked for both halves whatever is kept, because the comment state decides whether the
        // NEXT line can open a fence, and an unclosed one is an error on either side.
        out.push(match half {
            Half::Prose => visible,
            Half::Code => String::new(),
        });
    }
    if let Some(open) = fence {
        return Err(Unlexable::Fence {
            marker: open.marker,
            len: open.len,
            line: open.line,
        });
    }
    if let Some(line) = comment {
        return Err(Unlexable::Comment { line });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{Unlexable, code, destinations, prose};

    fn dests(text: &str) -> Vec<String> {
        let lines = prose(text).unwrap_or_else(|e| panic!("{e}"));
        destinations(&lines)
            .into_iter()
            .map(|d| format!("{}:{}", d.line, d.text))
            .collect()
    }

    #[test]
    fn a_destination_is_read_whole_including_its_parentheses() {
        // THE REVIEWED DEFECT: stopping at the first `)` yielded `a::b(`, and a gate keyed on
        // the whole destination then judged a shape mkdocs publishes in full.
        assert_eq!(dests("see [x](a::b())\n"), ["1:a::b()"]);
        assert_eq!(dests("see [x](a.md)\n"), ["1:a.md"]);
        assert_eq!(dests("see [x](a_(b).md)\n"), ["1:a_(b).md"]);
        // Two links on one line, each ending at its own closer.
        assert_eq!(dests("[a](one.md) and [b](two.md)\n"), ["1:one.md", "1:two.md"]);
        // A title travels with the destination - the caller's rule, not this one's.
        assert_eq!(dests("[a](b.md 'why')\n"), ["1:b.md 'why'"]);
    }

    #[test]
    fn an_unclosed_destination_is_not_a_link_rather_than_a_destination_to_end_of_line() {
        assert!(dests("see [x](a.md\n").is_empty(), "an unclosed destination is not a link");
        assert!(
            dests("see [x](a(b.md\n").is_empty(),
            "an unclosed braced destination is not a link"
        );
        // And the line after it is still read.
        assert_eq!(dests("see [x](a.md\n[b](c.md)\n"), ["2:c.md"]);
    }

    #[test]
    fn a_destination_inside_a_fence_is_not_a_link() {
        assert!(
            dests("```\n[shown](a.md)\n```\n").is_empty(),
            "a destination inside a fence is not a link"
        );
        assert_eq!(dests("```\n[shown](a.md)\n```\n[real](b.md)\n"), ["4:b.md"]);
    }

    fn lines(text: &str) -> Vec<String> {
        prose(text).unwrap_or_else(|e| panic!("{e}"))
    }

    fn coded(text: &str) -> Vec<String> {
        code(text).unwrap_or_else(|e| panic!("{e}"))
    }

    /// The two halves are complements, and a DELIMITER belongs to neither.
    ///
    /// Asserted as a partition rather than as two independent expectations, because the way a
    /// second reader of one page goes wrong is by disagreeing with the first about one line -
    /// which is what `github.com/telekom/sutura#301` was one level up.
    #[test]
    fn the_code_half_and_the_prose_half_partition_the_page() {
        let page = "intro\n```bash\nrun me\n```\nafter\n";
        assert_eq!(coded(page), vec!["", "", "run me", "", ""]);
        assert_eq!(lines(page), vec!["intro", "", "", "", "after"]);
        // Every line is claimed by at most one half, so a fence marker is in neither.
        for (from_code, from_prose) in coded(page).iter().zip(lines(page).iter()) {
            assert!(from_code.is_empty() || from_prose.is_empty(), "{from_code} / {from_prose}");
        }
    }

    /// A nested fence keeps its content as CODE, and the page below it stays prose.
    ///
    /// The `github.com/telekom/sutura#301` shape read from the code side: a parity toggle loses
    /// the inner block's content and then reads the line after the outer close as code.
    #[test]
    fn a_nested_fence_is_content_and_the_page_below_it_is_not() {
        let page = "````text\n```bash\nrun me\n````\nafter\n";
        assert_eq!(coded(page), vec!["", "```bash", "run me", "", ""]);
        // A tilde block's interior is code, and a backtick line inside it does not close it.
        assert_eq!(coded("~~~\n```\nrun me\n~~~\n"), vec!["", "```", "run me", ""]);
    }

    /// An unclosed block is an error on the code side too, for [`Unlexable`]'s own reason.
    #[test]
    fn the_code_half_refuses_an_unclosed_block_as_well() {
        let Err(Unlexable::Fence { line, .. }) = code("intro\n```bash\nrun me\n") else {
            panic!("an unclosed fence must not produce a line list");
        };
        assert_eq!(line, 2);
        // A fence inside an HTML comment opens nothing, so the comment is what is unclosed.
        let Err(Unlexable::Comment { line }) = code("<!-- why\n```\n") else {
            panic!("an unclosed comment must not produce a line list");
        };
        assert_eq!(line, 1);
    }

    /// A commented-out command is not an instruction, on either side.
    #[test]
    fn a_fenced_block_inside_an_html_comment_is_not_code() {
        assert_eq!(
            coded("<!--\n```bash\nrun me\n```\n-->\nafter\n"),
            vec!["", "", "", "", "", ""]
        );
    }

    #[test]
    fn a_fence_closes_only_on_its_own_delimiter_at_its_own_length() {
        // THE REPRODUCED BUG. A four-backtick block documenting a three-backtick one: the inner
        // markers are content, and the link after the block is prose. A parity toggle read the
        // inner opener as a close, the inner close as an open, and the link as code.
        let page = "````text\n```mermaid\ngraph LR\n```\n````\n[real](plan.md)\n";
        assert_eq!(lines(page).last().map(String::as_str), Some("[real](plan.md)"));
        // A tilde run does not close a backtick block, and the reverse.
        assert_eq!(lines("```\n~~~\n```\n[real](plan.md)\n").len(), 4);
        assert!(
            lines("~~~\n```\n~~~\n[real](plan.md)\n")[1].is_empty(),
            "the middle line between unrelated fences stays empty"
        );
        // A longer closing run is allowed; a shorter one is not.
        assert_eq!(lines("```\ncode\n`````\n[real](plan.md)\n")[3], "[real](plan.md)");
    }

    #[test]
    fn a_block_left_open_is_an_error_rather_than_an_answer() {
        let Err(Unlexable::Fence { marker, len, line }) = prose("intro\n```rust\nfn main() {}\n") else {
            panic!("an unclosed fence must not produce a line list");
        };
        assert_eq!((marker, len, line), ('`', 3, 2));
        let Err(Unlexable::Comment { line }) = prose("intro\n<!-- why\nmore\n") else {
            panic!("an unclosed comment must not produce a line list");
        };
        assert_eq!(line, 2);
    }

    #[test]
    fn an_html_comment_is_removed_across_lines_and_a_closed_one_does_not_leak() {
        let page = "before\n<!--\n[hidden](plan.md)\n-->\nafter [real](b.md)\n";
        let read = lines(page);
        assert_eq!(read.first().map(String::as_str), Some("before"));
        assert!(read.get(2).is_some_and(String::is_empty), "{read:?}");
        assert_eq!(read.last().map(String::as_str), Some("after [real](b.md)"));
        // One-line comments do not open anything.
        assert_eq!(lines("a <!-- x --> b\n"), vec![String::from("a  b")]);
    }

    #[test]
    fn an_inline_code_span_is_removed_and_an_unmatched_backtick_is_not() {
        assert_eq!(lines("say `[shown](plan.md)` here\n"), vec![String::from("say  here")]);
        // A double-backtick span may contain a single backtick.
        assert_eq!(lines("``a `b` c`` d\n"), vec![String::from(" d")]);
        // An unmatched run is literal text, so the rest of the LINE stays prose - the
        // fail-closed direction, because a real link keeps being read.
        assert_eq!(lines("a ` [real](plan.md)\n"), vec![String::from("a ` [real](plan.md)")]);
    }

    #[test]
    fn a_fence_inside_a_comment_does_not_open_a_block_and_the_reverse() {
        // Neither state may open the other, or a page explaining fence syntax inside a comment
        // would leave a block open at EOF and fail a correct tree.
        assert_eq!(lines("<!--\n```\n-->\n[real](plan.md)\n").len(), 4);
        assert_eq!(lines("```\n<!-- unclosed\n```\n[real](plan.md)\n")[3], "[real](plan.md)");
    }
}
