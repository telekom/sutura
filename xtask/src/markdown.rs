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
                    "line {line} opens a `{run}` fence that no later line closes - every line below it is either code or prose and this cannot say which, so the link scan refuses rather than guessing. Close the fence, or lengthen the outer one if a fence is nested"
                )
            }
            Self::Comment { line } => write!(
                f,
                "line {line} opens an HTML comment that no later line closes - everything below it would be swallowed, so the link scan refuses rather than reading none of it"
            ),
        }
    }
}

/// The fence a line opens, if it opens one.
///
/// **Indentation is deliberately not restricted**, where `CommonMark` allows an opening fence at
/// most three columns in. Material's admonitions carry fenced blocks four columns in and mkdocs
/// renders them, so the strict rule would read a documented command as prose. The direction is
/// the safe one: treating more of the page as code cannot invent a link, and the unclosed check
/// makes a fence opened by accident loud.
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

/// The prose of `text`: one entry per source line, with every fenced code block, HTML comment and
/// inline code span blanked.
///
/// Blanked rather than dropped so a line number is still a line number, and owned rather than
/// borrowed because blanking a span rewrites the line - the alternative is a second pass over
/// the same text by every caller.
pub(crate) fn prose(text: &str) -> Result<Vec<String>, Unlexable> {
    let mut out: Vec<String> = Vec::new();
    let mut fence: Option<Fence> = None;
    let mut comment: Option<usize> = None;
    for (index, line) in text.lines().enumerate() {
        let number = index.saturating_add(1);
        if let Some(open) = fence.as_ref() {
            if closes(line, open) {
                fence = None;
            }
            out.push(String::new());
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
        out.push(visible);
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
    use super::{Unlexable, prose};

    fn lines(text: &str) -> Vec<String> {
        prose(text).unwrap_or_else(|e| panic!("{e}"))
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
        assert!(lines("~~~\n```\n~~~\n[real](plan.md)\n")[1].is_empty());
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
