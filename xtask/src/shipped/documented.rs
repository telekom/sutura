//! What a published page tells a reader to BUILD, read out of the page's code blocks.
//!
//! **Its own file for `refusal.rs`'s reason: the parent is against the unexemptable 1000-line
//! cap.** The seam is the one the causality gate forces - the mechanism moves, the parent's
//! assertions stay where they are, and the tests added here are NEW ones, so nothing can be
//! orphaned by reverting either file.
//!
//! Why the fence boundary exists at all is on [`builds`]. Why it is a LEXER and not a parity
//! toggle is `github.com/telekom/sutura#301`, and it is the third instance of one shape in this
//! repository: `xtask/src/docs/links.rs` and `crates/sutura-cli/tests/documented.rs` each counted
//! three-backtick lines with a boolean. The reader is [`crate::markdown`], shared rather than
//! copied, because a fourth implementation would be a fourth thing to keep in step.

use crate::markdown;

/// A `cargo build` line in published prose that names a package AND a feature list.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct DocumentedBuild {
    /// Repo-relative page, for the message.
    pub(super) page: String,
    /// One-based line, so a failure names something a reader can open.
    pub(super) line: usize,
    /// The `-p` / `--package` value.
    pub(super) package: String,
    /// The `--features` value, split on commas.
    pub(super) features: Vec<String>,
}

/// Every documented feature build in one page.
///
/// **`cargo build` and not `cargo run`**, for the reason the module header gives. Both the spaced
/// and the `=` form of each flag are read, because a page may legitimately write either and a gate
/// that sees one shape silently passes the other.
///
/// **INSIDE A FENCED BLOCK ONLY, and that boundary was forced by running the rule.** The first
/// version read any line, and the gate's own report went from one reconciled build to two the
/// moment `docs/adr/0017` gained a sentence *quoting* the command this rule reconciles. The two
/// are different things: a fenced block is an instruction a reader follows, an inline citation is
/// a MENTION - and a decision record legitimately quotes a command that is no longer current,
/// which would then fail a correct tree. A gate that does that gets disabled, so the fence is the
/// boundary. **It was only visible because the report NAMES the rows** - a count would have read
/// `2` and looked like more coverage.
///
/// What it gives up: a page instructing inline rather than in a block is not read. The fail-closed
/// check in the parent's `run` is what stops that being silent - with no such block anywhere, the
/// gate refuses rather than reconciling nothing.
///
/// **LEXED, not toggled**, and the `Err` is the half a parity boolean could not have. A page whose
/// block is still open at the end makes every line below it either an instruction or a mention with
/// nothing to say which, so this refuses over it rather than answering - the rule
/// [`markdown::Unlexable`] states, reached from the code side.
pub(super) fn builds(page: &str, text: &str) -> Result<Vec<DocumentedBuild>, markdown::Unlexable> {
    let mut out = Vec::new();
    for (index, line) in markdown::code(text)?.iter().enumerate() {
        if !line.contains("cargo build") {
            continue;
        }
        let (Some(package), Some(features)) = (flag_value(line, "-p", "--package"), flag_value(line, "", "--features")) else {
            continue;
        };
        out.push(DocumentedBuild {
            page: String::from(page),
            line: index.saturating_add(1),
            package,
            features: features
                .split(',')
                .map(str::trim)
                .filter(|f| !f.is_empty())
                .map(String::from)
                .collect(),
        });
    }
    Ok(out)
}

/// The value of `short` or `long` on this line, in either the spaced or the `=` form.
///
/// An empty `short` means the flag has no short form. The value stops at whitespace and at the
/// punctuation prose wraps a command in - a backtick, a quote, a line-continuation backslash - so
/// an inline citation yields the same value a fenced block does.
fn flag_value(line: &str, short: &str, long: &str) -> Option<String> {
    for flag in [long, short] {
        if flag.is_empty() {
            continue;
        }
        for sep in [' ', '='] {
            let needle = format!("{flag}{sep}");
            let Some(at) = line.find(&needle) else {
                continue;
            };
            // A boundary before the flag, so `--no-features` is not read as `--features`.
            let boundary = line
                .get(..at)
                .and_then(|s| s.chars().next_back())
                .is_none_or(char::is_whitespace);
            if !boundary {
                continue;
            }
            let tail = line.get(at.saturating_add(needle.len())..)?.trim_start();
            let value: String = tail
                .chars()
                .take_while(|c| !c.is_whitespace() && !matches!(c, '\\' | '`' | '"' | '\''))
                .collect();
            if !value.is_empty() && !value.starts_with('-') {
                return Some(value);
            }
        }
    }
    None
}

