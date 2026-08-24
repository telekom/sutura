//! Types and port traits. Nothing here may depend on a framework — no async runtime, no
//! web server, no query engine. `cargo xtask check-boundaries` enforces it, because the
//! rule is worth more as a check than as a sentence in a design document.

pub mod redact;

/// Content hash of a pinned definition set.
///
/// Definitions are authored upstream and arrive as an immutable, hashed snapshot; the
/// digest is what makes "the same question returns the same number" checkable rather than
/// asserted, and what stops a catalogue edit from changing what executes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DefinitionDigest(String);

impl DefinitionDigest {
    /// Rejects an empty digest so an unhashed snapshot cannot masquerade as a pinned one.
    pub fn parse(raw: impl Into<String>) -> Result<Self, InvalidDigest> {
        let raw = raw.into();
        if raw.trim().is_empty() {
            return Err(InvalidDigest::Empty);
        }
        Ok(Self(raw))
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Parse failures are values, not panics: this crate denies `unwrap`/`panic` in lints.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidDigest {
    #[error("definition digest must not be empty")]
    Empty,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_rejects_empty() {
        assert_eq!(DefinitionDigest::parse("   "), Err(InvalidDigest::Empty));
    }

    #[test]
    fn digest_roundtrips() {
        let d = DefinitionDigest::parse("abc123").unwrap();
        assert_eq!(d.as_str(), "abc123");
    }
}
