//! The identifiers of a pinned definition set.
//!
//! Definitions are authored upstream and arrive as an immutable, hashed snapshot. The digest
//! is what makes "the same question returns the same number" checkable rather than asserted,
//! and what stops a catalogue edit from changing what executes - so a value that is not a
//! digest must not be able to occupy the slot where one is expected.

/// Hex characters in a digest. Upstream hashes a definition set with SHA-256.
///
/// This crate cannot recompute the hash - `sha2` is not in its allowlisted dependency tree,
/// and a domain that could hash would be a domain that could re-derive what it is supposed
/// to accept as given. So the shape is what it checks, and the shape is exact.
const HEX_LEN: usize = 64;

/// Content hash of a pinned definition set.
///
/// Construct it with [`DefinitionDigest::parse`]. There is no other way in: the field is
/// private and `Deserialize` is routed through the same constructor, so a `DefinitionDigest`
/// that is not `HEX_LEN` hex characters does not exist to be passed anywhere.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
// Without this, the derived `Deserialize` writes straight into the private field and every
// validation below is bypassed by the one path that actually carries untrusted input: a
// pinned snapshot arriving as JSON. Parsing has to happen at the boundary or it is theatre.
#[serde(try_from = "String")]
pub struct DefinitionDigest(String);

impl DefinitionDigest {
    /// Parses a digest, rejecting anything that is not one.
    ///
    /// Parse, not validate: once this returns `Ok`, nothing downstream re-checks the shape,
    /// because an ill-formed digest is unrepresentable. Case is normalised here rather than
    /// at comparison sites, so one digest has one spelling and the derived `PartialEq`,
    /// `Hash` and `Serialize` all agree about which digest this is.
    pub fn parse(raw: impl Into<String>) -> Result<Self, InvalidDigest> {
        let raw = raw.into();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(InvalidDigest::Empty);
        }
        if let Some(offending) = trimmed.chars().find(|c| !c.is_ascii_hexdigit()) {
            return Err(InvalidDigest::NotHex {
                value: String::from(trimmed),
                offending,
            });
        }
        // Every byte is an ASCII hex digit by now, so byte length is character length.
        if trimmed.len() != HEX_LEN {
            return Err(InvalidDigest::WrongLength {
                value: String::from(trimmed),
                len: trimmed.len(),
                expected: HEX_LEN,
            });
        }
        Ok(Self(trimmed.to_ascii_lowercase()))
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Delegates to [`DefinitionDigest::parse`] rather than repeating it: one constructor is the
/// source of truth, and `serde(try_from)` above is what makes this the deserialization path.
impl TryFrom<String> for DefinitionDigest {
    type Error = InvalidDigest;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

/// Why a digest was rejected. Parse failures are values, not panics: this crate denies
/// `unwrap`/`panic` in lints.
///
/// Each variant carries the offending input as a typed field, not a pre-formatted sentence.
/// The variants and their fields are the contract; the `#[error]` text is a convenience for
/// a human and may be reworded without breaking a caller that matched on `WrongLength`.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidDigest {
    /// Empty or whitespace-only, so an unhashed snapshot cannot masquerade as a pinned one.
    #[error("definition digest must not be empty")]
    Empty,
    /// Not hexadecimal. `offending` is the first character that is not, which is the one
    /// worth reporting - a message naming all of them tells the reader less.
    #[error("definition digest must be hexadecimal: {value:?} contains {offending:?}")]
    NotHex { value: String, offending: char },
    /// Hexadecimal, but not a SHA-256 digest. `expected` rides along so a caller can render
    /// its own message from fields, and so this text and `HEX_LEN` cannot drift apart.
    #[error("definition digest must be {expected} hex characters, {value:?} has {len}")]
    WrongLength { value: String, len: usize, expected: usize },
}

#[cfg(test)]
mod tests {
    use super::{DefinitionDigest, HEX_LEN, InvalidDigest};

