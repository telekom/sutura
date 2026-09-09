//! How one source establishes the identity a query runs as, what an adapter can carry, and what
//! each leg of an answer actually executed as.
//!
//! **Three facts by three different declarers, and conflating any two of them is how a mode acquires
//! two owners.** [Pluggable by declaration](https://github.com/telekom/sutura/blob/main/docs/adr/0011-pluggable-by-declaration.md)
//! is explicit about the split and this module is that split expressed as types:
//!
//! | Fact | Who declares it | The type here |
//! | --- | --- | --- |
//! | Which identity a query reaches this source as | configuration, per source | [`SourcePosture`] |
//! | Whether the linked adapter can carry a per-subject credential *at all* | code, per adapter | [`ImpersonationCapability`] |
//! | Which identity re-ran this source's anchors at boot | configuration, per source | [`SourceIdentity`] |
//!
//! The first two are compared at boot - [`SourcePosture::deliverable_by`] - because a posture the
//! build cannot perform is a configuration that would have to fall back, and there is no fallback.
//! The third is the *boot* identity and is deliberately a different type from anything on the
//! request path: an anchor runs before a caller exists.
//!
//! # What each leg ran as, and why it is not read off the settings tree
//!
//! [`ExecutedAs`] is what [`crate::pinned::Provenance`] carries. It is built from the posture the
//! **adapter was handed**, never from the configuration that was supposed to reach it. The two are
//! meant to agree, and if they ever disagreed the record has to say what *ran* - a field derived
//! from a file would report a leg as impersonated on the strength of a file, which is the one thing
//! this record exists to stop.
//!
//! **State the limit next to the claim.** Recording is not a control. An answer says how it was
//! executed, and provenance is read by whoever holds the answer *after* the rows were served, so it
//! cannot prevent a disclosure and does not attempt to. What keeps a shared source from being served
//! unnoticed is the boot refusal in `sutura_config::Settings::refusals` and the cross-check above,
//! both of which happen before a listener is bound.
//!
//! # One answer, one kind of identity - and this half IS a control
//!
//! [`ExecutedAs::uniform`] is the verdict, and [`UniformlyExecuted`] is what carrying it looks like:
//! `crate::pinned::PinnedDefinitions::provenance` takes only that, so an answer whose legs decide
//! identity two different ways is **unconstructible** rather than merely declined. It is a control
//! and the recording beside it is not, for the reason the paragraph above gives - a refusal reaches a
//! caller instead of the rows, and a record reaches them after.
//!
//! Two things it does not reach, both worth having in front of a reader here. It compares the posture
//! **variant** and never the value, because an acknowledgement is written per source and two ordinary
//! shared legs are therefore two unequal values and one posture. And *same posture* is not *same
//! asker*: nothing in this module or in [`crate::identity`] names WHICH shared identity a source is
//! read as.
//!
//! # Nothing here is `Deserialize`, and that is the same property [`crate::identity`] has
//!
//! [`AcknowledgementReason`] and [`VerificationIdentity`] are text an **operator** wrote, and the
//! settings layer turns the key into the type by calling `parse`. A `Deserialize` would let a value
//! reach these newtypes without passing that constructor, and for [`SharedIdentityDeclared`] it
//! would mean a witness that no operator wrote - which is exactly the state the witness exists to
//! make unreachable.
//!
//! What that does **not** claim: these constructors are `pub`, so any crate holding this one could
//! call them. The property is that a *file* cannot, and that neither type has a `Default` - so the
//! shared posture cannot be arrived at by leaving anything unset, and cannot be inherited from a
//! neighbouring source.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::SourceName;
use crate::text::first_invisible;

/// The longest operator-written reason accepted.
///
/// Wide enough for a sentence a reviewer needs and narrow enough that it cannot carry a document
/// into the line a startup log prints. The bound is here because an unbounded input is a
/// denial-of-service primitive whatever else it is, and because the value is echoed into a log.
const MAX_REASON_LEN: usize = 400;

/// The longest identity name accepted.
///
/// The same bound [`crate::identity`] puts on a principal identifier, and for the same reason: this
/// names something outside sutura - a database role, a service account - and is echoed into a
/// startup line.
const MAX_IDENTITY_LEN: usize = 256;

