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
/// **What it gives up, corrected - the boundary is FENCED, not *not inline*.** Review of
/// `github.com/telekom/sutura#301` measured that an instruction written as a four-column INDENTED
/// code block is neither fenced nor inline and was silently unread, so the earlier wording here
/// understated the loss. That half is closed by [`indented_instructions`], which refuses rather
/// than reads. What is still given up is an instruction genuinely written inline, in backticks, in
/// a sentence - excluded on purpose, per the paragraph above. The fail-closed check in the parent's
/// `run` is what stops the whole rule being silent: with no fenced build anywhere, the gate refuses
/// rather than reconciling nothing.
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

/// A build instruction written as an INDENTED code block, which the fenced reader cannot see.
///
/// **The limit review of `github.com/telekom/sutura#301` corrected, closed rather than declared.**
/// `CommonMark` reads a line four columns in as a code block; this lexer reads it as prose,
/// because mkdocs-material takes an admonition body four columns in and treating that as code
/// would blank every admonition on the site - measured against `check-docs`, whose per-page link
/// floor reads those bodies. So the LEXER cannot change, and the loss is real: an instruction in an
/// indented block is neither fenced nor inline, and the old limit list called the boundary
/// *inline*, which was wrong.
///
/// What closes it is a refusal rather than a reading: an indented line whose visible prose is a
/// `cargo build` carrying both a package and a feature list is an instruction this reader would
/// drop, so the gate says so and asks for a fence. **Read from [`markdown::prose`], and that is
/// what makes it precise** - an inline MENTION lives inside backticks and prose blanks a code
/// span, so the shape the fence boundary deliberately excludes cannot reach here. Four columns is
/// `CommonMark`'s own threshold.
///
/// **Measured before it was added:** `grep -rnE '^( {4,}|\t)cargo ' docs` matches one line on
/// 2026-09-05, in `docs/.tools/rustdoc_to_markdown.py` - not a page, and `cargo rustdoc`. So this
/// fires on nothing in the tree today, which is what a refusal over a shape nobody writes should
/// do.
fn indented_instructions(text: &str) -> Result<Vec<usize>, markdown::Unlexable> {
    let mut found = Vec::new();
    for (index, line) in markdown::prose(text)?.iter().enumerate() {
        let indent = line.chars().take_while(|c| *c == ' ' || *c == '\t').count();
        if indent < 4 {
            continue;
        }
        let visible = line.trim_start();
        if !visible.starts_with("cargo build") {
            continue;
        }
        if flag_value(visible, "-p", "--package").is_some() && flag_value(visible, "", "--features").is_some() {
            found.push(index.saturating_add(1));
        }
    }
    Ok(found)
}

