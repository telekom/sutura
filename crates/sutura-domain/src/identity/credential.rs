//! The credential port: what one answer executes with, and who mints it.
//!
//! **Leg 2's foundation, and the reason it is a port rather than a field.** Leg 1, which
//! `docs/adr/0014` decides, establishes who is asking. It does not make a source execute as that
//! person, and a deployment that has only leg 1 knows exactly who asked and still reads every row
//! as one identity. What changes that is a credential per leg, minted per request, for the asking
//! subject - and the thing that mints one is outside the hexagon, because it talks to an
//! authorization server.
//!
//! `docs/adr/0008` is the decision. Four properties of the signature below come straight out of it
//! and each one is load-bearing:
//!
//! - **[`CredentialBroker::mint`] takes the whole [`SourceSet`], in one call.** Not one call per
//!   leg. A downstream token has to be audience-restricted to the leg it is for, so N legs need N
//!   exchanges - and one call performing N exchanges puts the asker and the deadline in ONE place
//!   instead of N. That is the hoist [`LegCredentials`] exists for.
//! - **It is synchronous**, because [`crate::warehouse::Warehouse`] is and the interior names no
//!   framework. A broker adapter does network I/O and blocks; the serving path is already
//!   synchronous down to one `block_on` for exactly that reason.
//! - **A refusal comes back in the `Ok`**, as [`Minted::Refused`]. "This subject has no credential
//!   at that source" is a governance outcome a caller may be told about; "the authorization server
//!   returned a 502" is an `Err`. Putting the first in `Err` would let a client library retry a
//!   governance decision until something works.
//! - **The error is typed per adapter.** An unreachable authorization server and a malformed
//!   response are not the same thing to whoever is paged.
//!
//! # What a broker says is not trusted, and there is one place that decides it
//!
//! A port outside the hexagon returns values the interior then acts on, so *who checks the answer*
//! is part of the port's design rather than an implementation detail of its caller. Here nothing did,
//! and a review found three ways that shows: [`LegCredentials::minted`] is `pub` and takes any
//! [`Subject`], so a broker could hand back a grant minted for somebody else; [`Expiry`] was
//! computed, carried and read by nobody; and [`Minted::Refused`] carried a [`SourceName`] the caller
//! turned into a refusal without checking it was one of the sources it had asked about.
//!
//! [`Minted::agreeing_with`] is the single guard that closes all three, and [`BoundToTheRequest`] is
//! what makes it un-skippable rather than merely conventional: **no type in this module hands out a
//! [`Presented`] except that one, and only the guard builds one.** The three findings were one
//! defect - a broker's answer being used without being compared with the request it was made for -
//! and one comparison is what they get.
//!
//! # Where the caller's own assertion lives, and what is still open about it
//!
//! **A broker that has to exchange the caller's OWN token can now read it**, and the claim is kept
//! honest about the half that is still open. [`RequestContext::assertion`] carries an
//! [`crate::identity::Secret`] where the transport established and retained one - the shape
//! `docs/adr/0008` part 2 called `Caller { subject, assertion }`, split as `chain()` and
//! `assertion()`, and `docs/adr/0014` Decision 3 is the reason it arrived with the first broker that
//! exchanges rather than before it. What was gated by that decision and is STILL open is which token
//! a given inbound mode hands to that field: the exchange chain differs per mode and needs different
//! tokens in the direct one. Having the field is not the guess the port was delayed to avoid; WHICH
//! document the transport retains to fill it is, and that remains a per-mode decision for the
//! transport that verifies. The one implementor that ships mints from configuration and never reads
//! the assertion.

use std::collections::{BTreeMap, BTreeSet};

use crate::identity::principal::{RequestContext, Subject, parse_principal_id};
use crate::identity::{InvalidPrincipalId, Secret};
use crate::model::SourceName;
use crate::source::{SharedIdentityDeclared, SourcePosture};

/// The digest-only sibling of [`Secret`]: a correlation-safe fingerprint, in its own file so its
/// definition and mutation tests live inseparable (see [`assertion_digest`]).
mod assertion_digest;
pub use assertion_digest::AssertionDigest;

/// A name a data system knows a principal by, for the posture where a session is switched to it.
///
/// **Not a [`Secret`], and that is a statement rather than an omission.** A role or service-account
/// name is not secret: the trust on that leg belongs to the connection the deployment
/// authenticated, and the name is what the data system evaluates its policies against. A type that
/// redacted it would hide the one value an operator has to be able to read back in a log.
///
/// Parsed by the same parser every principal identifier in this module goes through, so a name that
/// could forge a line in the record a call is written to does not exist. Construct it with
/// [`parse`](Self::parse): the field is private, there is no `Deserialize`, and `TryFrom<String>`
/// delegates to the same constructor.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrincipalName(String);

