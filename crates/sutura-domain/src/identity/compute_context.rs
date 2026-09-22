//! The value a federation boundary keys per-caller isolation on.
//!
//! **This type exists because of one line of third-party source, and the line is a comparison.**
//! `datafusion-federation`'s optimizer decides which table scans belong to one remote by comparing
//! providers, and provider equality is `name() == name() && compute_context() == compute_context()`
//! and nothing else. Two scans whose providers compare EQUAL are fused into one federated node and
//! executed through ONE of the two providers. So two executors opened for the same data system on
//! behalf of two different callers, returning the same compute context, are two callers whose scans
//! the optimizer is entitled to run on one caller's credential - a cross-caller read, produced by an
//! optimisation rather than by a bug in either caller's path. `docs/adr/0037` records the source
//! citations and the measurement.
//!
//! The corollary is the whole design: make the subject part of the comparison and the same rule that
//! produced the leak produces the isolation. Unequal providers are the optimizer's *ambiguous* case,
//! which is the path that federates each single-source sub-plan separately - to its own remote, on
//! its own credential.
//!
//! **Two requirements pull in opposite directions, and both are load-bearing.**
//!
//! * The context must **discriminate** callers, or scans fuse.
//! * The context must **not disclose** one. It is interpolated into plan text -
//!   `datafusion-federation`'s `VirtualExecutionPlan` writes ` compute_context={ctx}` into its
//!   `DisplayAs`, and its `dyn FederationProvider`/`dyn SQLExecutor` `Debug` and `Display` impls
//!   render it too - so whatever goes here reaches `EXPLAIN` output and anything that renders or
//!   logs a plan. A raw subject there is a person's identifier in plan text.
//!
//! A digest satisfies both, which is why this is a digest and not the subject. It also has to be
//! **stable**: two scans for the SAME caller on the same source must still compare equal, or
//! legitimate push-down is lost and one source's answer arrives as several.
//!
//! **What it is NOT, stated beside the claim rather than left to be discovered.** This is a plain
//! SHA-256 over low-entropy inputs. It stops a subject appearing VERBATIM in plan text and it gives
//! the optimizer an unequal comparison; it is **not** secret against anyone who can guess or
//! enumerate subjects, because they can hash their guesses and compare. Closing that needs a
//! per-process random salt - stable within a process, which is all the optimizer's comparison
//! spans - and a salt needs an entropy source the hexagon's interior does not have and may not
//! acquire casually (`xtask/src/boundaries/edges.rs`'s allowlist is the argument's venue). It is the
//! named upgrade path rather than a limitation this module pretends away.

use sha2::{Digest as _, Sha256};

use crate::identity::Subject;
use crate::model::SourceName;

/// Contributes one field to a digest as its own fixed-width digest.
///
/// **Fixed width is the injectivity.** Feeding `source || subject` as raw bytes lets `("ab", "c")`
/// and `("a", "bc")` produce one digest, and two (source, subject) pairs that produce one digest are
/// two callers the federation optimizer may fuse - the exact shape [`ComputeContext`] exists to make
/// impossible. So each field contributes exactly thirty-two bytes, which no concatenation of a
/// different field sequence can reproduce, and a caller cannot forget to do that because this is the
/// only function that writes one.
///
/// **This is DEFENCE IN DEPTH today rather than the thing holding the property, and that was
/// measured rather than argued.** Replacing this body with `hasher.update(field)` leaves the whole
/// suite green, because [`ComputeContext::of`]'s field ORDER puts a non-empty constant between the
/// two caller-influenced fields - so the slide cannot be reached through the only constructor. What
/// makes this load-bearing is a change to that order, which nothing gates. `tests::
/// a_field_boundary_that_slides_is_still_two_digests` pins the construction directly for that
/// reason, and says so where it is.
///
/// **A hash of each field rather than a length prefix, and the reason is a lint with a point.** A
/// numeric prefix has to pick a byte order, and `clippy::big_endian_bytes` and its two siblings are
/// all on in this workspace - the one that matters being the host-endian case, where two hosts
/// would disagree about one subject's context and a federated answer's isolation would depend on
/// the architecture it ran on. A fixed-width digest per field needs no byte order at all, so the
/// choice does not arise rather than being justified in an `#[expect]`.
pub(super) fn feed_field(hasher: &mut Sha256, field: &[u8]) {
    hasher.update(Sha256::digest(field));
}

/// The scheme tag, first field of every digest.
///
/// Domain separation, so a digest taken here can never equal one taken over the same bytes by
/// another mechanism in this workspace - `DefinitionDigest` is also SHA-256 over domain values, and
/// two digests that can collide across schemes are two things a future comparison could confuse. The
/// version in the tag is what lets the encoding change without a stale digest comparing equal to a
/// fresh one.
const SCHEME: &[u8] = b"sutura.compute-context.v1";