/// Every documented feature build in `docs/**`, page by page.
///
/// **A page this cannot lex is an `Err` and not a shorter list.** The alternative - skip that page
/// and reconcile the rest - is the shape `.agents/skills/sutura/gates/SKILL.md` records twice: the
/// other pages supply the count, the fail-closed check in the parent's `run` stays satisfied, and
/// the page whose instructions went unread is the one nobody hears about.
pub(super) fn pages(root: &std::path::Path) -> Result<Vec<DocumentedBuild>, String> {
    let mut pages = Vec::new();
    crate::repo::collect_files(root, &root.join("docs"), &["md"], &mut pages);
    pages.sort();
    let mut found = Vec::new();
    for page in pages {
        let Ok(text) = std::fs::read_to_string(root.join(&page)) else {
            continue;
        };
        match builds(&page, &text) {
            Ok(here) => found.extend(here),
            Err(why) => return Err(format!("{page}: {why}")),
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::DocumentedBuild;

    fn read(page: &str, text: &str) -> Vec<DocumentedBuild> {
        super::builds(page, text).unwrap_or_else(|e| panic!("{e}"))
    }

    /// THE REPRODUCTION for `github.com/telekom/sutura#301`, and it asserts BOTH directions of the
    /// same inversion in one fixture.
    ///
    /// A four-backtick block whose content is a three-backtick one is how a page documents fence
    /// syntax, and `docs/publishing.md` already carries that shape. A parity toggle reads the
    /// inner opener as a CLOSE, so with three delimiter lines in the page:
    ///
    /// * the `cargo build` line inside the outer block is outside every block, and the
    ///   declaration is LOST - `probeFeatures` stops being reconciled against it;
    /// * the outer close inverts the flag a third time, so the MENTION below the block - the shape
    ///   [`super::builds`]'s own header says must not be read, because a record quoting a command
    ///   is not a page instructing a reader to run it - is read as a declaration.
    ///
    /// Neither is visible to the fail-closed check in the parent's `run`: other pages supply the
    /// count, which is the per-tree quantifier `.agents/skills/sutura/gates/SKILL.md` records.
    ///
    /// **Measured against the parity toggle over this exact fixture**, and the answer is worse than
    /// a shorter list: it returned ONE row, `sutura-serve` at line 5 - the mention - and not the
    /// declaration at line 3. So the count a reader would sanity-check was right and the row was
    /// fabricated, which is why this asserts the row's PACKAGE and not `found.len()`.
    #[test]
    fn a_nested_fence_neither_loses_a_declaration_nor_invents_one() {
        let page = concat!(
            "````text\n",
            "```bash\n",
            "cargo build --release -p sutura-cli --features bigquery\n",
            "````\n",
            "a MENTION: `cargo build -p sutura-serve --features invented`\n",
        );
        let found = read("docs/p.md", page);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].line, 3, "the declaration inside the outer block is at line 3");
        assert_eq!(found[0].package, "sutura-cli");
        assert_eq!(found[0].features, vec![String::from("bigquery")]);
        assert!(
            found.iter().all(|b| b.package != "sutura-serve"),
            "the mention below the block was read as a declaration: {found:?}"
        );
    }

    /// A tilde fence is a fence, and it does not close a backtick block.
    ///
    /// Two separate consequences of recording the delimiter CHARACTER: a tilde block's contents
    /// are read, and a tilde line inside a backtick block is content rather than a close.
    #[test]
    fn a_tilde_fence_is_a_fence_and_does_not_close_a_backtick_block() {
        let tilde = "~~~bash\ncargo build -p sutura-cli --features bigquery\n~~~\n";
        assert_eq!(read("docs/p.md", tilde).len(), 1);
        let mixed = concat!(
            "```bash\n",
            "~~~\n",
            "cargo build -p sutura-cli --features bigquery\n",
            "```\n",
            "a MENTION: `cargo build -p sutura-serve --features invented`\n",
        );
        let found = read("docs/p.md", mixed);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].line, 3);
    }

    /// A page whose block never closes is a refusal, not a shorter answer.
    ///
    /// The direction a parity toggle cannot express: with the flag left on, every remaining line
    /// is read as an instruction, and with it left off none of them is. Both are answers to a
    /// question this cannot answer.
    #[test]
    fn a_page_left_open_at_the_end_is_refused_rather_than_read() {
        let page = "```bash\ncargo build -p sutura-cli --features bigquery\n";
        let Err(why) = super::builds("docs/p.md", page) else {
            panic!("an unclosed fence must not produce a build list");
        };
        assert!(format!("{why}").contains("line 1"), "{why}");
    }
}