/// Why a piece of operator-written text is not usable here.
///
/// One error for both newtypes below, because they are one parse with two bounds. The variants carry
/// the offending input as typed fields; the `#[error]` text is a convenience for a human.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum InvalidOperatorText {
    /// Empty or whitespace-only.
    ///
    /// **The variant the whole witness rests on.** An acknowledgement whose reason is the empty
    /// string is a key an operator wrote without saying anything, which reads in a diff and in a
    /// startup log exactly like a deliberate acknowledgement and is not one.
    #[error("{name} must not be empty - write the reason, not nothing")]
    Empty { name: &'static str },
    /// Holds a control character. The startup log prints this on one line, so a newline here appends
    /// a line nobody wrote.
    #[error("{name} must not contain control characters: {value:?}")]
    ControlCharacter { name: &'static str, value: String },
    /// Holds an invisible or direction-changing code point.
    ///
    /// The second half of the reason the variant above exists: `char::is_control` is false for every
    /// one of these - general category `Cf`, not `Cc` - so the check that refuses a newline provably
    /// cannot see a right-to-left override. [`crate::text`] owns the set, so this refusal and the one
    /// a version label gets are the same refusal.
    ///
    /// The code is reported rather than the value, because text whose only defect draws nothing would
    /// print as though it were correct.
    #[error("{name} must not contain the invisible or direction-changing character {code:#06x}")]
    InvisibleCharacter { name: &'static str, code: u32 },
    #[error("{name} may be at most {limit} characters, {value:?} has {len}")]
    TooLong {
        name: &'static str,
        value: String,
        len: usize,
        limit: usize,
    },
}

/// Parses one piece of operator-written text, rejecting anything that is not one.
///
/// `name` is the configuration key, so the refusal names what to change rather than describing a
/// category. The order is the classic one - is there anything here at all, then the character
/// classes, then the bound - and the first check is deliberately first for the diagnostic rather
/// than for defence: "you wrote nothing" is the more accurate thing to tell an operator than "your
/// text has a hidden character", and the input is already bounded by the body limit above this.
fn parse_operator_text(name: &'static str, raw: &str, limit: usize) -> Result<String, InvalidOperatorText> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(InvalidOperatorText::Empty { name });
    }
    if trimmed.chars().any(char::is_control) {
        return Err(InvalidOperatorText::ControlCharacter {
            name,
            value: String::from(trimmed),
        });
    }
    if let Some(offending) = first_invisible(trimmed) {
        return Err(InvalidOperatorText::InvisibleCharacter {
            name,
            code: u32::from(offending),
        });
    }
    if trimmed.chars().count() > limit {
        return Err(InvalidOperatorText::TooLong {
            name,
            value: String::from(trimmed),
            len: trimmed.chars().count(),
            limit,
        });
    }
    Ok(String::from(trimmed))
}

/// The operator's stated reason for serving one source under one identity for every caller.
///
/// **A required value rather than a flag, because the reason is the part a reviewer needs and the
/// part nobody writes unless the type demands it.** A boolean acknowledgement records that somebody
/// clicked past a question; this records what they meant, on the source's own entry, and the startup
/// log prints it beside the posture.
///
/// Construct it with [`parse`](Self::parse). There is no other way in: the field is private, there is
/// no `Deserialize`, and `TryFrom<String>` delegates to the same constructor.
///
/// No `Default`, deliberately. A default reason is a reason nobody gave.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AcknowledgementReason(String);

impl AcknowledgementReason {
    /// The configuration key most of these are written under, named once so a refusal and the
    /// documentation cannot disagree about it.
    pub const KEY: &'static str = "sources.<alias>.acknowledged_because";

    /// The canonical constructor: parses a reason written under `key`.
    ///
    /// `key` is carried into the refusal so a message names the entry an operator has to change
    /// rather than describing a category. There are two keys in the settings tree that produce one of
    /// these - a source's own acknowledgement and the single-user mode declaration - and one parse, so
    /// a rule that held for one and not the other cannot exist.
    pub fn written_under(key: &'static str, raw: impl AsRef<str>) -> Result<Self, InvalidOperatorText> {
        parse_operator_text(key, raw.as_ref(), MAX_REASON_LEN).map(Self)
    }

    /// Parses a reason written under [`Self::KEY`].
    ///
    /// Delegates to [`Self::written_under`] rather than repeating the checks.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidOperatorText> {
        Self::written_under(Self::KEY, raw)
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for AcknowledgementReason {
    type Error = InvalidOperatorText;

    /// Delegates to [`parse`](Self::parse) rather than repeating it: one constructor stays the source
    /// of truth.
    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

impl core::fmt::Display for AcknowledgementReason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The identity the anchor path runs as, for one source.
///
/// **A distinct type from anything on the request path, on purpose.** An anchor executes at boot,
/// before any caller exists, and the credential that re-runs it must not be reachable from a
/// handler. `sutura_app::answer` holds no value of this type and cannot construct one; when the
/// execution port learns to take a credential, this is what its second method takes and the request
/// path takes the other.
///
/// It is a *name*, not material: a role or service-account name is not secret, and the credential
/// behind it arrives with the credential port. Least authority on it is a configuration requirement
/// an operator arranges - an identity holding a row-level-security bypass certifies the *unfiltered*
/// number, so the anchor passes and proves less than it appears to.
///
/// Construct it with [`parse`](Self::parse). No `Deserialize` and no `Default`, so it cannot be
/// arrived at by leaving anything unset and cannot be inherited from a neighbouring source.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct VerificationIdentity(String);

impl VerificationIdentity {
    /// The configuration key this name comes from.
    pub const KEY: &'static str = "sources.<alias>.verification_identity";

    /// Parses the declared identity, rejecting anything that is not a name.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidOperatorText> {
        parse_operator_text(Self::KEY, raw.as_ref(), MAX_IDENTITY_LEN).map(Self)
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for VerificationIdentity {
    type Error = InvalidOperatorText;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

impl core::fmt::Display for VerificationIdentity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The witness that an operator acknowledged serving one source under one identity for everybody.
///
/// It carries the reason and nothing else, and it exists so the weaker posture is **unreachable
/// without an operator's own words**. A `SharedServiceUser` posture cannot be constructed without
/// one, so there is no arrangement of a configuration file that arrives at it by leaving a key out.
///
/// **The same witness travels onto the leg.** When the credential port lands, this is the payload of
/// the third `Presented` variant - the one that carries no credential material at all - which is what
/// ties the boot-time acknowledgement to the thing that actually executed. An acknowledgement no
/// operator wrote has no value to travel, so there is no leg for it to reach.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SharedIdentityDeclared {
    reason: AcknowledgementReason,
}

impl SharedIdentityDeclared {
    /// Wraps an operator's reason as the witness.
    ///
    /// Takes the parsed reason rather than a string, so the only way to a witness is through
    /// [`AcknowledgementReason::parse`] - one canonical constructor, and this is not a second copy of
    /// its checks.
    #[inline]
    #[must_use]
    pub const fn of(reason: AcknowledgementReason) -> Self {
        Self { reason }
    }

    /// What the operator said, for the startup log.
    #[inline]
    #[must_use]
    pub const fn reason(&self) -> &AcknowledgementReason {
        &self.reason
    }
}

/// How a source establishes the identity a query runs as.
///
/// **No `Default`, and that is where it differs from `sutura_config::TlsTermination`, which is
/// otherwise the shape this copies.** That type defaults to `None` and is refused only where the bind
/// is reachable off-host - a default plus a conditional refusal - and it is right there, because a
/// loopback bind genuinely is the case where nothing needs declaring. Identity has no equivalent
/// condition: there is no bind address that makes "one identity for every caller" safe to assume. So
/// a source that declares no posture is refused unconditionally, and a `Default` here would be a
/// value that never passed a constructor.
///
/// Two variants, and the closed set is the point: an answer cannot claim a third thing happened.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourcePosture {
    /// Every query reaches this source under one identity the deployment holds. Carries the
    /// operator's acknowledgement, so it cannot be reached by leaving anything at a default.
    ///
    /// **Honest, not broken.** It is right for a single-user deployment - static credentials, one
    /// user, one host - and right for a source nobody needs to see per subject. The failure is never
    /// the posture; it is a source in this posture being *believed* to impersonate.
    SharedServiceUser { declared: SharedIdentityDeclared },
    /// Each query reaches this source as the asking subject, so the SOURCE decides what that subject
    /// sees: its own authorization, its row and column policies, its own catalog.
    ///
    /// Reaching it that way is the whole reason this posture exists - sutura carries no per-dataset
    /// classification and holds no second opinion about someone else's authorization.
    ImpersonationAtSource,
}

impl SourcePosture {
    /// Every accepted spelling, so a message and a parser cannot disagree.
    pub const NAMES: &'static [&'static str] = &["shared-service-user", "impersonation-at-source"];

    /// The spelling, for the startup log and for a wire shape.
    ///
    /// The one definition of the word, so what a log line says and what an answer carries cannot
    /// drift apart.
    #[inline]
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match *self {
            Self::SharedServiceUser { .. } => "shared-service-user",
            Self::ImpersonationAtSource => "impersonation-at-source",
        }
    }

    /// What decides what a subject sees here, as a sentence for the startup log.
    ///
    /// A function rather than a comment for the reason `TlsTermination::cleartext_hop` is one: the
    /// log, the documentation and this type read the same value, so none of them can drift into
    /// claiming this deployment impersonates when it does not.
    #[inline]
    #[must_use]
    pub const fn what_decides_what_a_caller_sees(&self) -> &'static str {
        match *self {
            Self::SharedServiceUser { .. } => {
                "one identity the deployment holds, so every caller sees the same rows - acknowledged by an operator"
            }
            Self::ImpersonationAtSource => "the source itself, evaluating the asking subject's own authorization",
        }
    }

    /// Refuses a posture the linked adapter has no way to perform.
    ///
    /// **The boot cross-check, and it is here rather than in the settings tree because half of it is
    /// a property of the BUILD.** Configuration says which posture the deployment is asking for;
    /// [`ImpersonationCapability`] says whether the code that was linked can carry a per-subject
    /// credential at all. `sutura-config` cannot see the second, so the comparison lives where both
    /// are in scope - the composition root - and this is the one function that makes it.
    ///
    /// Two exhaustive matches with no wildcard arm, so a third posture or a third capability is a
    /// compile error here rather than a case that quietly falls through to `Ok`.
    pub fn deliverable_by(&self, capability: ImpersonationCapability, source: &SourceName) -> Result<(), PostureNotDeliverable> {
        match (self, capability) {
            // One arm for the two combinations that are fine, because `match_same_arms` is denied and
            // two arms returning `Ok(())` are two arms returning `Ok(())`. It is still exhaustive over
            // the PAIR, which is the property that matters: a third posture or a third capability is a
            // compile error here rather than a case that quietly falls through.
            (Self::SharedServiceUser { .. }, _)
            | (Self::ImpersonationAtSource, ImpersonationCapability::PerSubjectCredential) => Ok(()),
            (Self::ImpersonationAtSource, ImpersonationCapability::NoPlaceForASubject) => Err(PostureNotDeliverable {
                at: source.clone(),
                posture: self.as_str(),
            }),
        }
    }
}