impl PrincipalName {
    /// Parses a name, rejecting anything that is not one.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidPrincipalId> {
        parse_principal_id(raw.as_ref()).map(Self)
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Delegates to `parse` rather than repeating it: one constructor stays the source of truth.
impl TryFrom<String> for PrincipalName {
    type Error = InvalidPrincipalId;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

impl core::fmt::Display for PrincipalName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// When everything one answer holds stops being usable.
///
/// **Two variants and no `Option`, because "nothing here expires" is a real answer rather than a
/// missing one.** A static credential an operator wrote in a file does not expire, and the
/// alternatives are both worse: an `Option<Expiry>` makes every reader decide what an absence
/// permits, and a sentinel instant makes "never" a number somebody can accidentally compare
/// against. `docs/adr/0008` part 7 is where the same argument keeps the expiry off [`Secret`] - an
/// expiring token is not a secret with a date on it, and a pre-shared bearer token has no date at
/// all.
///
/// **The domain reads no clock and it does compare this value**, and the distinction is the whole of
/// why [`Self::passed_by`] takes an argument. The instant arrives from a caller that has a clock, the
/// way [`crate::calendar::TimeRange`] carries dates a caller resolved; what lives here is the
/// direction of the comparison, once, in the type that owns the deadline.
///
/// **That is a correction rather than a refinement.** This paragraph used to say nothing in the
/// domain compares this to a clock and nothing here can, and it was true - which was the defect a
/// review found: a deadline computed correctly by [`Self::earliest`], carried through the port, and
/// read by nobody. A broker could mint a credential that had already expired and the question was
/// answered with it. `sutura_app::answer` reads the clock and [`Minted::agreeing_with`] makes the
/// comparison, before anything reaches an adapter.
///
/// `docs/adr/0008` part 6 still puts the FLOOR - is there enough life left for what this query may
/// take - in the broker adapter, which is the only component holding both a clock and the configured
/// query timeout. That is a different question from this one: the floor is a judgement about a query
/// that has not run, and [`Self::passed_by`] is a fact about a credential that is about to be
/// presented.
/// **No `Ord`, and its absence is the fix for a defect a review found.** This used to derive
/// `PartialOrd` and `Ord`, and a derived ordering on an enum is DECLARATION ORDER - so
/// [`Self::NothingExpires`] was the minimum, and `.min()` over a set holding one static credential
/// and one expiring token answered "nothing expires". That is the wrong direction, silently, in the
/// one operation this type's own documentation tells a minter to perform. The test that existed
/// pinned the inverted order and warned a reader not to read it as instants; a type should not need
/// the warning. [`Self::earliest`] is the operation, written out, and there is no comparison operator
/// left for a call site to reach for instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expiry {
    /// Nothing in this set expires. The honest answer for credentials that came from configuration.
    NothingExpires,
    /// The earliest instant at which any of it stops being usable, as seconds since the Unix epoch.
    ///
    /// Whoever mints computes the earliest across everything it minted, including the caller's own
    /// assertion where there is one - so there is one deadline for the whole answer and no way for
    /// two legs to be checked against different ones.
    At { unix_seconds: u64 },
}

impl Expiry {
    /// The earlier of two deadlines, where "nothing expires" is later than every instant.
    ///
    /// **Written out rather than derived, because the derive got it backwards.** An ordering over
    /// these two variants is not a comparison of instants: one of them is not an instant. So the
    /// question a minter asks - which of these stops being usable first - is answered by a function
    /// that names both cases, and `NothingExpires` is the one that loses to every deadline.
    #[must_use]
    pub const fn earlier_of(self, other: Self) -> Self {
        match (self, other) {
            (Self::NothingExpires, later) | (later, Self::NothingExpires) => later,
            (Self::At { unix_seconds: left }, Self::At { unix_seconds: right }) => Self::At {
                unix_seconds: if left <= right { left } else { right },
            },
        }
    }

    /// The earliest across everything a broker minted.
    ///
    /// **The operation [`Self`]'s own documentation asks a minter to perform**, so it lives here once
    /// rather than as a fold each broker writes. [`Self::NothingExpires`] is the fold's identity, and
    /// that is the honest answer for an empty set as well: a broker that minted nothing that expires
    /// has no deadline, which is what the static-credential broker that ships returns.
    #[must_use]
    pub fn earliest(deadlines: impl IntoIterator<Item = Self>) -> Self {
        deadlines.into_iter().fold(Self::NothingExpires, Self::earlier_of)
    }

    /// The instant, for a reader that has a clock. `None` where nothing expires.
    ///
    /// An `Option` rather than a number, so a caller with a clock has to name the case where there
    /// is no deadline instead of comparing against a sentinel.
    #[inline]
    #[must_use]
    pub const fn unix_seconds(self) -> Option<u64> {
        match self {
            Self::NothingExpires => None,
            Self::At { unix_seconds } => Some(unix_seconds),
        }
    }