/// An opaque, stable discriminator for *which caller, on which data system* a federated leg runs as.
///
/// **There is one constructor and it takes the subject, so a context with no subject in it is not a
/// value this type has.** That is the mechanism rather than a convention: the field is private, no
/// `From`, `Default` or `parse` exists, and [`Self::of`] cannot be called without a [`Subject`] in
/// hand. A future `FederationProvider::compute_context` that returns `self.context.as_str()` is
/// therefore subject-bearing by construction, and one that returns `None` or a constant does not
/// type-check against this type at all.
///
/// Equality and ordering are over the digest, which is what the optimizer's comparison needs: equal
/// for one caller's repeated scans of one source, unequal for two callers or two sources.
///
/// **No `serde` derive, deliberately.** Nothing in [`crate::identity`]'s principal half is
/// serializable, for that module's stated reason - a type that can be read off the wire is a caller
/// stating its own identity - and a compute context is derived from a verified subject. It reaches a
/// remote as plan text, which is the adapter's business, not a wire shape this type owns.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComputeContext(String);

impl ComputeContext {
    /// The context for `subject` asking of `source`.
    ///
    /// The digest covers, each field contributing its own fixed-width digest, in this order: the
    /// scheme tag, the data
    /// system's name, the subject's own discriminant, and - for a verified caller - the full
    /// [`SubjectKey`](crate::identity::SubjectKey), which contributes itself through
    /// `SubjectKey::feed` and is never returned to this function.
    ///
    /// **Both [`Subject`] variants contribute, and the discriminant is why they cannot collide.**
    /// [`Subject::TheDeploymentItself`] is a real principal - the honest answer on a deployment whose
    /// front door proves only that a request came through it - and it has no key. Left to "no key
    /// contributes nothing" it would digest identically to any future variant that also has none,
    /// which is a fused pair waiting for a third variant. The discriminant is
    /// [`Subject::established`], which that type already guarantees is a stable label nothing
    /// caller-supplied can collide with.
    #[must_use]
    pub fn of(source: &SourceName, subject: &Subject) -> Self {
        let mut hasher = Sha256::new();
        feed_field(&mut hasher, SCHEME);
        feed_field(&mut hasher, source.as_str().as_bytes());
        feed_field(&mut hasher, subject.established().as_bytes());
        match subject.key() {
            Some(key) => key.feed(&mut hasher),
            // The absent half of the same field, rather than no field: an empty field still
            // contributes its own thirty-two bytes, so this is not the same digest as a verified
            // caller whose key happens to be empty - which `SubjectKey` refuses anyway, making this
            // belt and braces rather than the only guard.
            None => feed_field(&mut hasher, b""),
        }
        Self(
            hasher
                .finalize()
                .iter()
                .flat_map(|byte| {
                    [
                        crate::definitions::nibble(byte >> 4_u8),
                        crate::definitions::nibble(byte & 0x0f_u8),
                    ]
                })
                .collect(),
        )
    }

    /// The digest, as the text a federation provider hands the optimizer.
    ///
    /// Lower-case hex of 32 bytes. An inherent accessor rather than `Deref` or `AsRef`, per this
    /// workspace's newtype rule - `cargo xtask check-newtype-leaks` holds the other two out.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{ComputeContext, Sha256};
    use crate::identity::Subject;
    use crate::model::SourceName;
    use sha2::Digest as _;

    fn source(name: &str) -> SourceName {
        SourceName::parse(name).expect("a test source name is a source name")
    }

    fn subject(raw: &str) -> Subject {
        Subject::verified(raw).expect("a test subject is a subject")
    }

    /// **The isolation property, and it is the negative one.** Two callers on ONE data system must
    /// not produce one context: equal contexts are what `datafusion-federation`'s optimizer reads as
    /// one remote, and it then runs both callers' scans through one of the two providers. The
    /// two-SOURCE case is deliberately not what this asserts - that one federates correctly for an
    /// unrelated reason (two different provider `name()`s already compare unequal), so a cell over
    /// it would pass with the subject dropped entirely.
    #[test]
    fn two_subjects_on_one_source_do_not_share_a_compute_context() {
        let one = ComputeContext::of(&source("warehouse"), &subject("user@example.com"));
        let other = ComputeContext::of(&source("warehouse"), &subject("other@example.com"));
        assert_ne!(one, other, "two callers on one source must not fuse");
    }