/// Every documented feature build in `docs/**`, page by page.
///
/// **A page this cannot lex is an `Err` and not a shorter list.** The alternative - skip that page
/// and reconcile the rest - is the shape `.agents/skills/sutura/gates/SKILL.md` records twice: the
/// other pages supply the count, the fail-closed check in the parent's `run` stays satisfied, and
/// the page whose instructions went unread is the one nobody hears about.
///
/// **And a page this cannot READ is the same `Err`, which it was not when this shipped.** Review
/// of `github.com/telekom/sutura#301` measured it: a non-UTF-8 `docs/probe-ghost.md` documenting a
/// feature `nix/shipped.nix` does not probe left the gate at `ok` and `EXIT=0`, because
/// `read_to_string` failed and the loop moved on. The unlexable arm one line below was already
/// closed, so the guard covered *cannot say which* and not *cannot look at all* - the same
/// asymmetry `check-docs` had, where an unreadable page was dropped in silence eight lines above a
/// `FAIL CLOSED` comment. **The two are one question: is this gate's answer over the tree it
/// names?**
///
/// **It walks every page under `docs/`, INCLUDING the ones the site does not publish.** #292's
/// `exclude_docs` keeps three implementation-plan pages off the site and this reader does not
/// consult it - reported in review of #301 as latent, and it is: none of those pages carries a
/// fenced `cargo build --features` today. Left as is deliberately, and the direction is why - a
/// build instruction in the repository is one somebody follows whether mkdocs renders the page or
/// not, so reconciling an unpublished page is the conservative error. The one thing to know is that
/// a refusal from here can name a page a reader will not find on the site.
pub(super) fn pages(root: &std::path::Path) -> Result<Vec<DocumentedBuild>, String> {
    let mut pages = Vec::new();
    crate::repo::collect_files(root, &root.join("docs"), &["md"], &mut pages);
    pages.sort();
    let mut found = Vec::new();
    for page in pages {
        let text =
            std::fs::read_to_string(root.join(&page)).map_err(|why| format!("{page}: cannot be read as UTF-8 text - {why}"))?;
        match builds(&page, &text) {
            Ok(here) => found.extend(here),
            Err(why) => return Err(format!("{page}: {why}")),
        }
        match indented_instructions(&text) {
            Ok(lines) if lines.is_empty() => {}
            Ok(lines) => {
                return Err(format!(
                    "{page}:{lines:?} document a `cargo build --features` as an INDENTED code block. \
                     This reads fenced blocks only, so the declaration would go unreconciled while \
                     the other pages keep the count non-empty - fence it instead"
                ));
            }
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

    /// AN INDENTED build instruction is refused rather than dropped.
    ///
    /// The limit review of `github.com/telekom/sutura#301` corrected. `CommonMark` calls this a code
    /// block, this lexer calls it prose, and [`super::builds`] reads only the fenced half - so
    /// without this arm the declaration vanishes and, with any other page carrying a fenced build,
    /// nothing fails. Both directions in one fixture: the indented line is refused, and the inline
    /// MENTION on the line below it - the shape the fence boundary deliberately excludes - is not.
    #[test]
    fn an_indented_build_instruction_is_refused_and_an_inline_mention_is_not() {
        let indented = "text\n\n    cargo build -p sutura-cli --features bigquery\n\nafter\n";
        assert_eq!(
            super::indented_instructions(indented).unwrap_or_else(|e| panic!("{e}")),
            vec![3]
        );
        // Inline, in backticks, inside a sentence: prose blanks a code span, so it cannot reach
        // this arm however deeply the paragraph is indented.
        let mention = "text\n\n    a record quoting `cargo build -p sutura-cli --features bigquery` here\n";
        assert!(
            super::indented_instructions(mention)
                .unwrap_or_else(|e| panic!("{e}"))
                .is_empty()
        );
        // And a FENCED block four columns in is code, not an indented instruction - the
        // admonition shape `markdown::opens` diverges from `CommonMark` for.
        let fenced = "text\n\n    ```bash\n    cargo build -p sutura-cli --features bigquery\n    ```\n";
        assert!(
            super::indented_instructions(fenced)
                .unwrap_or_else(|e| panic!("{e}"))
                .is_empty()
        );
        assert_eq!(read("docs/p.md", fenced).len(), 1);
    }

    /// A page this cannot READ is the same refusal as a page it cannot lex.
    ///
    /// **The blocking finding from review of `github.com/telekom/sutura#301`, held by a test rather
    /// than by the sentence that got it wrong.** Measured on the real tree before the fix: a
    /// non-UTF-8 `docs/probe-ghost.md` documenting `--features ghostfeature`, which
    /// `nix/shipped.nix` does not probe, left the gate at `ok` and exit 0.
    #[test]
    fn a_page_that_cannot_be_read_is_refused_rather_than_skipped() {
        let dir = std::env::temp_dir().join(format!("sutura-documented-{}", std::process::id()));
        let docs = dir.join("docs");
        std::fs::create_dir_all(&docs).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(
            docs.join("good.md"),
            "```bash\ncargo build -p sutura-cli --features bigquery\n```\n",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let found = super::pages(&dir).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(found.len(), 1, "{found:?}");

        // A second page that is not UTF-8. The good page keeps the reconciled set non-empty, which
        // is exactly why the old `continue` was invisible.
        std::fs::write(docs.join("ghost.md"), [0xff_u8, 0xfe, 0x00]).unwrap_or_else(|e| panic!("{e}"));
        let Err(why) = super::pages(&dir) else {
            panic!("an unreadable page must not produce a shorter list");
        };
        assert!(why.contains("docs/ghost.md"), "{why}");
        assert!(why.contains("UTF-8"), "{why}");
        std::fs::remove_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
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