    /// The deadline, if it has already passed at `now_unix_seconds`.
    ///
    /// **The comparison, written once, in the type that owns the value.** The paragraphs above say
    /// the domain reads no clock and they still hold: the instant arrives as an argument, the way
    /// [`crate::calendar::TimeRange`] carries dates a caller resolved. What changed is that the
    /// comparison itself is no longer scattered to whoever has a clock - there is one direction to
    /// get wrong and one place it is written.
    ///
    /// An `Option<u64>` rather than a `bool`, because the caller that finds a passed deadline has to
    /// report it and the instant is the half worth reporting: a deadline in the past by a decade and
    /// one in the past by two seconds are a defect and a clock-skew problem.
    ///
    /// **The boundary second counts as passed**, and the direction is deliberate. `not_after` is
    /// whole seconds, so at equality the credential has under a second of life left at the source -
    /// less than any query takes - and this control rounds against the deployment rather than
    /// against the data system.
    #[inline]
    #[must_use]
    pub const fn passed_by(self, now_unix_seconds: u64) -> Option<u64> {
        match self {
            Self::At { unix_seconds } if unix_seconds <= now_unix_seconds => Some(unix_seconds),
            // One arm for the two ways there is no passed deadline, because `match_same_arms` is
            // denied and they genuinely are one answer: nothing expires, and a deadline still ahead.
            Self::NothingExpires | Self::At { .. } => None,
        }
    }
}

/// What an adapter presents, for one leg.
///
/// **Three shapes, because there are three postures and the third one is not the absence of the
/// other two.** The engine that ships cannot impersonate anybody - one process, one
/// operating-system identity - so under a two-variant shape it would receive a value it ignores,
/// and "the service-identity fallback was removed" would mean the fallback came back as a variant
/// nobody looked at. Under three there is nothing to ignore: the shared leg's value holds no
/// credential material, so an adapter cannot mistake it for one, and a reader of this enum can see
/// that a third posture exists without reading an adapter.
///
/// An adapter matches exhaustively on what it received and returns its own typed error for a shape
/// it is not configured for. `docs/adr/0008` part 4 states both directions and says which is the
/// dangerous one: an adapter that quietly *accepted* subject material it cannot use would report a
/// leg as impersonated that ran shared.
#[derive(Debug)]
pub enum Presented {
    /// The asker's own bearer credential, minted for this source.
    ///
    /// For a source declared [`crate::source::SourcePosture::ImpersonationAtSource`], where the
    /// data system authenticates the subject itself.
    SubjectToken { material: Secret },
    /// A principal the data system switches to, on a connection the DEPLOYMENT authenticated, so
    /// that the query evaluates as the asker.
    ///
    /// Also `ImpersonationAtSource` - the asker's identity is what the source evaluates against -
    /// by a different mechanism. Its own variant so that the weaker trust is a shape a reader can
    /// see, never a field on the stronger one.
    SubjectPrincipal { name: PrincipalName },
    /// **No credential for the asker exists on this leg, and that is what this variant says out
    /// loud.**
    ///
    /// It carries no material at all: the payload is the operator's acknowledgement witness, which
    /// is not a secret and not a placeholder. The leg executes under the identity the deployment
    /// holds for that source, which is not the asker's, and the answer records that it did.
    SharedServiceUser { declared: SharedIdentityDeclared },
}

impl Presented {
    /// The spelling, for an adapter's own error and for a log line.
    ///
    /// One definition of each word, so a refusal naming what it was handed cannot drift from what a
    /// startup line calls the same thing.
    #[inline]
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match *self {
            Self::SubjectToken { .. } => "the asker's own credential",
            Self::SubjectPrincipal { .. } => "a principal to switch to as the asker",
            Self::SharedServiceUser { .. } => "the deployment's own identity for this source",
        }
    }

    /// Does this leg agree with how the source it is for was declared?
    ///
    /// **The check the adapters used to look like they were making and were not.** Each matched the
    /// variant it was handed against its own
    /// [`ImpersonationCapability`](crate::source::ImpersonationCapability), which answers "can this
    /// code carry a subject at all", and never read the posture the composition root handed it. So a
    /// shared leg carrying a *different* acknowledgement was accepted, and the answer's provenance
    /// then reported the adapter's own declaration rather than what the broker presented.
    ///
    /// **Two independent values compared, which is what makes this pair worth having.** The broker
    /// reads the settings tree and the adapter holds what the root handed it - `sutura_config`'s own
    /// documentation says the two are only an independent pair because they read different things -
    /// so a comparison here is a comparison and not a value against itself.
    ///
    /// **The limit, stated with the claim.** The witness is not a secret and equality of it is the
    /// only comparison available: a fabricated witness whose prose is byte-for-byte this source's is
    /// indistinguishable from this source's, and nothing here can tell them apart. What it catches is
    /// a witness that is *another* source's, one an unrelated component invented, and every mismatch
    /// of shape. It is also not a check on the credential MATERIAL in either subject variant - that
    /// is the broker's contract and, for a source that authenticates the asker, the source's.
    ///
    /// One exhaustive match over the PAIR with no wildcard arm, so a third posture or a fourth
    /// presented shape is a compile error here rather than a case that falls through to `Ok`.
    pub fn agrees_with(&self, posture: &SourcePosture, at: &SourceName) -> Result<(), PresentedDisagreesWithPosture> {
        match (self, posture) {
            (Self::SubjectToken { .. } | Self::SubjectPrincipal { .. }, SourcePosture::ImpersonationAtSource) => Ok(()),
            (Self::SharedServiceUser { declared }, SourcePosture::SharedServiceUser { declared: mine }) => {
                if declared == mine {
                    Ok(())
                } else {
                    Err(PresentedDisagreesWithPosture::WitnessIsNotThisSources { at: at.clone() })
                }
            }
            (Self::SubjectToken { .. } | Self::SubjectPrincipal { .. }, SourcePosture::SharedServiceUser { .. })
            | (Self::SharedServiceUser { .. }, SourcePosture::ImpersonationAtSource) => {
                Err(PresentedDisagreesWithPosture::ShapeIsNotThePosture {
                    at: at.clone(),
                    posture: posture.as_str(),
                    presented: self.as_str(),
                })
            }
        }
    }
}