impl core::fmt::Display for SourcePosture {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A posture was configured that the adapter behind that source cannot perform.
///
/// Its own type rather than a string, because the composition root has to name the source in a
/// message an operator acts on, and because a refusal that carried only prose could not be asserted
/// on by a test without asserting on the prose.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "source `{at}` is configured `{posture}` and the adapter linked into this build cannot carry \
     a per-subject credential at all, so every query there would reach it as this process. There is \
     no fallback: configure `shared-service-user` with an acknowledgement, or deploy a build whose \
     adapter for that source can impersonate"
)]
pub struct PostureNotDeliverable {
    /// `at` rather than `source`, because `thiserror` reads a field called `source` as the
    /// `Error::source` chain and a `SourceName` there does not compile.
    at: SourceName,
    posture: &'static str,
}

impl PostureNotDeliverable {
    /// Which source was misconfigured.
    ///
    /// Named `at` rather than `source`, and not by preference: `thiserror`'s derive gives this type an
    /// `Error::source`, and `clippy::same_name_method` is denied - an inherent `source` beside a trait
    /// `source` is a call site whose meaning depends on which traits are in scope.
    #[inline]
    #[must_use]
    pub const fn at(&self) -> &SourceName {
        &self.at
    }
}

/// Whether an adapter can carry a per-subject credential **at all**.
///
/// A property of code, declared by the adapter as a required associated item on
/// [`crate::warehouse::Warehouse`] that it cannot omit. It is deliberately **not** the mode: an
/// adapter cannot declare a mode it does not own, because the same adapter is correct in either
/// posture and only the deployment knows which one it is being asked for.
///
/// `Copy`, because it is a declaration rather than a value with an identity, and because the boot
/// check reads it beside a posture it borrows.
///
/// **The variants are named for the MECHANISM rather than for yes and no**, which is not only style:
/// `Can...`/`Cannot...` share a postfix and `clippy::enum_variant_names` is denied, and the names that
/// survived that are the better ones anyway - a reader of `NoPlaceForASubject` at an adapter's
/// declaration is told why, not just that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpersonationCapability {
    /// There is a place in this adapter's path for a subject's own credential to arrive.
    PerSubjectCredential,
    /// There is not. An in-process engine over local files is this: one process, one operating-system
    /// identity, and nowhere for a subject to appear. Saying so explicitly is the point of the
    /// declaration - a file engine is the easiest source in the world to assume nothing about, and
    /// "nobody declared anything for the engine" is how a deployment ends up believing its whole
    /// surface impersonates because its *network* source does.
    NoPlaceForASubject,
}