    /// A real SHA-256 digest, as a definition set's hash arrives.
    const VALID: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn a_sha256_digest_parses_and_round_trips() {
        let digest = DefinitionDigest::parse(VALID).expect("a real digest is a digest");
        assert_eq!(digest.as_str(), VALID);
    }

    #[test]
    fn empty_is_rejected() {
        assert_eq!(DefinitionDigest::parse("").unwrap_err(), InvalidDigest::Empty);
        assert_eq!(DefinitionDigest::parse("   ").unwrap_err(), InvalidDigest::Empty);
    }

    #[test]
    fn prose_is_not_a_digest() {
        // The bug this type existed to prevent and did not: `parse` rejected only whitespace,
        // so arbitrary text became a "content hash" and every downstream claim of
        // determinism rested on it.
        assert_eq!(
            DefinitionDigest::parse("not a hash").unwrap_err(),
            InvalidDigest::NotHex {
                value: String::from("not a hash"),
                offending: 'n',
            }
        );
    }

    #[test]
    fn a_non_hex_character_is_reported_with_the_character() {
        // `g` is the classic paste error: base32, or a truncated identifier, not a hash.
        let raw = "g3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(
            DefinitionDigest::parse(raw).unwrap_err(),
            InvalidDigest::NotHex {
                value: String::from(raw),
                offending: 'g',
            }
        );
    }

    #[test]
    fn hex_of_the_wrong_length_is_rejected() {
        // Hexadecimal and plausible-looking: a truncated digest, or a git short SHA in the
        // slot where a definition hash belongs.
        assert_eq!(
            DefinitionDigest::parse("abc123").unwrap_err(),
            InvalidDigest::WrongLength {
                value: String::from("abc123"),
                len: 6,
                expected: HEX_LEN,
            }
        );
    }

    #[test]
    fn case_is_normalised_so_one_digest_has_one_spelling() {
        let upper = DefinitionDigest::parse(VALID.to_ascii_uppercase()).expect("hex in either case is hex");
        let lower = DefinitionDigest::parse(VALID).expect("hex in either case is hex");
        // Equality and hashing are derived, so normalising in the constructor is the only
        // place that makes them agree with upstream about which digest this is.
        assert_eq!(upper, lower);
        assert_eq!(upper.as_str(), lower.as_str());
    }

    #[test]
    fn surrounding_whitespace_is_not_part_of_the_digest() {
        let digest = DefinitionDigest::parse(format!("  {VALID}\n")).expect("a trimmed digest is a digest");
        assert_eq!(digest.as_str(), VALID);
    }

    /// Deserialize without pulling a format crate into the domain's dependency tree - the
    /// boundary gate allowlists neither, and this tests the wiring, not the JSON parser.
    fn deserialize(raw: &str) -> Result<DefinitionDigest, serde::de::value::Error> {
        use serde::Deserialize as _;
        use serde::de::IntoDeserializer as _;

        DefinitionDigest::deserialize(String::from(raw).into_deserializer())
    }

    #[test]
    fn deserialization_goes_through_the_constructor() {
        // The hole a derived `Deserialize` leaves open: the field is private, so the only
        // way to forge a digest was the one path that carries untrusted input.
        drop(deserialize("not a hash").expect_err("prose must not deserialize into a digest"));
        drop(deserialize("").expect_err("an empty string must not deserialize into a digest"));
        let digest = deserialize(VALID).expect("a real digest still deserializes");
        assert_eq!(digest.as_str(), VALID);
    }

    #[test]
    fn the_try_from_conversion_is_the_same_check() {
        // Not a second implementation: `TryFrom` delegates, so there is one place where the
        // rules live and one place a future rule has to be added.
        assert_eq!(
            DefinitionDigest::try_from(String::from(VALID)),
            DefinitionDigest::parse(VALID)
        );
        assert_eq!(
            DefinitionDigest::try_from(String::from("nope")),
            DefinitionDigest::parse("nope")
        );
    }
}