/// What a broker presented does not agree with how the source it is for was declared.
///
/// **An `Err` on the adapter that found it and never a refusal**, for the reason a wiring defect
/// always is one here: nothing about the question was wrong, and offering it as a refusal would
/// invite a client to retry a deployment bug until something works.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PresentedDisagreesWithPosture {
    /// The leg's shape is not the shape the posture asks for - a subject's credential for a source
    /// declared shared, or the deployment's own identity for a source declared impersonating.
    #[error("source `{at}` is declared `{posture}` and was handed {presented}")]
    ShapeIsNotThePosture {
        at: SourceName,
        posture: &'static str,
        presented: &'static str,
    },
    /// Both say shared, and the acknowledgement witness on the leg is not this source's.
    ///
    /// **The case the shape check misses, and the one worth a variant of its own.** A leg carrying
    /// *some* acknowledgement matches the variant an adapter is configured for; it may still be
    /// another source's witness, or one a caller-side defect invented - both constructors on
    /// [`SharedIdentityDeclared`] are `pub`. Provenance is read off the adapter's own posture, so a
    /// leg accepted here would be recorded as running under an acknowledgement it did not carry.
    #[error("source `{at}` is declared shared under one operator acknowledgement and was handed a leg carrying another")]
    WitnessIsNotThisSources { at: SourceName },
}

/// The sources one answer reads. Non-empty by construction.
///
/// **A set rather than a list, and one call rather than N.** The port takes this whole value because
/// N audience-restricted tokens come out of ONE decision about who is asking - see the module
/// header. Non-empty because [`Self::of`] is the only way in: an answer that read no source is not
/// an answer, and a broker asked for nothing would have nothing to be complete about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSet {
    sources: BTreeSet<SourceName>,
}

impl SourceSet {
    /// One source. The canonical constructor, and the only way a set comes into existence.
    #[must_use]
    pub fn of(one: SourceName) -> Self {
        Self {
            sources: BTreeSet::from([one]),
        }
    }

    /// A second source, for a federated answer.
    ///
    /// Consumes and returns, so a set is built in one expression. A source already in the set is not
    /// an error - a plan reading one source twice reads it once - which is the difference between
    /// this and [`crate::source::ExecutedAs::and`], where a second leg for one source means a
    /// wiring defect rather than a duplicate name.
    #[must_use]
    pub fn and(mut self, another: SourceName) -> Self {
        self.sources.insert(another);
        self
    }

    /// Every source, in source order.
    pub fn iter(&self) -> impl Iterator<Item = &SourceName> {
        self.sources.iter()
    }

    /// How many sources. At least one.
    ///
    /// Named `count` rather than `len` for the reason [`crate::identity::ActorChain::count`] is: a
    /// `len` invites an `is_empty` beside it, and an `is_empty` that can only return `false` teaches
    /// the wrong thing about the type.
    #[must_use]
    pub fn count(&self) -> usize {
        self.sources.len()
    }

    /// Whether this answer reads `source`.
    #[must_use]
    pub fn contains(&self, source: &SourceName) -> bool {
        self.sources.contains(source)
    }
}

/// Everything one answer executes with: one asker, one deadline, N legs.
///
/// **The hoist is the mechanism, and it is worth being precise about what it makes true.** The
/// obvious shape is a credential per leg, each carrying its own subject, and a check that they all
/// match. That is a check: it can be moved, skipped, or written in one of two places and not the
/// other. This shape lifts every property that has to agree across legs out of the legs, and leaves
/// what genuinely differs per leg as a [`Presented`] variant rather than a field somebody reads.
///
/// Two claims are true of a value of this type:
///
/// 1. **One asker per answer.** There is one [`Self::asked_by`] field, `by_source` is private with no
///    `insert`, and [`Self::minted`] is the only constructor and takes one [`Subject`]. Two askers
///    in one answer would need two of these values, and answering takes one. Nothing is compared to
///    establish it; there is no second place for a disagreement to live.
/// 2. **No leg runs as a third identity, and the value says which of the two it ran as.**
///    [`Presented`] has three variants and no fourth: two mean "the source evaluates this as the
///    asker" and one means "the deployment's own identity for this source, acknowledged".
///
/// **What is NOT true, said in the direction that costs us:** a
/// [`Presented::SharedServiceUser`] leg does not execute as the asker, deliberately, so an answer
/// that reads one is not an answer every part of which the asker's own permissions filtered. This
/// type makes the LABELS on the legs agree with what executed; it does not make the effective
/// identities identical, because in a mixed deployment they are deliberately not. The field is
/// called `asked_by` rather than `subject` for exactly that reason - it is the identity the question
/// arrived under, and it is not a claim about every leg.
///
/// # A second leg cannot carry a second asker
///
/// There is no constructor taking a per-leg subject and no way to add a leg to a value that exists,
/// so the attempt does not compile:
///
/// ```compile_fail
/// use std::collections::BTreeMap;
/// use sutura_domain::identity::{Expiry, LegCredentials, Presented, Subject};
/// use sutura_domain::model::SourceName;
///
/// fn _two_askers(second: Presented, source: SourceName) -> LegCredentials {
///     LegCredentials {
///         asked_by: Subject::TheDeploymentItself,
///         not_after: Expiry::NothingExpires,
///         by_source: BTreeMap::from([(source, second)]),
///     }
/// }
/// ```
///
/// **A struct literal rather than a method call, and a review is why.** The block used to write
/// `credentials.and(source, second, Subject::TheDeploymentItself)`, which fails because no method
/// named `and` exists on this type - so what it proved was the absence of one name, and adding an
/// `and` for any purpose would have made it pass while the property it is named for stayed broken.
/// The literal above fails on the three private fields, which is the property: `minted` is the only
/// way to a value of this type, and it takes one [`Subject`].
///
/// The compiling twin, so a rename cannot make the block above pass vacuously - and it is out of
/// crate, which is what pins [`Self::minted`] as `pub`: a broker adapter lives in another crate and
/// this is its only way to return a value.
///
/// ```
/// use std::collections::BTreeMap;
/// use sutura_domain::identity::{Expiry, LegCredentials, Presented, SourceSet, Subject};
/// use sutura_domain::model::SourceName;
/// use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared};
///
/// let source = SourceName::parse("local")?;
/// let declared = SharedIdentityDeclared::of(AcknowledgementReason::parse("one process, one identity")?);
/// let mut presented = BTreeMap::new();
/// presented.insert(source.clone(), Presented::SharedServiceUser { declared });
///
/// let credentials = LegCredentials::minted(
///     Subject::TheDeploymentItself,
///     Expiry::NothingExpires,
///     &SourceSet::of(source),
///     presented,
/// )?;
/// assert_eq!(credentials.count(), 1);
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
#[derive(Debug)]
pub struct LegCredentials {
    asked_by: Subject,
    not_after: Expiry,
    by_source: BTreeMap<SourceName, Presented>,
}