impl ImpersonationCapability {
    /// The spelling, for the startup log.
    #[inline]
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PerSubjectCredential => "can carry a per-subject credential",
            Self::NoPlaceForASubject => "cannot carry a per-subject credential",
        }
    }
}

/// One source's whole identity declaration: the posture, and the identity its anchors re-run under.
///
/// **Parsed rather than validated**, and the pairing is what it parses: the two fields are not
/// independent, and three of the four combinations mean something different from the other. A
/// [`Self::declared`] that returned the struct unchecked would leave every reader to work that out
/// again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceIdentity {
    posture: SourcePosture,
    verification: Option<VerificationIdentity>,
}

/// Why a source's two identity declarations do not go together.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConflictingSourceIdentity {
    /// A verification identity was declared on a shared source, where nothing would read it.
    ///
    /// Refused rather than ignored, for the reason every unknown key in the settings tree is an
    /// error: a declaration that does nothing reads as a control that is in place. On a shared source
    /// the verification identity **is** the shared identity - there is no second credential to
    /// configure - so a name written here is either a misunderstanding or a key on the wrong entry.
    ///
    /// The field is `at` rather than `source`, and that is not a naming preference: `thiserror` reads
    /// a field called `source` as the `Error::source` chain, so a `SourceName` there does not compile.
    /// Every error in this module names its source field `at` for that reason.
    #[error(
        "source `{at}` is `shared-service-user` and also declares {key}. On a shared source the \
         verification identity IS the shared identity, so nothing would read this key. Remove it, or \
         declare the source `impersonation-at-source`"
    )]
    VerificationIdentityOnASharedSource { at: SourceName, key: &'static str },
}

