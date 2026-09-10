//! A stable, non-reversible fingerprint of a caller's assertion, for correlation.
//!
//! **Its own file, and the reason is the causality gate rather than the line cap.** A newtype and
//! the mutation tests that prove it can be red against base only when definition and tests share a
//! file - reverting the definition without the tests leaves a tree that does not compile, which is
//! the empty-floor case a regression test cannot be measured against. `crate::identity::credential`
//! is already near its own 1000-line cap, so this module holds [`AssertionDigest`] beside the
//! [`Secret`] it completes, in the credential half of the identity module, with its tests inline
//! below.
//!
//! **What it holds, and what it deliberately cannot.** [`Secret`] is opaque but still HOLDS the
//! plaintext - its `Debug` prints `REDACTED`, which is a check that the value is absent from a log,
//! not proof that it is. This type stores the 32 SHA-256 digest bytes and never the token,
//! [`Self::of`] is the only way in and it hashes then discards, so there is no plaintext for any
//! formatter to reach. `Debug`/`Display` render a bounded hex prefix and the derived
//! `PartialEq`/`Eq` compare the digest, not secret bytes.
//!
//! Digest equality is the standard safe comparison and deliberately NOT the timing-oracle `==`
//! [`Secret`] avoids: comparing a credential returns on the first differing byte, comparing a
//! fingerprint compares a hash. [`Secret`]'s own `==` stays absent, so the only comparable shape of
//! a caller's assertion is this digest.

use sha2::{Digest as _, Sha256};

use crate::identity::Secret;

/// Bytes of a SHA-256 digest.
const DIGEST_BYTES: usize = 32;
/// Bytes of the digest a formatter may render; the rest is deliberately unreachable.
const PREFIX_BYTES: usize = 4;

/// A stable, non-reversible fingerprint of a caller's assertion, for correlation.
///
/// **One constructor, no re-exposed field, no `Deref`/`Borrow`, and no `Deserialize`** -
/// [`crate::identity`]'s standing rule that a wire document cannot mint a credential or an
/// identity: a caller-supplied body cannot become a fingerprint it chose, the way it cannot become
/// a [`Secret`] or a `RequestContext`.
///
/// ## `==` stays absent on `Secret`, so the digest is the only comparable shape
///
/// [`Secret`] has no `PartialEq` and neither does this type's source matter: comparing the token
/// itself is the timing oracle the identity skill forbids. What `==` means HERE is digest equality
/// - the two fingerprints are equal exactly when the two assertions hash the same.
///
/// ```compile_fail
/// let a = sutura_domain::identity::Secret::new("hunter2");
/// let b = sutura_domain::identity::Secret::new("hunter2");
/// assert!(a == b, "a `Secret` still has no `PartialEq` to call");
/// ```
///
/// The compiling twin, comparing the digest shape instead - the only comparable form:
///
/// ```
/// use sutura_domain::identity::{AssertionDigest, Secret};
/// let digest = AssertionDigest::of(&Secret::new("hunter2"));
/// let again = AssertionDigest::of(&Secret::new("hunter2"));
/// assert_eq!(digest, again);
/// ```
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct AssertionDigest([u8; DIGEST_BYTES]);

impl AssertionDigest {
    /// The fingerprint of one assertion: the only way in, and the one exposure.
    ///
    /// The plaintext is read through [`Secret::expose_secret`] - the ONE greppable exposure of a
    /// caller's own token - hashed with the domain's already-allowed `sha2`, and dropped. A value
    /// of this type never carries the token it was made from, so the correlation-safe sibling of
    /// [`Secret`] cannot render it either.
    #[expect(
        clippy::disallowed_methods,
        reason = "the one exposure a fingerprint is made of: hashed and dropped here, never stored or rendered"
    )]
    pub fn of(secret: &Secret) -> Self {
        let plain = secret.expose_secret();
        Self(Sha256::digest(plain.as_bytes()).into())
    }
}

impl core::fmt::Debug for AssertionDigest {
    /// A bounded prefix of the own digest, never the token it was made from.
    ///
    /// The value held is 32 bytes of hash; there is no plaintext on this type for a formatter to
    /// reach, and rendering `PREFIX_BYTES` of the digest keeps the output constant in length no
    /// matter what was hashed. The full fingerprint is available nowhere on the type's renderable
    /// surface.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "AssertionDigest(")?;
        for byte in &self.0[..PREFIX_BYTES] {
            write!(f, "{byte:02x}")?;
        }
        write!(f, ")")
    }
}

impl core::fmt::Display for AssertionDigest {
    /// Delegates to `Debug`: the same bounded prefix, so `{}` cannot widen what `{:?}` prints.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(self, f)
    }
}

#[cfg(test)]
mod tests {
    use super::AssertionDigest;
    use crate::identity::Secret;

    /// R1: the digest's rendered form is a bounded prefix, invariant to what was hashed.
    ///
    /// The digest-holding proof: a type that held the token would render caller-controlled length -
    /// a 1-char and a 10_000-char `Secret` would format to different-length `Debug`. This type
    /// holds only the 32-byte hash, so the two render identically, to a short hex prefix.
    #[test]
    fn debug_is_invariant_to_plaintext_length() {
        let short = AssertionDigest::of(&Secret::new("h"));
        let long = AssertionDigest::of(&Secret::new("x".repeat(10_000)));

        let short_rendered = format!("{short:?}");
        let long_rendered = format!("{long:?}");
        assert_eq!(
            short_rendered.len(),
            long_rendered.len(),
            "Debug length must not depend on the hashed token"
        );
        assert!(
            short_rendered.len() < 32,
            "the rendered form must be a bounded prefix, got {}",
            short_rendered.len()
        );

        for rendered in [&short_rendered, &long_rendered] {
            let inner = rendered.trim_start_matches("AssertionDigest(").trim_end_matches(')');
            assert!(
                inner.chars().all(|c| c.is_ascii_hexdigit()),
                "rendered digest must be hex-only: {rendered}"
            );
            assert_eq!(inner.len(), 8, "unbounded prefix: {rendered}");
        }
    }

    /// R1: the digest's `Debug` never contains the token it was made from.
    ///
    /// Deterministic haystack, in the house pattern - `Debug` of a parsed value must not echo the
    /// value.
    #[test]
    fn debug_never_contains_the_plaintext() {
        let digest = AssertionDigest::of(&Secret::new("hunter2-do-not-log-me"));
        let rendered = format!("{digest:?}");
        assert!(!rendered.contains("hunter2"), "Debug leaked the token: {rendered}");
    }

    /// R1: `==` on this type is digest equality - identical plaintext, identical digest.
    ///
    /// Pins the R1 contract: equal digests compare equal exactly for equal plaintext, so a caller
    /// correlating a fingerprint is comparing a hash, never secret bytes.
    #[test]
    fn equal_digests_compare_equal_only_for_equal_plaintext() {
        let a = AssertionDigest::of(&Secret::new("same-token"));
        let b = AssertionDigest::of(&Secret::new("same-token"));
        assert_eq!(a, b, "identical tokens must hash to identical digests");

        let different = AssertionDigest::of(&Secret::new("a-different-token"));
        assert_ne!(a, different, "distinct tokens must not compare equal by digest");
    }
}