/// A broker returned credentials that do not match the sources it was asked about.
///
/// **A wiring defect between a broker and the plan, so it is an error rather than a refusal** -
/// nothing about the question was wrong. It is refused HERE, at construction, rather than
/// discovered by whoever looks a leg up: a value of [`LegCredentials`] that exists covers exactly
/// the set it was minted for, so a caller reading one leg out of it does not need a fallback for a
/// leg the broker forgot.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CredentialsDoNotCoverThePlan {
    /// The broker was asked about a source and presented nothing for it.
    ///
    /// The direction that matters: a missing leg discovered at execution time is a leg that either
    /// runs as the process or fails somewhere with no name attached.
    #[error("nothing was minted for source `{at}`, which this answer reads")]
    Missing { at: SourceName },
    /// The broker presented something for a source this answer does not read.
    ///
    /// Refused rather than ignored, for the reason an unknown key in the settings tree is refused: a
    /// credential minted for a source nobody asked about means the broker and the plan disagree
    /// about what is being answered, and the half that is wrong may be either one.
    #[error("something was minted for source `{at}`, which this answer does not read")]
    Unasked { at: SourceName },
}

impl LegCredentials {
    /// The only constructor, and it refuses a set that does not cover the plan exactly.
    ///
    /// `sources` is what the broker was asked about, and `presented` is what it produced. Parsed
    /// rather than validated: the two are compared here, once, so every reader downstream can stop
    /// asking. The field names are the mechanism the type's own documentation describes - one
    /// `asked_by`, one `not_after`, and a private map with no way to add a leg afterwards.
    pub fn minted(
        asked_by: Subject,
        not_after: Expiry,
        sources: &SourceSet,
        presented: BTreeMap<SourceName, Presented>,
    ) -> Result<Self, CredentialsDoNotCoverThePlan> {
        let credentials = Self {
            asked_by,
            not_after,
            by_source: presented,
        };
        credentials.covers(sources)?;
        Ok(credentials)
    }

    /// Whether these legs are exactly the sources `sources` names, in both directions.
    ///
    /// **Extracted from [`Self::minted`] rather than duplicated beside it, because it is asked
    /// twice about two different sets.** The constructor asks it about the set the BROKER passed,
    /// which a broker chooses and a defective one chooses wrongly. [`Minted::agreeing_with`] asks it
    /// about the set the REQUEST was made for, which a broker does not choose - and that second
    /// question is the one a confused deputy fails. Two call sites, one comparison, so the two
    /// cannot drift.
    pub fn covers(&self, sources: &SourceSet) -> Result<(), CredentialsDoNotCoverThePlan> {
        for source in sources.iter() {
            if !self.by_source.contains_key(source) {
                return Err(CredentialsDoNotCoverThePlan::Missing { at: source.clone() });
            }
        }
        for source in self.by_source.keys() {
            if !sources.contains(source) {
                return Err(CredentialsDoNotCoverThePlan::Unasked { at: source.clone() });
            }
        }
        Ok(())
    }

    /// Who asked. One field, so N legs cannot disagree about it.
    #[inline]
    #[must_use]
    pub const fn asked_by(&self) -> &Subject {
        &self.asked_by
    }

    /// When the earliest thing in this set stops being usable.
    ///
    /// One field for the whole answer, so there is one thing to check and no way for two legs to be
    /// checked against different clocks. It IS checked, by [`Minted::agreeing_with`], against an
    /// instant the application resolved - see [`Expiry::passed_by`] for why the clock is not here.
    #[inline]
    #[must_use]
    pub const fn not_after(&self) -> Expiry {
        self.not_after
    }

    /// How many legs. At least one, and equal to the size of the set this was minted for.
    ///
    /// **The only thing this type says about its legs, and the absence of the other two accessors is
    /// the mechanism.** There used to be a `presented_for` and a `legs` here, and they were the
    /// second and third paths from a broker's answer to a value an adapter executes with - so the
    /// comparison [`Minted::agreeing_with`] makes was skippable by anybody who read a leg out
    /// directly. Both moved to [`BoundToTheRequest`], which only that comparison can build. What is
    /// left here is a count, which nothing can execute.
    ///
    /// Named `count` for the reason [`SourceSet::count`] is: a `len` invites an `is_empty` that can
    /// only answer `false`.
    #[must_use]
    pub fn count(&self) -> usize {
        self.by_source.len()
    }
}

