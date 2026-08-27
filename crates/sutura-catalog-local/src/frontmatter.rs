//! Splitting a catalog document into its YAML frontmatter and its prose.
//!
//! Its own module because it is the only text scanning in this crate, and because the failure
//! modes are the interesting part: a document whose frontmatter is silently treated as prose
//! loads as a metric with no definition, and one whose prose is silently treated as frontmatter
//! fails with a YAML error that names a line nobody wrote.

/// The line that opens and closes a frontmatter block.
const FENCE: &str = "---";

/// Why a document could not be split.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MalformedDocument {
    /// The document does not open with a fence.
    ///
    /// Required rather than inferred. A document with no frontmatter is prose, and treating prose
    /// as an empty definition would load a metric that declares nothing and refuse every question
    /// about it, which is a confusing way to report a misplaced file.
    #[error("a catalog document must begin with a `{FENCE}` frontmatter fence")]
    Unfenced,
    /// The opening fence is never closed.
    ///
    /// The failure this catches is a missing second fence swallowing the whole prose body into the
    /// YAML, which then fails to parse with a message about the prose.
    #[error("the `{FENCE}` frontmatter fence opened at line 1 is never closed")]
    Unterminated,
}

/// A document split into the part that is parsed and the part that is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Split<'a> {
    frontmatter: &'a str,
    body: &'a str,
}

impl<'a> Split<'a> {
    #[inline]
    pub const fn frontmatter(&self) -> &'a str {
        self.frontmatter
    }

    /// The prose, trimmed. It becomes a description that travels with an answer, so leading and
    /// trailing blank lines are noise rather than content.
    #[inline]
    pub const fn body(&self) -> &'a str {
        self.body
    }
}

/// Splits `text` at its frontmatter fences.
///
/// Line-oriented rather than a search for the next `---` anywhere: a `---` inside the YAML (a
/// horizontal rule in a quoted string, or a nested document marker) must not end the block, and a
/// fence is only a fence when it is the whole line.
pub fn split(text: &str) -> Result<Split<'_>, MalformedDocument> {
    // Byte-order marks arrive from editors on this platform, and a BOM before the fence makes the
    // first line not equal to `---`, which reads as a missing fence on a file that has one.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = text
        .strip_prefix(FENCE)
        .and_then(|after| after.strip_prefix('\n').or_else(|| after.strip_prefix("\r\n")))
        .ok_or(MalformedDocument::Unfenced)?;

    let mut offset = 0_usize;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']).trim_end() == FENCE {
            let (frontmatter, after_fence) = rest.split_at(offset);
            let body = after_fence.strip_prefix(line).unwrap_or_default();
            return Ok(Split {
                frontmatter,
                body: body.trim(),
            });
        }
        offset = offset.saturating_add(line.len());
    }
    Err(MalformedDocument::Unterminated)
}

#[cfg(test)]
mod tests {
    use super::{MalformedDocument, split};

    #[test]
    fn a_document_splits_at_its_fences() {
        let doc = "---\nname: revenue\n---\nNet revenue.\n";
        let parts = split(doc).expect("a fenced document splits");
        assert_eq!(parts.frontmatter(), "name: revenue\n");
        assert_eq!(parts.body(), "Net revenue.");
    }

    #[test]
    fn prose_with_no_frontmatter_is_an_error_and_not_an_empty_definition() {
        // The bug this prevents: treating a file with no frontmatter as a definition that declares
        // nothing. It then loads, and every question about it is refused for a reason that says
        // nothing about the file being in the wrong place.
        assert_eq!(split("Just some notes.\n").unwrap_err(), MalformedDocument::Unfenced);
    }

    #[test]
    fn an_unclosed_fence_is_reported_as_one() {
        // Without the check, the whole prose body is swallowed into the YAML and the parse fails
        // with a column number inside a sentence, which sends the reader to the wrong problem.
        assert_eq!(
            split("---\nname: revenue\nNet revenue.\n").unwrap_err(),
            MalformedDocument::Unterminated
        );
    }

    #[test]
    fn a_fence_is_only_a_fence_on_its_own_line() {
        // A `---` inside the frontmatter, in a quoted string or as a YAML document marker, must not
        // end the block early: that would truncate the definition and parse what remains.
        let doc = "---\nname: revenue\nnote: \"a --- b\"\n---\nBody.\n";
        let parts = split(doc).expect("an inline triple dash is not a fence");
        assert_eq!(parts.frontmatter(), "name: revenue\nnote: \"a --- b\"\n");
        assert_eq!(parts.body(), "Body.");
    }

    #[test]
    fn crlf_line_endings_split_the_same_way() {
        // This repository normalises to LF on checkin, but a document can reach the loader from a
        // working tree that has not been through git, and a fence that fails to match because of a
        // carriage return reads as a missing fence.
        let doc = "---\r\nname: revenue\r\n---\r\nBody.\r\n";
        let parts = split(doc).expect("CRLF is still a fence");
        assert_eq!(parts.frontmatter(), "name: revenue\r\n");
        assert_eq!(parts.body(), "Body.");
    }

    #[test]
    fn a_byte_order_mark_before_the_fence_is_ignored() {
        // Editors on Windows write one. Without stripping it the first line is not `---`, and a
        // document that plainly has frontmatter is reported as having none.
        let doc = "\u{feff}---\nname: revenue\n---\nBody.\n";
        let parts = split(doc).expect("a BOM is not content");
        assert_eq!(parts.frontmatter(), "name: revenue\n");
    }

    #[test]
    fn an_empty_body_is_allowed() {
        // A model document often has nothing to say in prose. It is a description that is empty,
        // not a document that is broken.
        let parts = split("---\nname: revenue\n---\n").expect("an empty body is a body");
        assert_eq!(parts.body(), "");
    }
}