impl SourceIdentity {
    /// The canonical constructor: a posture, and the verification identity if one was declared.
    ///
    /// `source` is taken for the refusal's sake alone - a message an operator acts on has to name the
    /// entry - and is not stored, because the registry that holds these is already keyed by it.
    pub fn declared(
        source: &SourceName,
        posture: SourcePosture,
        verification: Option<VerificationIdentity>,
    ) -> Result<Self, ConflictingSourceIdentity> {
        if matches!(posture, SourcePosture::SharedServiceUser { .. }) && verification.is_some() {
            return Err(ConflictingSourceIdentity::VerificationIdentityOnASharedSource {
                at: source.clone(),
                key: VerificationIdentity::KEY,
            });
        }
        Ok(Self { posture, verification })
    }

    /// Which identity a query reaches this source as.
    #[inline]
    #[must_use]
    pub const fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    /// Which identity re-runs an anchor on this source.
    ///
    /// **An exhaustive match rather than an `Option`, because the asymmetry is the useful part.** On a
    /// shared source the verification identity *is* the shared identity, so nothing is configured and
    /// an anchor is a complete claim: every caller reads that source as that one identity, so the
    /// number the anchor certifies is the number every caller gets. On an impersonating source the
    /// operator declares one, and if none is declared there is nothing to run the anchor as - which
    /// [`AnchorIdentity::NoneDeclared`] says out loud rather than answering `None` and leaving a
    /// reader to decide whether that is a permitted mode.
    #[must_use]
    pub const fn anchors_run_as(&self) -> AnchorIdentity<'_> {
        match (&self.posture, self.verification.as_ref()) {
            (SourcePosture::SharedServiceUser { declared }, _) => AnchorIdentity::TheSharedIdentity { declared },
            (SourcePosture::ImpersonationAtSource, Some(identity)) => AnchorIdentity::Declared { identity },
            (SourcePosture::ImpersonationAtSource, None) => AnchorIdentity::NoneDeclared,
        }
    }
}

