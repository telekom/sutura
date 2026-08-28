//! The authored prose a catalog carries that no identifier parser covers: a declared value and a
//! description.
//!
//! Everything else a catalog author writes into a definition is an identifier - a
//! [`ModelName`](crate::model::ModelName), a [`ColumnName`](crate::model::ColumnName), a
//! [`Grain`](crate::model::Grain) - and [`crate::model`]'s parser refuses everything that is not one
//! of a few dozen ASCII bytes. These two types are not identifiers, and for a long time they were not
//! anything: a declared dimension value and a description were `String`, straight from a YAML
//! document, and the only thing between a catalog file and an agent's context was that somebody had
//! reviewed the file.
//!
//! **Both reach an agent-facing prompt, so both are parsed here.** A declared value is interpolated
//! into the dimension list `sutura_app::prompt` renders and into the scope line of a caveat; a
//! description is quoted into the same document. Neither went through a parse, so neither was held
//! to the one rule every other piece of authored text in this crate is held to - the rule
//! [`crate::text`] owns, that the text a reviewer reads has to be the text that runs.
//!
//! **Two types, three fields**, and the count is worth stating because it was wrong here once: the
//! header of this file used to say "the two authored strings", and a
//! [`RequiredFilter`](crate::measure::RequiredFilter)'s value was a third one, on the same wired path,
//! with no parse on it. It is a [`DimensionValue`] now - the same type as a declared value, for the
//! reason that type's documentation gives about the two sides of one comparison - so the sentence and
//! the code agree again. A field counted as covered by the type it does not use is the failure mode a
//! header sentence has, and the remedy is that the types are what is enumerated here.
//!
//! # Why a separate file
//!
//! `cargo xtask max-lines` fails at a thousand lines under `crates/` and cannot be exempted, and
//! [`super`] plus these two types with their refusals is over it. The seam is a real one rather than
//! a place the file happened to be cut: [`super`] is what a catalog SAYS and where its
//! cross-references are checked, and this is the character-level parse of the two fields in it that
//! are prose rather than structure. The names stay where they were - a caller still writes
//! `sutura_domain::catalog::DimensionValue` - because the module is the unit of API and the files are
//! not. Same arrangement as [`crate::knowledge`]'s `note` and `bundle`.

use crate::text::first_invisible;

/// The longest declared dimension value, in characters.
///
/// **Measured before it was chosen.** The longest value anywhere in this repository's example catalog
/// is `fixed_internet`, at 14 characters, and every other one is a single word: `business`,
/// `wholesale`, `convergent`, `north`. So 64 is four and a half times the longest thing authored here
/// and still covers the shapes real columns hold - a UUID is 36 characters, an ISO-8601 timestamp is
/// 25, a product name like `Tariff L Business` is 17.
///
/// **The number that matters is the product of this and [`super::MAX_VALUES_PER_DIMENSION`]**, not
/// either alone, because the rendered prompt lists every declared value of a dimension on one line:
/// 64 values of 64 characters is 4 KiB, which is the same order as
/// [`crate::knowledge::MAX_NOTE_BODY_BYTES`] - one dimension's value list is bounded by about what
/// one note body is. Choosing the two together is the whole point of bounding either.
///
/// Counted in characters rather than bytes, for the reason
/// [`crate::knowledge`]'s phrase limit gives: a value may be German or Greek text, and a limit in
/// bytes would make one value legal in one language and not in another.
pub const MAX_DIMENSION_VALUE_CHARS: usize = 64;

