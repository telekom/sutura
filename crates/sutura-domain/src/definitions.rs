//! The identifiers of a pinned definition set, and the one operation that computes one.
//!
//! Definitions are authored upstream and arrive as an immutable, hashed snapshot. The digest
//! is what makes "the same question returns the same number" checkable rather than asserted,
//! and what stops a catalogue edit from changing what executes - so a value that is not a
//! digest must not be able to occupy the slot where one is expected.
//!
//! **The canonical form and its hash live HERE, and that is a correction rather than a
//! preference.** They used to live in a catalog adapter, and
//! [`crate::pinned::PinnedDefinitions::pin`] took the hashing function from its caller. A review
//! found what that leaves open: any safe public code could pass `|_| Ok(some_other_digest)` and pair
//! an unrelated digest with a set of definitions, so an answer could carry provenance for content
//! that did not produce it. Handing the definitions to a function is not proof that the function read
//! them. The digest has to be computed by code the domain trusts, which means code the domain holds,
//! which is what `DefinitionDigest::of` is.
//!
//! The cost is two entries on the domain's dependency allowlist - `sha2` and `serde_json`, twelve
//! crates transitively - and `xtask/src/boundaries.rs` records what was measured and why it was
//! accepted. Neither is a framework, both were already linked into the shipped binary through the
//! catalog adapter, and no lockfile entry is new.

use sha2::{Digest as _, Sha256};

use crate::catalog::Definitions;
use crate::knowledge::Knowledge;
use crate::pinned::ContributionManifest;

/// Hex characters in a digest: SHA-256, as lower-case hex.
const HEX_LEN: usize = 64;

/// Content hash of a pinned definition set.
///
/// Two ways in and they answer different questions. `DefinitionDigest::of` computes one FROM a set
/// of definitions and is what [`crate::pinned::PinnedDefinitions::pin`] uses; [`DefinitionDigest::parse`]
/// reads one that arrived as text - a provenance record, a serialized bundle - and checks its shape.
/// The field is private and `Deserialize` is routed through `parse`, so a `DefinitionDigest` that is
/// not `HEX_LEN` hex characters does not exist to be passed anywhere.
///
/// **A parsed digest is not a forgery route, and the difference is worth being precise about.**
/// Anybody may parse any hex string into one of these; what nothing can do is get it into a
/// [`crate::pinned::PinnedDefinitions`], because that type's constructor takes no digest and computes
/// its own. So this type says "this is digest-shaped" and the bundle says "this digest is of that
/// content", and only the second claim is one an answer rests on.
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

    /// The digest of everything a bundle carries: the definitions, the knowledge about them, and the
    /// composition that produced both.
    ///
    /// Three arguments, not two, because all three are content somebody composed and all three change
    /// what an answer means - see [`canonical_form`] for why a glossary belongs under the same hash as
    /// a measure, and `docs/adr/0011` for why the contribution manifest belongs beside both: a bundle
    /// that assembled identically from a different set of sources is still a different bundle, and a
    /// digest that could not tell the two apart would report a re-composition as the same snapshot.
    ///
    /// **The trusted operation, and the only way content becomes a digest.** It is `pub(crate)`
    /// rather than `pub` on purpose: the public surface for "hash these definitions" is
    /// [`crate::pinned::PinnedDefinitions::pin`], which stores the definitions it hashed in the same
    /// step. Exposing this on its own would put a digest-of-content value in a caller's hands with
    /// nothing holding it to the content, which is one refactor away from the hole this replaced.
    pub(crate) fn of(
        definitions: &Definitions,
        knowledge: &Knowledge,
        manifest: &ContributionManifest,
    ) -> Result<Self, NotDigestible> {
        let canonical =
            canonical_form(definitions, knowledge, manifest).map_err(|cause| NotDigestible::Canonicalize { cause })?;
        let hash = Sha256::digest(&canonical);
        let hex: String = hash
            .iter()
            .flat_map(|byte| [nibble(byte >> 4_u8), nibble(byte & 0x0f_u8)])
            .collect();
        // Lower-case hex of 32 bytes is what `parse` accepts, so this cannot fail today. Routed
        // through `parse` anyway rather than writing the private field directly, because "the
        // constructor is the only way in" is the rule this type is here to demonstrate - and if the
        // hashing ever changes shape the error says so rather than blaming the catalog.
        Self::parse(hex).map_err(|cause| NotDigestible::NotADigest { cause })
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The canonical byte form of a bundle's content: what the digest is taken over.
///
/// JSON rather than the YAML or the wire format a definition was read from, and that is the whole
/// point. Reformatting a document, reordering two files or rewording a comment must not move the
/// digest; changing what a metric means must. Serializing the *parsed* content gives exactly that,
/// because everything that survives parsing is meaning and everything that does not is layout.
///
/// **All three halves, and the knowledge and the composition are not decoration in here.**
/// A glossary entry decides which metric an agent asks about, so a bundle whose glossary changed is a
/// bundle that answers different questions from the same words - and a digest that did not move would
/// say the two were the same snapshot. The contribution manifest is the same argument one level out: a
/// subscription that swapped which metadata sources composed it, or lost one that a previous run
/// included, is not the bundle it used to be, and the digest is what makes that visible. It also makes
/// the capability distinction certifiable - a provider that declares an empty absence list and one that
/// has no such concept serialize differently, and the prompt is allowed to say different things about
/// them, so the digest has to be able to tell them apart.
///
/// Deterministic for three reasons that all have to hold: [`Definitions`], [`Knowledge`] and
/// [`ContributionManifest`] use `BTreeMap` throughout, so collection order is content order rather
/// than hash order, and `serde_json` writes struct fields in declaration order. A three-element
/// sequence rather than a struct with three keys because the shape only has to be stable, not
/// readable - nothing ever parses these bytes back.
///
/// Private, and not merely unexported: the bytes are an implementation detail of the digest, and a
/// second caller of this function would be a second place with an opinion about what canonical means.
fn canonical_form(
    definitions: &Definitions,
    knowledge: &Knowledge,
    manifest: &ContributionManifest,
) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&(definitions, knowledge, manifest))
}