/// What minting produced: credentials, or a refusal.
///
/// **The same two-outcome shape a compiled question has**, and for the same reason: a refusal is a
/// result. `docs/adr/0008` part 6 splits the cases - "this subject may not reach that source" is a
/// governance outcome the caller can act on, and an authorization server that answered a `502` is an
/// `Err` from the port.
///
/// **[`Self::Refused`] names a source and nothing else, which is narrower than the record it comes
/// from.** `docs/adr/0008` gave the variant a whole `RefusalReason`; a broker holding one could
/// answer that a metric is unknown, which is not a thing a credential broker knows. So the port
/// carries the one fact a broker has - which source it could not mint for - and the application
/// turns it into `crate::query::RefusalReason::CredentialUnavailable`. One refusal, one place it is
/// spelled.
#[derive(Debug)]
pub enum Minted {
    /// Everything the plan's legs need, all as one asker.
    Granted { credentials: LegCredentials },
    /// This subject has no credential at that source, and asking differently will not help: what is
    /// missing is a grant at the data system, or a different subject.
    Refused { source: SourceName },
}

/// What a broker answered, checked against the request it was asked about.
///
/// **One guard rather than three, and the reason is that the three defects it closes were one
/// defect.** A review of this port found a grant minted for another subject, a deadline nothing
/// read, and a refusal naming a source nobody asked about - three findings, each with an obvious
/// local fix, and three local fixes are three places the fourth case gets forgotten. All three are
/// the same question: **does the broker's answer agree with the request it was made for?**
/// [`Minted::agreeing_with`] asks it once, and a value of this type is what an affirmative looks
/// like.
///
/// **The check is not skippable by placement**, which is the difference between this and a rule.
/// [`BoundToTheRequest`] is the only type in this module that hands out a [`Presented`], its field
/// is private, and [`Minted::agreeing_with`] is the only thing that builds one - so the path from a
/// broker's answer to a value an adapter can execute with runs through the comparison. `Minted` is
/// still an enum whose variants a caller may match on; what it can get out of the granted one is a
/// [`LegCredentials`] with no accessor that yields a leg.
#[derive(Debug)]
pub enum Agreed {
    /// The grant agrees with the request: one asker, exactly these sources, not yet expired.
    Granted { credentials: BoundToTheRequest },
    /// This subject has no credential at that source - and the source is one the request named.
    Refused { source: SourceName },
}

/// A grant that has been checked against the request it came back for.
///
/// The wrapper is the mechanism rather than the documentation: the field is private, there is no
/// constructor beside [`Minted::agreeing_with`], and [`LegCredentials`] itself has no accessor that
/// yields a [`Presented`]. So a leg reaching `crate::warehouse::Warehouse::execute` came out of a
/// grant that was compared with the asker, the source set and the deadline - or it was fabricated
/// by its caller, which is the limit this type does not close and the adapters' own
/// [`Presented::agrees_with`] partly does.
#[derive(Debug)]
pub struct BoundToTheRequest(LegCredentials);

impl BoundToTheRequest {
    /// What to present on the leg that reads `source`.
    ///
    /// A `Result` rather than an `Option`, and the `Err` is unreachable: [`Minted::agreeing_with`]
    /// refuses a grant that does not cover the request's own [`SourceSet`] exactly, so a source in
    /// that set has a leg here. It is answered for rather than unwrapped because `unwrap_used` is
    /// denied and a panic on this path is process death under `panic = "abort"` - and it is answered
    /// HERE, once, rather than at each caller inventing what an absence means.
    pub fn presented_for(&self, source: &SourceName) -> Result<&Presented, CredentialsDoNotFitTheRequest> {
        self.0
            .by_source
            .get(source)
            .ok_or_else(|| CredentialsDoNotFitTheRequest::Coverage {
                cause: CredentialsDoNotCoverThePlan::Missing { at: source.clone() },
            })
    }

    /// Every leg, by source, in source order.
    pub fn legs(&self) -> impl Iterator<Item = (&SourceName, &Presented)> {
        self.0.by_source.iter()
    }

    /// Who asked, as the grant states it - which the check above has already compared with the
    /// subject the request arrived under.
    #[inline]
    #[must_use]
    pub const fn asked_by(&self) -> &Subject {
        self.0.asked_by()
    }

    /// When the earliest thing in this set stops being usable.
    #[inline]
    #[must_use]
    pub const fn not_after(&self) -> Expiry {
        self.0.not_after()
    }

    /// Whether this grant is still usable at `now_unix_seconds`.
    ///
    /// **One comparison, asked at every boundary a credential crosses**, which is what makes the
    /// deadline a control rather than a field. [`Minted::agreeing_with`] asks it before the grant
    /// exists, so nothing already expired is ever presented; the application asks it again between a
    /// pre-flight and an execution, because a pre-flight against a networked data system is a round
    /// trip and a credential with seconds left when it was minted may have none by the time the
    /// statement runs. The direction lives in [`Expiry::passed_by`] and is written once.
    pub const fn still_usable_at(&self, now_unix_seconds: u64) -> Result<(), CredentialsDoNotFitTheRequest> {
        // `match` and not `map_or`, and `const` is what makes that uncontroversial: a `const fn`
        // cannot call a combinator taking a closure, so `option_if_let_else` does not fire here.
        match self.not_after().passed_by(now_unix_seconds) {
            None => Ok(()),
            Some(deadline_unix_seconds) => Err(CredentialsDoNotFitTheRequest::Expired {
                deadline_unix_seconds,
                now_unix_seconds,
            }),
        }
    }
}