/// One value a dimension declares, and one value a caller may filter on.
///
/// **The same type on both sides, deliberately, and that is a decision worth arguing rather than
/// assuming.** A caller sends a value and a catalog declares one, and the two are compared for
/// equality: a caller-sent value that could not have been declared can never match an allowlist, so
/// parsing it at the boundary refuses nothing a request could have got an answer for. The precedent
/// is already in this crate and predates this type - a caller's `metric` and `dimension` arrive as
/// text and are parsed by [`crate::model::MetricName`] and
/// [`crate::model::DimensionName`], the same types the catalog loader uses. A second, laxer type for
/// the request side would be a second character rule that nothing compares against the first.
///
/// What it refuses is what makes a value unusable as one:
///
/// * nothing at all - a filter on the empty string is a filter nobody wrote;
/// * a control character, a newline included - a value is one line, and the prompt renders it inline
///   inside a comma-separated list, so a newline in one writes a line of that document;
/// * an invisible or direction-changing code point, the set [`crate::text::is_invisible`] names -
///   this is the same refusal [`crate::expression::SqlFragment`] and
///   [`crate::knowledge::NoteBody`] make, at a channel that did not have it;
/// * spacing a reader cannot see: whitespace at either end, whitespace that is not a plain space,
///   and a run of two or more spaces. Two values that read as one word must not both be declarable,
///   which is [`crate::knowledge::Phrase`]'s argument - and the cost is stated rather than hidden: a
///   column whose values genuinely carry a tab, a no-break space or a double space cannot be
///   filtered on here;
/// * more than [`MAX_DIMENSION_VALUE_CHARS`] characters.
///
/// **It normalises nothing**, and that is the difference from [`crate::knowledge::Phrase`], which
/// collapses runs of whitespace and drops the invisible code points. A phrase is a key a reader
/// types; a value is compared byte for byte against what a data system holds and is bound as a
/// parameter, so a stored value that differed from the authored text would make the digest certify
/// something other than what the statement compares against. Where a phrase folds, this refuses.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
// Without this the derived `Deserialize` writes straight into the private field, and the one path
// that carries a catalog file - `values: [..]` in a metric document - bypasses every check in
// `parse`.
#[serde(try_from = "String")]
pub struct DimensionValue(String);

/// Why a value was rejected.
///
/// Every variant carries the offending text, unlike [`InvalidDescription`], and the asymmetry is the
/// one [`crate::knowledge::InvalidPhrase`] and [`crate::knowledge::InvalidNoteBody`] already make: a
/// value is at most [`MAX_DIMENSION_VALUE_CHARS`] characters, so naming it is what sends an author to
/// the line in the file, while naming four kilobytes of prose would not.
///
/// **Nothing on the request path may render one of these.** `sutura_http::wire` parses a caller's
/// filter value and reports the field and the index without the cause, for the reason
/// [`crate::query::RefusalReason`] gives: reflecting a caller's text into a message that reaches a
/// log, a UI and an agent's context is how a rejected value becomes somebody else's input. The text
/// is here for the author of a catalog, which is read by a person and loaded by an operator.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidDimensionValue {
    /// Empty. A dimension that declares the empty string permits a filter nobody wrote.
    #[error("a dimension value must not be empty")]
    Empty,
    /// Holds a control character, a newline included.
    ///
    /// Checked before the spacing rule below, even though a newline is also whitespace that is not a
    /// plain space: "this value is not one line" is the more accurate thing to tell its author than
    /// "this value has odd spacing".
    #[error("a dimension value must be one line and must not contain control characters: {value:?}")]
    ControlCharacter { value: String },
    /// One of the code points [`crate::text::is_invisible`] names.
    ///
    /// A second refusal beside the control-character one rather than a widening of it, for the reason
    /// [`crate::expression::InvalidFragment::InvisibleCharacter`] gives: `char::is_control` is false
    /// for every one of these - general category `Cf`, not `Cc` - so nothing that tests for a control
    /// character sees one. The code is reported alongside the text because an author cannot find the
    /// character by looking at the file.
    #[error("a dimension value may not contain the invisible or direction-changing character {code:#06x}: {value:?}")]
    InvisibleCharacter { value: String, code: u32 },
    /// Whitespace at either end, whitespace that is not `U+0020`, or a run of two or more spaces.
    ///
    /// One variant for the three because they are one fault: spacing a reader cannot see. `north ` and
    /// `north`, `north east` and `north\u{00A0}east`, `north east` and `north  east` are each one
    /// value to everybody who reads the rendered catalog and two entries in an allowlist.
    #[error("a dimension value must not have leading, trailing, doubled or non-plain spacing: {value:?}")]
    Spacing { value: String },
    #[error("a dimension value may be at most {limit} characters, {value:?} has {len}")]
    TooLong { value: String, len: usize, limit: usize },
}