/// One hex digit.
///
/// Written out rather than reached through `format!`, because the formatting machinery returns a
/// `Result` that cannot fail here and the ways of discarding it all trip a lint: the alternatives are
/// an `expect` on a path a catalog file can reach, or a `let _` on a `#[must_use]` value.
fn nibble(value: u8) -> char {
    if value < 10 {
        char::from(b'0'.saturating_add(value))
    } else {
        char::from(b'a'.saturating_add(value.saturating_sub(10)))
    }
}

/// Why a set of definitions could not be reduced to a digest.
///
/// Two variants, and neither is reachable from any catalog this repository can load - which is why
/// they are variants rather than a panic. Neither `Definitions` nor `Knowledge` holds a float and
/// every map key in both is a newtype over a string, so the serializer has nothing to refuse; and
/// lower-case hex of 32 bytes is
/// what a digest is. Each variant names which half changed, so a future field of a type that does not
/// serialize says so instead of surfacing as "the catalog is broken".
#[derive(Debug, thiserror::Error)]
pub enum NotDigestible {
    /// The definitions could not be written into their canonical form.
    #[error("the definitions could not be put into canonical form to be hashed")]
    Canonicalize {
        #[source]
        cause: serde_json::Error,
    },
    /// The computed hash is not digest-shaped, which means the hashing changed and not the catalog.
    #[error("the computed digest is not a digest, which means the hashing changed shape")]
    NotADigest {
        #[source]
        cause: InvalidDigest,
    },
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

    /// Deserialize through the domain-side shape rather than through a JSON parser.
    ///
    /// NOT a boundary-gate constraint: `serde_json` is a dependency of this crate and IS
    /// allowlisted (`xtask/src/boundaries.rs`), which is what makes the digest's canonical form
    /// possible at all. The reason is scope - this asserts the serde WIRING, which attribute
    /// routes which direction, and a format parser would add a second thing that could fail.
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