/// What an anchor on one source would re-execute as.
///
/// Three variants, and the third is not the absence of the other two: it is the state a deployment
/// must not boot in if the bundle declares an anchor reading that source. Not skipped, not warned
/// about and not treated as a passing anchor, which are the three ways this would otherwise become a
/// mode nobody chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorIdentity<'declaration> {
    /// The source is shared, so the anchor runs as the one identity every caller reads it as.
    TheSharedIdentity { declared: &'declaration SharedIdentityDeclared },
    /// The source impersonates, and the operator declared a static identity for the boot path.
    Declared { identity: &'declaration VerificationIdentity },
    /// The source impersonates and nothing was declared. There is no identity to run an anchor as.
    ///
    /// A deployment that genuinely wants an impersonating source with no verification identity gets
    /// it by authoring no anchors on that source's metrics - a catalog fact a reviewer can see, rather
    /// than a runtime behaviour they have to infer.
    NoneDeclared,
}

/// What each leg of one answer executed as.
///
/// **Non-empty by construction: an answer has at least one leg.** There is no empty form and no
/// `remove`, so an answer cannot carry an execution record that claims nothing ran - which is the
/// shape `crate::pinned::PinnedDefinitions::pin` uses for the digest, applied to the other half of
/// what travels with a result.
///
/// One entry per source rather than per leg, because a plan reads a source once: two legs against one
/// source would be one leg. [`Self::and`] refuses a second entry for a source already recorded rather
/// than overwriting it, so a wiring defect that ran the same source twice under two postures is an
/// error instead of whichever value was written last.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ExecutedAs {
    by_source: BTreeMap<SourceName, SourcePosture>,
}

/// A second leg was recorded for a source that already had one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("source `{at}` already has a leg in this answer's execution record")]
pub struct LegAlreadyRecorded {
    /// `at` rather than `source`, for the `thiserror` reason the errors above give.
    at: SourceName,
}

impl ExecutedAs {
    /// One leg. The canonical constructor, and the only way a record comes into existence.
    #[must_use]
    pub fn of(source: SourceName, posture: SourcePosture) -> Self {
        let mut by_source = BTreeMap::new();
        drop(by_source.insert(source, posture));
        Self { by_source }
    }

    /// A second leg, for a federated answer.
    ///
    /// Consumes and returns, so a record is built in one expression and there is no half-built state
    /// for something else to read. The federated answer path constructs the second leg here and
    /// groups the two in [`crate::plan::federated::FederatedPlan::combine`], so the shape of this
    /// record is what decides whether a leg can be added without moving the digest - which is why it
    /// was settled before an answer format shipped rather than after.
    ///
    /// **What stood here denied both**, was true the day it was written, and stayed on the method the
    /// answer path calls until somebody read the caller - republished on a page in the nav the whole
    /// time, because `just api` regenerates a doc comment faithfully and regeneration is not
    /// verification. `check-guidance` registers that wording now, and
    /// `xtask/src/guidance/absences.rs` is the reader for the direction a registered wording cannot
    /// hold: an absence nobody has got wrong yet.
    pub fn and(mut self, source: SourceName, posture: SourcePosture) -> Result<Self, LegAlreadyRecorded> {
        if self.by_source.contains_key(&source) {
            return Err(LegAlreadyRecorded { at: source });
        }
        drop(self.by_source.insert(source, posture));
        Ok(self)
    }

    /// Every leg, by source, in source order.
    pub fn legs(&self) -> impl Iterator<Item = (&SourceName, &SourcePosture)> {
        self.by_source.iter()
    }

    /// What one source's leg ran as, if this answer has one.
    #[must_use]
    pub fn posture(&self, source: &SourceName) -> Option<&SourcePosture> {
        self.by_source.get(source)
    }