impl DimensionValue {
    /// Parses a value, refusing anything that is not one. Normalises nothing.
    ///
    /// The order of the checks is the order the messages should arrive in, and it is deliberate
    /// rather than incidental: emptiness first because it is the most accurate thing to say about
    /// nothing, then the two character sets - control before invisible, so a newline is reported as a
    /// newline - then spacing, then the length. Reversing any pair would report a true fault under a
    /// less useful name.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidDimensionValue> {
        let raw = raw.as_ref();
        if raw.is_empty() {
            return Err(InvalidDimensionValue::Empty);
        }
        if raw.chars().any(char::is_control) {
            return Err(InvalidDimensionValue::ControlCharacter {
                value: String::from(raw),
            });
        }
        if let Some(offending) = first_invisible(raw) {
            return Err(InvalidDimensionValue::InvisibleCharacter {
                value: String::from(raw),
                code: u32::from(offending),
            });
        }
        if has_unreadable_spacing(raw) {
            return Err(InvalidDimensionValue::Spacing {
                value: String::from(raw),
            });
        }
        let len = raw.chars().count();
        if len > MAX_DIMENSION_VALUE_CHARS {
            return Err(InvalidDimensionValue::TooLong {
                value: String::from(raw),
                len,
                limit: MAX_DIMENSION_VALUE_CHARS,
            });
        }
        Ok(Self(String::from(raw)))
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Spacing a reader of the rendered catalog cannot tell from other spacing.
///
/// Three shapes, one answer, because the remedy is the same for all three: write the value the way
/// the column holds it, with single plain spaces between words and none at either end.
fn has_unreadable_spacing(raw: &str) -> bool {
    if raw.starts_with(char::is_whitespace) || raw.ends_with(char::is_whitespace) {
        return true;
    }
    // Only `U+0020` is a space a reader can account for. A no-break space, a narrow no-break space
    // and an ideographic space all draw as one and are not one.
    if raw.chars().any(|character| character.is_whitespace() && character != ' ') {
        return true;
    }
    raw.contains("  ")
}

/// The first control character a renderer would remove, if the text holds one.
///
/// Named for what it is about rather than for what it matches: the set is *the control characters
/// `sutura_app::prompt::quote` does not keep*, which is every one of them but `\n` and `\t`. Written
/// as the complement of the renderer's two exemptions rather than as its own list, so the two cannot
/// drift the way [`crate::text`]'s module documentation describes two copies of one rule drifting -
/// if the renderer ever kept a third character, this refusal would be the thing to widen, and it
/// says so in one place.
///
/// A free function here rather than a predicate in [`crate::text`]: that module owns the rule EVERY
/// authored type agrees on, and this is one type's agreement with one renderer.
fn first_altered_control(raw: &str) -> Option<char> {
    raw.chars()
        .find(|character| character.is_control() && *character != '\n' && *character != '\t')
}

/// Delegates to [`DimensionValue::parse`]: one constructor is the source of truth, and
/// `serde(try_from)` above is what makes this the deserialization path.
impl TryFrom<String> for DimensionValue {
    type Error = InvalidDimensionValue;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

impl core::fmt::Display for DimensionValue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The longest description, in bytes.
///
/// **The same number as [`crate::knowledge::MAX_NOTE_BODY_BYTES`], because it is the same
/// measurement.** The longest prose body in this repository's example catalog is 3513 bytes over 51
/// lines, and that document is a metric description - `revenue_per_churned_subscription.md` - so the
/// note-body cap was already chosen against the longest description anybody here has written. Two
/// numbers derived from one measurement would be two numbers that drift, and [`crate::text`] exists
/// because exactly that happened once already to a load-bearing refusal.
///
/// They are two constants rather than one because [`crate::knowledge`] depends on [`super`] and not
/// the other way round: a description is part of what a catalog DEFINES, and the knowledge layer is
/// checked against it. So the check that they agree is a test rather than a comment asking the next
/// author to update both - `super::tests` asserts the equality, and it fails whichever is edited
/// alone.
pub const MAX_DESCRIPTION_BYTES: usize = 4 * 1024;

/// The most lines one description may have.
///
/// Beside the byte cap rather than instead of it, for the reason
/// [`crate::knowledge::MAX_NOTE_LINES`] gives: four thousand newlines are four thousand lines of a
/// rendered prompt and well inside the byte budget. 200 is about four times the 51 lines of the
/// longest description written here.
pub const MAX_DESCRIPTION_LINES: usize = 200;

/// The prose that travels with a definition: what a model, a metric or a dimension means.
///
/// **The channel this type was added for was the last one whose rendering could differ from its
/// content.** A metric description is quoted into the agent-facing prompt by
/// `sutura_app::prompt::quote`, and the rule in this repository is *refuse at load, never alter at
/// render*, because a render that quietly removed a character would make the document differ from the
/// text the definition digest certifies, and would do so with nothing downstream able to tell. Every
/// other body reaching that renderer is a [`crate::knowledge::NoteBody`], which refuses at parse.
/// This one was a `String` built with `String::from` from a markdown document, with no character
/// check, no length check and no emptiness check anywhere on the path - so a description reading
/// `status = 'active'` in every terminal and every diff, saying something else, was reachable.
/// CVE-2021-42574 with the fragment replaced by a paragraph.
///
/// **The rule cuts both ways, which is what the first version of this type got half right.** It
/// refused the code points [`crate::text::is_invisible`] names, which the renderer keeps, and said
/// nothing about the control characters the renderer DROPS - so `refuse at load, never alter at
/// render` held in one direction here and not the other, and the alteration in the other direction
/// was reachable from a CRLF working tree. Both sets are refused now:
/// [`InvalidDescription::ControlCharacter`] names the renderer's set, minus the newline and the tab it
/// keeps, and [`InvalidDescription::InvisibleCharacter`] names [`crate::text`]'s. The renderer's own
/// filter stays where it is, because a [`crate::knowledge::NoteBody`] still reaches it and still
/// permits a control character mid-prose.
///
/// **Empty is legal, and that is the current shape rather than a concession.** A definition document
/// with no prose under its frontmatter is a definition with no description; `sutura_app::prompt`
/// renders no quoted block for one and `sutura-cli`'s `metric` command prints no paragraph. Making
/// emptiness a refusal would fail catalogs that load today, for a field whose absence is already
/// handled at every reader. That is the one place this differs from
/// [`crate::knowledge::NoteBody`], where a body is the reason a note exists at all and nothing is a
/// heading over blank space.
///
/// Multi-line, also unlike a [`crate::knowledge::Phrase`]: a description is a markdown block and its
/// paragraph breaks are the author's, so the caps are bytes and lines the way a note body's are.
/// Leading and trailing whitespace is trimmed, because a document body arrives with the newline that
/// followed its frontmatter and the one before end of file, and neither is content.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct Description(String);

/// Why a description was rejected.
///
/// **No `Empty` variant**, for the reason [`Description`] gives: a definition with no prose is a
/// definition this repository already ships and every reader already handles.
///
/// The lengths are reported without the offending text, which is [`InvalidNoteBody`]'s decision and
/// its argument applies unchanged: four kilobytes of prose named in an error message is not a
/// message. The invisible-character variant reports the code alone for the same reason - the path
/// and the code are what a `grep` needs, and `sutura_catalog_local` supplies the path.
///
/// [`InvalidNoteBody`]: crate::knowledge::InvalidNoteBody
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidDescription {
    /// A control character other than a newline or a tab, anywhere in the prose.
    ///
    /// **The half of the promise that was missing.** `sutura_app::prompt::quote` DROPS every control
    /// character except those two, so a description carrying one rendered into the agent-facing
    /// prompt as something other than the text on disk - the alteration at render this module's rule
    /// exists to forbid, at the one field that had no character check reaching that renderer. A
    /// newline and a tab are content in a markdown block and are what the renderer keeps, so they are
    /// what this permits.
    ///
    /// **A carriage return is refused with the rest, and that is the reachable case rather than the
    /// theoretical one.** `sutura_catalog_local`'s frontmatter reader strips `\r` at the fence lines
    /// alone, and `cargo xtask line-endings` sees tracked files only - so a CRLF working tree, a
    /// document pasted from a Windows editor, or a catalog directory an operator mounted from one
    /// gives every description a `\r` at the end of every line, invisible in every diff and dropped
    /// by the renderer. Refusing it names the file; normalising it would be this crate altering
    /// authored text, which is the thing the digest is taken over.
    ///
    /// Reported as a code rather than with the prose, for the reason the length variants below give,
    /// and for the reason [`InvalidDimensionValue::InvisibleCharacter`] gives: an author cannot find
    /// the character by looking at the file.
    #[error(
        "a description may not contain the control character {code:#06x}; a newline and a tab are the only ones a description may hold"
    )]
    ControlCharacter { code: u32 },
    /// One of the code points [`crate::text::is_invisible`] names, anywhere in the prose.
    ///
    /// The whole reason this type exists. It is not folded into a length or an emptiness check
    /// because it cannot be: `char::is_control` is false for every one of these code points, so a
    /// description made of ordinary sentences with one soft hyphen or one right-to-left override in
    /// the middle passes every other check there is and renders as a paragraph that reads correctly
    /// and is not what it says.
    #[error("a description may not contain the invisible or direction-changing character {code:#06x}")]
    InvisibleCharacter { code: u32 },
    /// Over [`MAX_DESCRIPTION_BYTES`]. The catalog does not load; the prose is not shortened.
    #[error("a description may be at most {limit} bytes, this one has {len}")]
    TooLong { len: usize, limit: usize },
    /// Over [`MAX_DESCRIPTION_LINES`].
    #[error("a description may be at most {limit} lines, this one has {len}")]
    TooManyLines { len: usize, limit: usize },
}