    /// The deployment itself is a principal too, and it must not collide with a verified caller.
    #[test]
    fn the_deployment_itself_does_not_share_a_context_with_a_verified_caller() {
        let deployment = ComputeContext::of(&source("warehouse"), &Subject::TheDeploymentItself);
        let verified = ComputeContext::of(&source("warehouse"), &subject("user@example.com"));
        assert_ne!(deployment, verified);
    }

    /// The other half of the same coin, and it is not redundant with the cell above: a context that
    /// discriminated by being fresh each call would pass that one and lose every legitimate
    /// push-down here, because one caller's two scans of one source would look like two remotes.
    #[test]
    fn one_subject_on_one_source_reproduces_its_compute_context() {
        let first = ComputeContext::of(&source("warehouse"), &subject("user@example.com"));
        let second = ComputeContext::of(&source("warehouse"), &subject("user@example.com"));
        assert_eq!(first, second, "one caller's repeated scans must still fuse");
    }

    /// One caller on two data systems is two contexts - the "active database" half of what the
    /// upstream trait's own documentation says a compute context distinguishes.
    #[test]
    fn one_subject_on_two_sources_does_not_share_a_compute_context() {
        let here = ComputeContext::of(&source("warehouse"), &subject("user@example.com"));
        let there = ComputeContext::of(&source("elsewhere"), &subject("user@example.com"));
        assert_ne!(here, there);
    }

    /// **The disclosure half, and it is the cell the brief for this change asked for by name.** The
    /// context is interpolated into plan text by the upstream crate, so a subject identifier
    /// appearing in the rendered form is a person's identifier in `EXPLAIN` output. Asserted over
    /// both the digest and its `Debug`, because the `Debug` is what a `{:?}` of a provider reaches.
    ///
    /// The needle is a full address rather than a fragment on purpose: the haystack is lower-case
    /// hex, whose alphabet cannot spell `@` or `.` at all, so this assertion is deterministic rather
    /// than probabilistic - and it stays deterministic for the local part too, since `u`, `s`, `e`
    /// and `r` are outside `[0-9a-f]` except for `e`.
    #[test]
    fn a_rendered_compute_context_carries_no_subject_identifier() {
        let raw = "user@example.com";
        let context = ComputeContext::of(&source("warehouse"), &subject(raw));
        assert!(!context.as_str().contains(raw), "{}", context.as_str());
        assert!(!context.as_str().contains("example"), "{}", context.as_str());
        assert!(!context.as_str().contains('@'), "{}", context.as_str());
        let rendered = format!("{context:?}");
        assert!(!rendered.contains(raw), "{rendered}");
        assert!(!rendered.contains('@'), "{rendered}");
    }

    /// The shape of the rendered value, so the two cells above are asserting over what they think
    /// they are: 64 lower-case hex characters and nothing else. Without this, a constructor that
    /// returned the empty string would satisfy every "does not contain" assertion above.
    #[test]
    fn a_compute_context_is_sixty_four_hex_characters() {
        let context = ComputeContext::of(&source("warehouse"), &subject("user@example.com"));
        assert_eq!(context.as_str().len(), 64, "{}", context.as_str());
        assert!(
            context
                .as_str()
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
            "{}",
            context.as_str()
        );
    }

    /// **`feed_field`'s own property, and this cell exists in the shape it does because a mutation
    /// refuted the shape it had first.** It was written end to end - two `(source, subject)` pairs
    /// whose concatenations agree - and replacing the per-field digest with the raw bytes left the
    /// whole suite GREEN. The reason is the field order: `established()` is a non-empty constant
    /// sitting between the two caller-influenced fields, so `("ab", "c")` and `("a", "bc")` render
    /// as `ab` + `verified` + `c@…` and `a` + `verified` + `bc@…`, which do not agree. The boundary
    /// slide is therefore **unreachable through [`ComputeContext::of`] as its fields stand today**.
    ///
    /// So this pins the construction rather than the unreachable path, and the limit is the point:
    /// nothing gates that field ORDER. Remove the discriminant, or move the subject next to the
    /// source, and the slide becomes reachable - at which point `feed_field` is the only thing
    /// standing in front of it, and this is the cell that says it still does.
    #[test]
    fn a_field_boundary_that_slides_is_still_two_digests() {
        fn digest_of(fields: &[&[u8]]) -> Vec<u8> {
            let mut hasher = Sha256::new();
            for field in fields {
                super::feed_field(&mut hasher, field);
            }
            hasher.finalize().as_slice().to_vec()
        }
        assert_ne!(digest_of(&[b"ab", b"c"]), digest_of(&[b"a", b"bc"]));
    }
}