/// The broker's answer does not fit the request it was made for.
///
/// **An `Err` and not a refusal, for every variant, and the argument is the same one each time.** A
/// refusal is a governance outcome the caller could act on - "this subject has no credential at that
/// source" is one, and asking a different question will not change it. None of these is that. A
/// broker that answers about another subject, about another source, or with a credential that was
/// already dead when it arrived is not answering this request: either it is misconfigured or the
/// plan is, and the half that is wrong may be either one. Offering any of it as
/// `crate::query::RefusalReason::CredentialUnavailable` would tell a caller they lack access to
/// data they may be entitled to, and would let a client library retry a wiring defect forever.
///
/// **What the `#[error]` sentences carry, and what they deliberately do not.** A `SurfaceFailure` is
/// logged by the transport and never returned to a caller, so these sentences are written for
/// whoever is paged. They name the subject the way a record does - what established it, and the
/// identifier where there is one, which is what makes a bad broker mapping findable - and they carry
/// no credential material, because none of these variants holds any.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CredentialsDoNotFitTheRequest {
    /// The grant names a subject other than the one this question arrived under.
    ///
    /// **The confused deputy, and it is the one defect this port cannot merely document away.**
    /// [`LegCredentials::minted`] is `pub` and takes any [`Subject`], because a broker adapter lives
    /// in another crate and that is its only way to return a value - so nothing but a comparison
    /// stops a broker from handing back a credential minted for somebody else, by defect or by
    /// compromise. The consequence is not the label: the audit record takes the subject from the
    /// request while the leg carries whatever the broker chose, so a bad mapping executes as one
    /// principal and is recorded as another.
    #[error("the grant was minted for a different subject than the one that asked: this question is attributed to {}, and the grant says {}", attributable(.asked), attributable(.granted))]
    AnotherSubject {
        /// The subject the transport established for this request.
        asked: Subject,
        /// The subject the broker says it minted for.
        granted: Subject,
    },
    /// The grant covers a different set of sources than this answer reads.
    ///
    /// Wraps [`CredentialsDoNotCoverThePlan`] rather than restating it: the same two directions are
    /// already spelled there, and [`LegCredentials::minted`] refuses them against the set the BROKER
    /// passed. This is the same check against the set the REQUEST asked about, which is the one a
    /// broker cannot choose.
    #[error("the grant does not cover the sources this answer reads")]
    Coverage {
        #[from]
        #[source]
        cause: CredentialsDoNotCoverThePlan,
    },
    /// The refusal names a source the request did not ask about.
    ///
    /// The granted path is checked by [`Self::Coverage`]; this is the refusal path, which used to
    /// trust an arbitrary name. Answering it as a refusal would turn a broker defect into a caller
    /// being told they lack access to a source they never named - and would put that name in front
    /// of them.
    #[error("the broker refused source `{at}`, which this answer does not read")]
    RefusalNamesAnUnaskedSource {
        /// The source the broker refused.
        at: SourceName,
    },
    /// The deadline on the grant had already passed when it arrived.
    ///
    /// **The variant that makes [`Expiry`] a control rather than a field.** A broker mints and this
    /// is checked immediately afterwards, so an expired credential here is a broker that minted
    /// something dead or a clock that disagrees - not a credential that ran out during the answer.
    /// The instants are both carried, because a deadline in the past by a decade and one in the past
    /// by two seconds are a defect and a clock-skew problem respectively.
    #[error("the grant expired at {deadline_unix_seconds}, and it is now {now_unix_seconds}")]
    Expired {
        /// The deadline the grant carried, as seconds since the Unix epoch.
        deadline_unix_seconds: u64,
        /// The instant it was compared against, as the caller resolved it.
        now_unix_seconds: u64,
    },
}

/// One subject, as a line an operator reads can carry it.
///
/// What established it, and the identifier where there is one. Both halves, because
/// [`Subject::established`] alone cannot tell two verified people apart and an identifier alone
/// cannot say what proved it. A `SubjectId` is not credential material - it is what the audit record
/// already writes as its own field - and this is the same operator's log.
fn attributable(subject: &Subject) -> String {
    subject.id().map_or_else(
        || format!("a {} subject", subject.established()),
        |id| format!("a {} subject `{id}`", subject.established()),
    )
}