impl Description {
    /// Parses a description, refusing prose that cannot be read as what it says.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidDescription> {
        let trimmed = raw.as_ref().trim();
        // First, and before the invisible set, for the reason `DimensionValue::parse` gives: a
        // control character is the more accurate thing to name when a description holds both, and a
        // carriage return reported as an odd invisible code point would send its author looking for
        // something exotic rather than at their editor's line endings. `\n` and `\t` are exempt
        // because they are content in a markdown block and are the two the renderer keeps - the set
        // refused here is exactly the set `sutura_app::prompt::quote` would otherwise remove.
        if let Some(offending) = first_altered_control(trimmed) {
            return Err(InvalidDescription::ControlCharacter {
                code: u32::from(offending),
            });
        }
        if let Some(offending) = first_invisible(trimmed) {
            return Err(InvalidDescription::InvisibleCharacter {
                code: u32::from(offending),
            });
        }
        if trimmed.len() > MAX_DESCRIPTION_BYTES {
            return Err(InvalidDescription::TooLong {
                len: trimmed.len(),
                limit: MAX_DESCRIPTION_BYTES,
            });
        }
        let lines = trimmed.lines().count();
        if lines > MAX_DESCRIPTION_LINES {
            return Err(InvalidDescription::TooManyLines {
                len: lines,
                limit: MAX_DESCRIPTION_LINES,
            });
        }
        Ok(Self(String::from(trimmed)))
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// No description at all.
///
/// **Not a second constructor around the checks**, which is the thing a `Default` on a parsed newtype
/// usually is: the empty string is what [`Description::parse`] returns for a document with no prose
/// under its frontmatter, so this is one of `parse`'s own outputs rather than a way past it. It
/// exists because `values`-style optionality is spelled `#[serde(default)]` in the catalog format -
/// a dimension may omit `description:` - and because a test bundle that has no prose to give should
/// say so rather than parsing an empty string it wrote itself.
impl Default for Description {
    fn default() -> Self {
        Self(String::new())
    }
}

impl TryFrom<String> for Description {
    type Error = InvalidDescription;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

#[cfg(test)]
mod tests;