    /// This record, if every leg in it decides identity the same way.
    ///
    /// **The predicate compares the VARIANT and never the value, and that distinction is the whole
    /// of what makes this shippable.** [`SourcePosture`] derives `PartialEq` and a shared source
    /// carries the operator's own acknowledgement, which is resolved per source - so two ordinary
    /// `shared-service-user` legs whose operators wrote different sentences are two *unequal*
    /// values and one posture. A `!=` here would refuse the only federating shape that ships.
    /// [`SourcePosture::as_str`] is the variant, so the set below has one member for any number of
    /// shared legs.
    ///
    /// What it decides is *same posture*, and what it cannot decide is *same asker*:
    /// [`crate::identity::Presented::SharedServiceUser`] carries the acknowledgement witness and no
    /// identity, and nothing here names WHICH shared identity a source is read as. So two
    /// `shared-service-user` legs may be two different deployment-held identities and this passes
    /// them. Stated with the claim, because the stronger reading is the one somebody will make.
    pub fn uniform(self) -> Result<UniformlyExecuted, LegsDecideIdentityDifferently> {
        let postures: BTreeSet<&'static str> = self.by_source.values().map(SourcePosture::as_str).collect();
        if postures.len() > 1 {
            return Err(LegsDecideIdentityDifferently { postures });
        }
        Ok(UniformlyExecuted(self))
    }
}

/// An execution record whose legs all decide identity the same way.
///
/// **This exists so a mixed-posture answer is unconstructible rather than refused twice.**
/// [`crate::pinned::PinnedDefinitions::provenance`] takes one of these, `Provenance::new` is
/// private, and `ToolOutcome::Answer` carries a `Provenance` - so an answer combining two postures
/// has no way to be built, whatever a call site above it forgets to ask. The refusal in the
/// federated answer path is what stops the legs *running*; this is what stops rows *reaching a
/// caller* if that call site is ever moved below execution.
///
/// The field is private and there is no `Deserialize`, for [`SharedIdentityDeclared`]'s reason: a
/// value that reached this type without passing [`ExecutedAs::uniform`] would be the one state it
/// exists to make unreachable. What it does **not** claim is that the constructors are unreachable
/// from another crate - [`Self::of`] is `pub`, because one leg cannot disagree with itself and the
/// mono answer path has no error arm to write.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct UniformlyExecuted(ExecutedAs);

impl UniformlyExecuted {
    /// One leg, which is uniform by construction.
    ///
    /// The mono answer path's door, and it returns no `Result` deliberately: a single-leg record has
    /// one posture, so an `Err` arm there would be a refusal nothing can provoke sitting on the path
    /// every question takes. Two doors, one property - the other is [`ExecutedAs::uniform`].
    #[must_use]
    pub fn of(source: SourceName, posture: SourcePosture) -> Self {
        Self(ExecutedAs::of(source, posture))
    }

    /// Every leg, by source, in source order.
    pub fn legs(&self) -> impl Iterator<Item = (&SourceName, &SourcePosture)> {
        self.0.legs()
    }

    /// What one source's leg ran as, if this answer has one.
    #[must_use]
    pub fn posture(&self, source: &SourceName) -> Option<&SourcePosture> {
        self.0.posture(source)
    }
}

/// The legs of one answer would not all decide identity the same way.
///
/// Carries the posture LABELS and never a [`SourcePosture`], and that is a disclosure decision
/// rather than a convenience: the shared variant holds [`SharedIdentityDeclared`] ->
/// [`AcknowledgementReason`], both `Serialize`, so a value here would publish the operator's own
/// prose to whatever reads the refusal this becomes - a caller, a log, an agent's context. The
/// labels come from [`SourcePosture::NAMES`]' closed set and say the whole of what a reader needs.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "the legs of one answer would decide identity differently ({postures:?}), and this deployment \
     will not combine rows read under one identity with rows read under another into a single \
     certified number"
)]
pub struct LegsDecideIdentityDifferently {
    postures: BTreeSet<&'static str>,
}

impl LegsDecideIdentityDifferently {
    /// The posture labels this answer would have combined, in name order.
    #[inline]
    #[must_use]
    pub const fn postures(&self) -> &BTreeSet<&'static str> {
        &self.postures
    }

    /// The labels, for a refusal that carries them onward.
    #[inline]
    #[must_use]
    pub fn into_postures(self) -> BTreeSet<&'static str> {
        self.postures
    }
}

#[cfg(test)]
mod tests;
