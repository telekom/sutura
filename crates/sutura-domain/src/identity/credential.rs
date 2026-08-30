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
//! # What this port cannot express yet, said with the claim
//!
//! **A broker that has to exchange the caller's OWN token has nowhere to read it from.** `mint`
//! takes the [`RequestContext`], which carries who the caller is and not what they presented.
//! `docs/adr/0008` part 2 sketched a `Caller { subject, assertion }` for that, and `docs/adr/0014`
//! Decision 3 is why the field is deliberately absent rather than guessed: the exchange chain
//! differs per inbound mode, needs TWO exchanges in the direct one, and is **blocked on a
//! verification nobody has done** - whether a deployment's own identity provider will mint a token
//! of the required type for an audience we do not control. The shape of the value a broker would
//! exchange is exactly what that verification decides, and a `Secret` field added now would be the
//! guess the port was delayed to avoid. The one implementor that ships mints from configuration and
//! needs the subject only.
//!
//! So: adding the caller's assertion here is a change to this port's signature, and it arrives with
//! the broker adapter that performs an exchange - not before it.

use std::collections::{BTreeMap, BTreeSet};

use crate::identity::principal::{RequestContext, Subject, parse_principal_id};
use crate::identity::{InvalidPrincipalId, Secret};
use crate::model::SourceName;
use crate::source::{SharedIdentityDeclared, SourcePosture};

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
/// **Nothing in the domain compares this to a clock, and nothing here can.** The domain reads no
/// clock, in either crate - the way [`crate::calendar::TimeRange`] carries dates a caller resolved.
/// `docs/adr/0008` part 6 puts the floor - is there enough life left for what this query may take -
/// in the broker adapter, which is the only component holding both a clock and the configured query
/// timeout, and part 4 puts the same check before each leg for the same reason. What this value is
/// for HERE is the audit record: a record that cannot say how long the credential it used was good
/// for cannot answer the question an incident asks first.
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
/// assert_eq!(credentials.legs().count(), 1);
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
        for source in sources.iter() {
            if !presented.contains_key(source) {
                return Err(CredentialsDoNotCoverThePlan::Missing { at: source.clone() });
            }
        }
        for source in presented.keys() {
            if !sources.contains(source) {
                return Err(CredentialsDoNotCoverThePlan::Unasked { at: source.clone() });
            }
        }
        Ok(Self {
            asked_by,
            not_after,
            by_source: presented,
        })
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
    /// checked against different clocks. Nothing in the domain checks it - see [`Expiry`].
    #[inline]
    #[must_use]
    pub const fn not_after(&self) -> Expiry {
        self.not_after
    }

    /// What to present on the leg that reads `source`.
    ///
    /// `Option` because a caller may ask about a source this value was not minted for, and the
    /// honest answer is that there is nothing here for it. It cannot be `None` for a source in the
    /// [`SourceSet`] this was minted from - [`Self::minted`] refuses that set - which is what makes
    /// a missing leg a construction-time error rather than an execution-time one.
    #[must_use]
    pub fn presented_for(&self, source: &SourceName) -> Option<&Presented> {
        self.by_source.get(source)
    }

    /// Every leg, by source, in source order.
    pub fn legs(&self) -> impl Iterator<Item = (&SourceName, &Presented)> {
        self.by_source.iter()
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
    fn mint(&self, context: &RequestContext, sources: &SourceSet) -> Result<Minted, Self::Error>;
}

#[cfg(test)]
mod tests;