impl Minted {
    /// Checks this answer against the request it was made for, and refuses one that disagrees.
    ///
    /// **The one guard, and the three ways a broker's answer can contradict the request are three
    /// arms of it.** `asked_by` is the subject the transport established, `sources` is the set the
    /// broker was asked about, and `now_unix_seconds` is an instant the caller resolved.
    ///
    /// | Checked | Because |
    /// | --- | --- |
    /// | The grant's subject is the asker | Nothing else stops a broker returning somebody else's credential, and the answer would be recorded under the asker either way |
    /// | The grant covers exactly `sources` | [`LegCredentials::minted`] checks the set the broker passed, which a broker chooses; this checks the set the request asked about, which it does not |
    /// | The deadline has not passed | Otherwise [`Expiry`] is metadata: computed correctly, carried, and read by nobody |
    /// | A refusal names a source in `sources` | Otherwise a broker defect becomes a caller-facing refusal about a source they never named |
    ///
    /// # It takes the instant rather than reading a clock
    ///
    /// The domain reads no clock, in either crate, the way `crate::calendar::TimeRange` carries
    /// dates a caller resolved - so the comparison is here, where the deadline is, and the reading
    /// is the application's. That is also what makes an expired grant testable without waiting: a
    /// deadline of zero is in the past for every clock there has ever been.
    ///
    /// **The window this does not close, stated with the claim.** It runs once, before anything
    /// reaches an adapter, so what it guarantees is that no credential a broker had already let
    /// expire is presented anywhere. It does not re-check while the answer is in flight, so a
    /// credential whose life is shorter than a pre-flight plus an execution can still expire at the
    /// data system - and it is the data system that would refuse it. Covering that needs a clock
    /// this side can advance in a test rather than a `SystemTime` call, which is a port and an
    /// architecture decision; `docs/adr/0008` part 6 already puts the "is there enough life left for
    /// what this query may take" floor in the broker adapter, which is the component holding both a
    /// clock and the configured query timeout.
    pub fn agreeing_with(
        self,
        asked_by: &Subject,
        sources: &SourceSet,
        now_unix_seconds: u64,
    ) -> Result<Agreed, CredentialsDoNotFitTheRequest> {
        match self {
            Self::Refused { source } => {
                if sources.contains(&source) {
                    Ok(Agreed::Refused { source })
                } else {
                    Err(CredentialsDoNotFitTheRequest::RefusalNamesAnUnaskedSource { at: source })
                }
            }
            Self::Granted { credentials } => {
                if credentials.asked_by() != asked_by {
                    return Err(CredentialsDoNotFitTheRequest::AnotherSubject {
                        asked: asked_by.clone(),
                        granted: credentials.asked_by().clone(),
                    });
                }
                credentials.covers(sources)?;
                let bound = BoundToTheRequest(credentials);
                // The same comparison the application makes again after its pre-flight, through the
                // same method: one direction, written once, asked at every boundary the credential
                // crosses. Here it catches a broker that minted something already dead.
                bound.still_usable_at(now_unix_seconds)?;
                Ok(Agreed::Granted { credentials: bound })
            }
        }
    }
}

/// Mints the credentials one answer needs, all as one subject.
///
/// The domain declaring what it needs: an adapter outside the hexagon talks to whatever issues
/// credentials - an authorization server, or a settings tree - and conforms to this. The module
/// header carries the four properties of the signature that are decisions rather than convenience,
/// and what it cannot express yet.
pub trait CredentialBroker {
    /// Why the broker itself failed. Typed per adapter: an unreachable authorization server and a
    /// malformed response are not the same thing to whoever is paged.
    ///
    /// **It must stay distinguishable from a data system that did not answer.** `docs/adr/0014`
    /// makes the point that a caller told "unavailable, retry" against an authorization-server
    /// outage will retry successfully, while one told the same against a bound that will fire again
    /// retries forever - so the two causes must not collapse into one message on the way out.
    type Error: core::error::Error + 'static;

    /// One call per answer, for every source the plan reads.
    ///
    /// # What this costs, on the request path
    ///
    /// **It is called once per ACCEPTED question, synchronously, and it has no bound of its own.**
    /// There is no cache, no pool, no per-subject reuse and no deadline enforced here: the only
    /// ceiling is the transport's own request timeout, and nothing in the domain can see one. For the
    /// implementor that ships this is free, because the identity provider it talks to *is* the
    /// settings tree. For the first broker that exchanges a token it is **one authorization-server
    /// round trip per question**, and N audience-restricted exchanges inside it for a plan reading N
    /// sources - which is the reason the port takes the whole [`SourceSet`] in one call rather than
    /// one call per leg.
    ///
    /// Two things follow, and they are stated here rather than left to be discovered by whoever
    /// deploys the first exchanging broker. **One:** a question this deployment declines does not
    /// reach here - compilation and the source lookup run first, so an unknown metric or a dimension
    /// outside the allowlist costs nothing. `sutura_app`'s
    /// `a_refused_question_never_reaches_the_broker` is what pins the ordering, and it states the
    /// narrowness: the refusals decided AFTER minting - a result over the cap, an exhausted working
    /// set, and this port's own refusal - could not be decided before it. **Two:** caching what a
    /// broker minted is an architecture decision and not an optimisation, for the reason there is no
    /// result cache: a cache keyed on anything but the subject is a cross-subject leak, and a
    /// credential cache also has to be keyed on the deadline it is holding.
    ///
    /// Takes the [`RequestContext`] rather than a subject identifier, because who is asking is
    /// something the transport ESTABLISHED and a caller cannot state - none of the types in
    /// `crate::identity` implements `Deserialize`, so there is no code that could turn a request
    /// body into one. It takes `&self` and holds no request state, like every other port here.
    ///
    /// The caller's own assertion, where the transport retained one, is read off
    /// [`RequestContext::assertion`] - the value a broker that performs an exchange hands to an
    /// authorization server, and the value the broker that ships never reads because it mints from
    /// configuration. A broker that needs to exchange and finds none has a per-source refusal
    /// (`crate::query::RefusalReason::CredentialUnavailable`), not a leg that runs as the process.
    fn mint(&self, context: &RequestContext, sources: &SourceSet) -> Result<Minted, Self::Error>;
}

#[cfg(test)]
mod tests;
