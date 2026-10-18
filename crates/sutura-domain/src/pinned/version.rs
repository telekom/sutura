//! The label of a pinned snapshot, parsed: what an audit line may carry of it.

use crate::text::first_invisible;

/// The longest version label we accept. Long enough for a commit id plus a tag, short enough that
/// it cannot be used to smuggle a paragraph into an audit record.
pub(super) const MAX_VERSION_LEN: usize = 128;

/// Which snapshot of the definitions this is.
///
/// Free-form on purpose, because what identifies a snapshot differs per catalog: a commit id, a
/// build number, an export timestamp. What is *not* free-form is its shape, because it is echoed
/// into provenance and into an audit record, and a value with a newline in it can forge a second
/// record.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct DefinitionVersion(String);

/// Why a version label was rejected.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidVersion {
    /// Empty or whitespace-only. An unversioned snapshot must not be able to claim it is one.
    #[error("a definition version must not be empty")]
    Empty,
    /// Holds a control character. This is the one that matters: provenance is written to an audit
    /// sink line by line, so a newline here appends a record nobody wrote.
    #[error("a definition version must not contain control characters: {value:?}")]
    ControlCharacter { value: String },
    /// Holds an invisible or direction-changing code point. **The second half of the reason the
    /// variant above exists**, and it was missing: `char::is_control` is false for every one of
    /// these (general category `Cf`, not `Cc`), so the check that refuses a newline could not see a
    /// right-to-left override, and a version label carrying one passed.
    ///
    /// That matters here for exactly the reason the control-character refusal states. A version
    /// travels in [`Provenance`](crate::pinned::Provenance), and provenance is written to an audit sink line by line, echoed
    /// into the body of an answer, and printed by the compile command - so a label that renders as
    /// `v2.1` in every one of those places and is a different string is a provenance record that
    /// cannot be matched against the bundle it names. The digest is what makes an answer checkable;
    /// two labels a reader cannot tell apart is the one way to make the version half of it useless.
    ///
    /// The code is reported rather than the value, which is the opposite of the variant above: a
    /// label whose only defect is a character that draws nothing would print as though it were
    /// correct, so the message names the character instead.
    #[error("a definition version must not contain the invisible or direction-changing character {code:#06x}")]
    InvisibleCharacter { code: u32 },
    #[error("a definition version may be at most {limit} characters, {value:?} has {len}")]
    TooLong { value: String, len: usize, limit: usize },
}

impl DefinitionVersion {
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidVersion> {
        let trimmed = raw.as_ref().trim();
        if trimmed.is_empty() {
            return Err(InvalidVersion::Empty);
        }
        if trimmed.chars().any(char::is_control) {
            return Err(InvalidVersion::ControlCharacter {
                value: String::from(trimmed),
            });
        }
        // Beside the control-character check rather than folded into it, because it is a second
        // character class the first one provably cannot see. `crate::text` owns the set, so this
        // refusal and the one an authored SQL fragment gets are the same refusal.
        if let Some(offending) = first_invisible(trimmed) {
            return Err(InvalidVersion::InvisibleCharacter {
                code: u32::from(offending),
            });
        }
        if trimmed.chars().count() > MAX_VERSION_LEN {
            return Err(InvalidVersion::TooLong {
                value: String::from(trimmed),
                len: trimmed.chars().count(),
                limit: MAX_VERSION_LEN,
            });
        }
        Ok(Self(String::from(trimmed)))
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for DefinitionVersion {
    type Error = InvalidVersion;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

impl core::fmt::Display for DefinitionVersion {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}
