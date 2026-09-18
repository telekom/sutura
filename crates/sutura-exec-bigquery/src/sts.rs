//! The first [`CredentialBroker`] that performs a token exchange: a subject's own credential in, a
//! Google access token scoped to that subject out.
//!
//! **This is the broker `docs/adr/0008` part 2 called-for and the port change that carried the
//! caller's assertion exists for.** [`WorkloadIdentityBroker`] hands a verified caller's token to
//! Google's Security Token Service for a Workload Identity Federation provider (`RFC 8693`), and
//! presents the exchanged access token as a [`Presented::SubjectToken`] on the leg that reads the
//! source. It is what makes a `BigQuery` source execute as the asker rather than decline with
//! `CredentialUnavailable`.
//!
//! # What it holds, and why it needs both halves
//!
//! One broker serves a plan that may read several sources with different postures, so it holds two
//! maps by source: the shared sources it mints configuration witness for (like
//! `sutura_config::StaticCredentialBroker`), and the impersonating sources whose credentials it
//! exchanges. For the request's own asker and deadline (`docs/adr/0008` part 2) it computes the
//! earliest expiry across everything it minted.
//!
//! # Why the exchange is behind a port
//!
//! Everything this broker DECIDES - which sources exchange, that a source with no caller assertion is
//! refused rather than answered as the process, that the caller's token is never logged - is
//! exercised against a fake [`StsExchange`] that returns a canned credential. The real HTTP exchange
//! arrives at that port as [`crate::wire::StsOverHttp`], behind the default-off `wire` feature, so the
//! outbound TLS stack stays a decision a composition root makes.

use std::collections::BTreeMap;
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::Arc;
use std::time::Duration;
use base64::Engine as _;


use sutura_domain::identity::{
    CredentialBroker, Expiry, LegCredentials, Minted, Presented, RequestContext, Secret, SourceSet, SubjectKey,
};
use sutura_domain::model::SourceName;
use sutura_domain::source::SharedIdentityDeclared;

mod cache;
use cache::CredentialCache;

/// The setup one impersonating source needs from the settings tree, minus the borrowing.
///
/// Carried here rather than as a reference into configuration because an adapter may not depend on
/// the settings tree. The composition root constructs one of these per source from the parsed
/// declaration, which has already refused a value that is not usable.
#[derive(Debug, Clone)]
pub struct WorkloadIdentity {
    /// The provider audience a subject's token is exchanged against.
    audience: String,
    /// The scope the exchanged credential is minted for.
    scope: String,
    /// The declared subject -> service-account map for the second hop, telekom/sutura#376's
    /// `iamcredentials.generateAccessToken` step.
    ///
    /// **Empty is a value, not an omission.** A source with nothing here keeps the bare RFC 8693
    /// exchange - `docs/adr/0008` part 2's original shape, presented as the caller's own federated
    /// credential - which is what makes this addition additive rather than a breaking change to
    /// every source that never opts in.
    ///
    /// **Keyed on the FULL verified subject ([`SubjectKey`]), not the masked
    /// [`SubjectId`](sutura_domain::identity::SubjectId).** The
    /// hop is an authorization decision - which declared account a caller may become - and a mask
    /// collides every undeclared caller that shares a declared subject's mask with the declared one.
    /// [`SubjectKey`] holds the raw `sub` for equality while rendering only the mask, so two
    /// distinct callers stay two distinct keys and an absent caller resolves `None`.
    impersonate: BTreeMap<SubjectKey, String>,
    /// The issuer the pool trusts - the `iss` a subject token must carry for Google STS to accept
    /// it. `None` (the bare exchange) declares no link and performs no claim check, keeping today's
    /// deployment exactly; `Some` engages the `telekom/sutura#817` link to leg 1 at boot and a
    /// claim check at mint.
    expected_issuer: Option<String>,
    /// The audience the pool's provider accepts - the STS `aud` a subject token must carry.
    expected_audience: Option<String>,
}

impl WorkloadIdentity {
    /// Names a provider audience and a scope for one impersonating source, with no impersonation hop.
    #[must_use]
    pub const fn of(audience: String, scope: String) -> Self {
        Self {
            audience,
            scope,
            impersonate: BTreeMap::new(),
            expected_issuer: None,
            expected_audience: None,
        }
    }

    /// Declares the subject -> service-account map this source's hop uses.
    ///
    /// A separate builder rather than a third [`Self::of`] argument, so every existing call site -
    /// none of which impersonates a service account - reads unchanged.
    #[must_use]
    pub fn with_impersonation(mut self, impersonate: BTreeMap<SubjectKey, String>) -> Self {
        self.impersonate = impersonate;
        self
    }

    /// Declares what the pool itself trusts - the `iss` and STS `aud` a subject token must carry.
    ///
    /// A second builder rather than a wider [`Self::of`], for the same reason `with_impersonation`
    /// is: every existing call site that exchanges a bare RFC 8693 token reads unchanged, and the
    /// two roots become a thing a source can tie together rather than a cost every source pays.
    #[must_use]
    pub fn with_expectations(mut self, expected_issuer: Option<String>, expected_audience: Option<String>) -> Self {
        self.expected_issuer = expected_issuer;
        self.expected_audience = expected_audience;
        self
    }

    /// The provider audience.
    #[inline]
    #[must_use]
    pub fn audience(&self) -> &str {
        &self.audience
    }

    /// The scope the exchanged credential carries.
    #[inline]
    #[must_use]
    pub fn scope(&self) -> &str {
        &self.scope
    }

    /// The issuer the pool trusts, if the declaration named one.
    #[inline]
    #[must_use]
    pub fn expected_issuer(&self) -> Option<&str> {
        self.expected_issuer.as_deref()
    }

    /// The audience the pool's provider accepts, if the declaration named one.
    #[inline]
    #[must_use]
    pub fn expected_audience(&self) -> Option<&str> {
        self.expected_audience.as_deref()
    }

    /// The service account `subject`'s exchanged credential is impersonated into, if this source
    /// declares one.
    ///
    /// `None` is not a fallback - it is the answer for every subject a deployment did not name. A
    /// source that declares a NON-empty map refuses a `None` caller in
    /// [`WorkloadIdentityBroker::mint`] rather than answering with a bare exchange; an empty map
    /// keeps today's bare exchange for everyone.
    #[inline]
    #[must_use]
    pub fn target_for(&self, subject: &SubjectKey) -> Option<&str> {
        self.impersonate.get(subject).map(String::as_str)
    }

    /// Does the subject token carry the issuer and audience this pool declared it trusts?
    ///
    /// **The claim check for `telekom/sutura#817`'s twin-root link, at mint.** When a source names
    /// an `expected_issuer`/`expected_audience`, a subject token that does not carry them is one the
    /// pool would decline no matter what the STS answered - so it is refused here, before any
    /// round trip, rather than offered to an exchange that rejects it.
    ///
    /// The decode is deliberately UNVERIFIED: leg 1 already verified the signature, issuer, audience
    /// and lifetime before this broker ever saw the token (`RequestContext.assertion()` is the
    /// VERIFIED document, `docs/adr/0008` part 2). All that is needed here is to read the two claims
    /// the pool cares about back out of that same document and compare them to the declared values.
    ///
    /// `None` is not a failure - it is the bare exchange, which declares no link and therefore
    /// checks nothing. This is the same "absent is a value" shape `impersonate` uses.
    #[expect(
        clippy::disallowed_methods,
        reason = "reading the issuer/audience claims of the document leg 1 ALREADY verified needs \
                  its bytes once; the token is the subject-slots OWN verified document, not a \
                  process secret, and it is decoded into nothing but the two pool claims before \
                  the exchange - a bounded, purpose-named exposure like every other in this crate"
    )]
    #[must_use]
    pub fn assertion_matches_expectations(&self, assertion: &Secret) -> bool {
        let (Some(expected_issuer), Some(expected_audience)) = (self.expected_issuer(), self.expected_audience()) else {
            return true;
        };
        let Some(payload) = jwt_payload(assertion.expose_secret()) else {
            // A token whose payload does not parse is not one whose claims can be trusted to match
            // the declared pool - refuse it rather than pass the exchange a document it will decline.
            return false;
        };
        payload
            .get("iss")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|iss| iss == expected_issuer)
            && payload
                .get("aud")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|aud| aud == expected_audience)
    }
}
/// Decodes a compact JWT's payload segment as a JSON object, without verifying it.
///
/// **Unverified on purpose** - see [`WorkloadIdentity::assertion_matches_expectations`]: the caller
/// passes the document leg 1 ALREADY verified, so the only thing needed here is to read the `iss`
/// and `aud` claims back out of it. A payload that does not decode or does not parse as a JSON
/// object is `None`.
///
/// Only `serde_json`'s `Value` and a base64url decode are needed - `serde_json` derives nothing here,
/// which is why the crate carries `serde_json` and `base64` always-on rather than a full `serde` and
/// `jsonwebtoken` (the wire gate keeps the outbound stack, not JSON, off the lean build).
fn jwt_payload(assertion: &str) -> Option<serde_json::Value> {
    let payload = assertion.split('.').nth(1)?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&decoded).ok()
}

/// One exchanged credential: a Google access token and the instant it stops being usable.
///
/// The deadline is carried beside the token - the whole point of `docs/adr/0008`'s
/// [`Expiry`](sutura_domain::identity::Expiry) - so a broker can compute one deadline for the whole
/// answer and nothing answers with a token that was already dead.
///
/// **No `PartialEq`/`Eq`, because it holds a [`Secret`]** - a derived `==` on credential material is a
/// timing oracle, the same reason the domain's `Secret` has no comparison.
#[derive(Debug, Clone)]
pub struct StsCredential {
    access_token: Secret,
    not_after: Expiry,
}

impl StsCredential {
    /// Assembles an exchanged credential.
    #[must_use]
    pub const fn of(access_token: Secret, not_after: Expiry) -> Self {
        Self { access_token, not_after }
    }

    /// The exchanged access token.
    #[inline]
    #[must_use]
    pub const fn access_token(&self) -> &Secret {
        &self.access_token
    }

    /// When the token stops being usable.
    #[inline]
    #[must_use]
    pub const fn not_after(&self) -> Expiry {
        self.not_after
    }
}

/// Exchanges one subject's token for a credential to a `BigQuery` source.
///
/// **The narrow port that keeps `WorkloadIdentityBroker` testable without a network**, for the same
/// reason `crate::transport::JobTransport` exists: everything the broker decides is exercised against
/// a fake, and the HTTP exchange is one implementor behind the `wire` feature. Nothing here takes a
/// `&Warehouse` or a deadline - it is as narrow as a broker's need.
pub trait StsExchange {
    /// Why the exchange could not happen. The broker wraps it and never lets it reach a caller raw.
    ///
    /// `Send + Sync`, because the broker's failure travels across a thread boundary on the serving
    /// path - the same bound `JobTransport`'s error carries.
    type Error: core::error::Error + Send + Sync + 'static;

    /// Exchanges `subject_token` for an access token audienced to `audience` at `scope`.
    fn exchange(&self, audience: &str, scope: &str, subject_token: &Secret) -> Result<StsCredential, Self::Error>;
}

/// The second hop, telekom/sutura#376's iamcredentials step: a federated access token in, a
/// service-account access token out.
///
/// **A second port and not a second [`StsExchange`] method** - the two calls have different request
/// and response shapes (RFC 8693 token exchange vs `{scope, lifetime}`) and different failure modes
/// (STS `invalid_target` vs `iamcredentials`'s own `403` for "may not impersonate"). Everything this
/// broker decides about WHEN to call it is exercised against a fake; the real HTTP call arrives at
/// this port as [`crate::wire::IamCredentialsOverHttp`], behind the same default-off `wire` feature
/// [`StsExchange`]'s real implementor is.
pub trait ImpersonateAsAccount {
    /// Why the hop could not happen. The broker wraps it the same way it wraps [`StsExchange::Error`]
    /// and never lets it reach a caller raw.
    type Error: core::error::Error + Send + Sync + 'static;

    /// Impersonates `target_sa`, presenting `federated` (the exchanged access token from the first
    /// hop) as the bearer, for `lifetime` at `scope`.
    fn impersonate(
        &self,
        federated: &Secret,
        target_sa: &str,
        scope: &str,
        lifetime: Duration,
    ) -> Result<StsCredential, Self::Error>;
}

/// The impersonation port a broker holds when it was never wired to one.
///
/// **The default for the same reason [`SystemClock`] is one for the clock parameter**: a composition
/// root that never calls [`WorkloadIdentityBroker::impersonating_via`] gets a broker whose TYPE says
/// the hop cannot run, rather than a value that happens never to be invoked. Every test and every
/// existing call site that declares no `impersonate` entry at all never reaches
/// [`ImpersonateAsAccount::impersonate`] - [`WorkloadIdentity::target_for`] answers `None` for every
/// subject, so `mint` never calls it.
///
/// **A composition root can still reach it by mistake**, declaring a source's `impersonate` map
/// without ever calling [`WorkloadIdentityBroker::impersonating_via`] - the type system does not
/// forbid attaching an `I` and a per-source map independently, since [`WorkloadIdentityBroker::impersonating`]
/// (the per-source declaration) takes no `I` at all. So this is a REFUSAL, not an
/// invariant asserted with `unreachable!`: a caller whose subject resolves a target here is told the
/// composition is wrong, the same way [`ExchangeUnusable::Provider`] tells it about any other
/// provider defect, rather than the process panicking on a request nobody malformed.
#[derive(Debug, Clone, Copy)]
pub struct NoImpersonation;

/// A source's declared `impersonate` entry named a target and this broker holds no port to reach it.
///
/// Reachable only through a composition defect - `docs/adr`'s telekom/sutura#376 record names it as
/// the cost of keeping [`WorkloadIdentityBroker::impersonating`] (declaring a target) and
/// [`WorkloadIdentityBroker::impersonating_via`] (wiring the port that reaches one) independent
/// calls, which is what lets every source that never opts in stay on [`NoImpersonation`]'s default.
#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error(
    "a source's declared `impersonate` entry named a target, and this broker has no port wired to \
     reach it - the composition root declared the map without calling `impersonating_via`"
)]
pub struct ImpersonationNotWired;

impl ImpersonateAsAccount for NoImpersonation {
    type Error = ImpersonationNotWired;

    fn impersonate(
        &self,
        _federated: &Secret,
        _target_sa: &str,
        _scope: &str,
        _lifetime: Duration,
    ) -> Result<StsCredential, Self::Error> {
        Err(ImpersonationNotWired)
    }
}

/// Where this broker reads "now" for its expiry floor.
///
/// **A port for the same reason [`StsExchange`] is one.** Everything this broker DECIDES is
/// exercised against a fake, and the floor is one of the things it decides - so an ambient
/// `SystemTime::now()` inside [`WorkloadIdentityBroker::mint`] would make the outcome of every
/// broker-level test a function of the day it ran on. The instant is an input instead, and
/// `A_FIXED_NOW` in this file's suite records what that bought.
///
/// **The narrower shape this is NOT.** Every other time-dependent API in this workspace takes the
/// instant as a parameter - `Expiry::passed_by`, `LegCredentials::still_usable_at`,
/// `Minted::agreeing_with`, `crate::wire::AccessTokens::bearer` - and that is the better shape. It
/// is unavailable here because `CredentialBroker::mint` is a DOMAIN port signature carrying no
/// instant, and widening it reaches ten implementors across eight crates. A held clock is what an
/// adapter can do alone; the parameter is the follow-up.
pub trait UnixClock {
    /// Seconds since the Unix epoch, or the reason this process cannot say what time it is.
    ///
    /// The error is `std`'s, because that is what the shipping implementor's failure IS - a clock
    /// reading before the epoch - and a second name for it would only widen
    /// [`ExchangeUnusable::NoClock`]'s source type without telling anyone more.
    fn unix_seconds(&self) -> Result<u64, std::time::SystemTimeError>;
}

/// The wall clock: the shipping [`UnixClock`].
///
/// What [`WorkloadIdentityBroker::empty`] hands a composition root, so wiring a served deployment
/// takes no clock argument and a test that wants a fixed instant says so through
/// [`WorkloadIdentityBroker::measured_against`].
///
/// **Not the only ambient time read on this path, and the distinction is the control's limit.**
/// `crate::wire::StsOverHttp::exchange` reads its own clock to turn the provider's `expires_in`
/// into the deadline this floor then judges. So the floor's COMPARISON is deterministic; the path
/// it judges still has two clock reads in it, milliseconds apart in production and not the same
/// instant.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock;

impl UnixClock for SystemClock {
    fn unix_seconds(&self) -> Result<u64, std::time::SystemTimeError> {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_secs())
    }
}

/// A broker that mints a per-subject credential for impersonating sources and a declared witness for
/// shared ones.
#[derive(Debug, Clone)]
pub struct WorkloadIdentityBroker<E, I = NoImpersonation, C = SystemClock> {
    exchange: E,
    /// The second hop, `docs/adr` telekom/sutura#376 - [`NoImpersonation`] until a composition root
    /// calls [`Self::impersonating_via`].
    impersonation: I,
    /// Where the floor's "now" comes from - an INPUT, not an ambient read.
    clock: C,
    impersonating: BTreeMap<SourceName, WorkloadIdentity>,
    shared: BTreeMap<SourceName, SharedIdentityDeclared>,
    /// How much life a minted credential must leave for one answer - `None` for no floor at all.
    ///
    /// The FLOOR `docs/adr/0008` part 6 puts in the broker adapter, the component holding the
    /// configured query timeout: an exchanged credential that would age out *during* the answer is
    /// refused here as `Minted::Refused` rather than presented and left to fail at the source
    /// mid-query, where there is nothing for sutura to refuse.
    ///
    /// **`Option<NonZeroU64>` rather than a `u64` whose zero means disabled.** The sentinel version
    /// needed the same "is there a floor" test in two places - `mint`'s guard and `clears_floor`'s
    /// first branch - and a paragraph in each explaining that the duplication was deliberate. A
    /// type makes them unable to disagree instead, which is the difference between a contract and a
    /// convention.
    floor: Option<NonZeroU64>,
    /// The exchanged-credential cache, `docs/adr/0031` - `None` unless
    /// [`Self::with_cache`] was called, which is what makes an operator's `false` (the shipped
    /// default) a broker with nothing extra to reason about rather than a cache with a zero
    /// capacity.
    ///
    /// `Arc`, not owned outright: this broker derives `Clone`, and every clone must share ONE map -
    /// an empty second cache per clone would silently cost most of the hit rate a composition root
    /// thought it configured. Sharing state across a clone is what `Arc` is for; nothing here
    /// escapes the borrow checker with it.
    cache: Option<Arc<CredentialCache>>,
}

/// A defect in this broker itself.
#[derive(Debug, thiserror::Error)]
pub enum ExchangeUnusable {
    /// The exchange provider did not answer, or answered unusably.
    #[error("the token exchange provider did not return a usable credential: {cause}")]
    Provider {
        #[source]
        cause: Box<dyn core::error::Error + Send + Sync>,
    },
    /// The credentials built here did not cover the sources they were minted for.
    ///
    /// Unreachable: this broker builds its map from the same set it is asked about. Answered rather
    /// than unwrapped because `unwrap_used` is denied.
    #[error("the exchanged credentials did not cover the sources they were minted for")]
    Coverage {
        #[source]
        cause: sutura_domain::identity::CredentialsDoNotCoverThePlan,
    },
    /// The broker's [`UnixClock`] could not say what time it is, so the floor could not be applied.
    ///
    /// Reachable two ways now: a mint whose floor CAN fire (a declared floor over at least one
    /// exchanged deadline), or a mint where a cache (`docs/adr/0031`) is configured at all - the
    /// cache asks the same clock for its own TTL fold regardless of whether a floor exists. A
    /// purely shared mint on a broker with neither a floor nor a cache still never asks, which is a
    /// property under test rather than a claim.
    #[error("this process could not read the time, so the exchanged-token expiry floor could not be applied")]
    NoClock {
        #[source]
        cause: std::time::SystemTimeError,
    },
    /// The subject token did not carry the issuer and audience this source's pool declared it
    /// trusts - so a document leg 1 verified is one the pool would decline (telekom/sutura#817).
    ///
    /// **The token's claims are never rendered**, for the same reason no operator-written value is
    /// - the pair that mattered to the refusal is named, not the whole document.
    #[error(
        "the subject token does not carry the issuer/audience this source's identity pool declared \
         it accepts - a document `security.inbound` verified is one the pool would decline"
    )]
    PoolExpectation,
}

impl<E> WorkloadIdentityBroker<E, NoImpersonation, SystemClock> {
    /// An empty broker, on the wall clock, with no impersonation hop wired. The `with_*` constructors
    /// add the per-source halves; [`Self::impersonating_via`] wires the second hop.
    ///
    /// **No floor**, which is the honest default for a broker whose caller has not said how long a
    /// query may take: nothing is refused here - not even an already-past expiry, which is left to
    /// the domain's `Expiry::passed_by` at the leg. It is `None` rather than a zero, so there is no
    /// second reading of what no floor means.
    #[must_use]
    pub const fn empty(exchange: E) -> Self {
        Self {
            exchange,
            impersonation: NoImpersonation,
            clock: SystemClock,
            impersonating: BTreeMap::new(),
            shared: BTreeMap::new(),
            floor: None,
            cache: None,
        }
    }
}

impl<E, I, C> WorkloadIdentityBroker<E, I, C> {
    /// Measures the floor against `clock` instead of the wall clock.
    ///
    /// A test hands a fixed instant and asserts the floor's decision at it; the served path never
    /// calls this and keeps [`SystemClock`]. It consumes and rebuilds rather than mutating because
    /// the clock is a type parameter - a broker on a fixed instant is a different TYPE from one on
    /// the wall clock, which is what stops a composition root acquiring one by accident.
    #[must_use]
    pub fn measured_against<K>(self, clock: K) -> WorkloadIdentityBroker<E, I, K>
    where
        K: UnixClock,
    {
        WorkloadIdentityBroker {
            exchange: self.exchange,
            impersonation: self.impersonation,
            clock,
            impersonating: self.impersonating,
            shared: self.shared,
            floor: self.floor,
            cache: self.cache,
        }
    }

    /// Wires the second hop: telekom/sutura#376's `iamcredentials.generateAccessToken` step.
    ///
    /// A composition root that serves ANY source declaring an `impersonate` map entry must call this,
    /// or a caller whose subject resolves that entry is refused with [`ImpersonationNotWired`] rather
    /// than silently answered with a bare exchange for a subject the deployment meant to hop. Consumes and rebuilds for the same
    /// reason [`Self::measured_against`] does: the port is a type parameter, so a broker with the real
    /// hop wired is a different TYPE from one without.
    #[must_use]
    pub fn impersonating_via<J>(self, impersonation: J) -> WorkloadIdentityBroker<E, J, C>
    where
        J: ImpersonateAsAccount,
    {
        WorkloadIdentityBroker {
            exchange: self.exchange,
            impersonation,
            clock: self.clock,
            impersonating: self.impersonating,
            shared: self.shared,
            floor: self.floor,
            cache: self.cache,
        }
    }

    /// Declares the expiry FLOOR: the minimum life, in seconds, a minted credential must have left
    /// for one answer (the configured query timeout). A composition root that knows the timeout wires
    /// it here; a caller that does not want one leaves the default.
    ///
    /// **Zero parses to no floor**, and this is the one place that reading happens. The served path
    /// cannot reach it - `sutura_config::RequestTimeout::parse` already refuses a zero timeout - so
    /// the conversion is here for a caller that computed the number rather than parsed it.
    #[must_use]
    pub const fn with_floor(mut self, floor_seconds: u64) -> Self {
        self.floor = NonZeroU64::new(floor_seconds);
        self
    }

    /// Declares a source this broker mints for as the deployment's own shared identity.
    #[must_use]
    pub fn shared(mut self, source: SourceName, declared: SharedIdentityDeclared) -> Self {
        drop(self.shared.insert(source, declared));
        self
    }

    /// Declares a source whose asker's credential this broker exchanges.
    #[must_use]
    pub fn impersonating(mut self, source: SourceName, workload: WorkloadIdentity) -> Self {
        drop(self.impersonating.insert(source, workload));
        self
    }

    /// How many impersonating sources this broker exchanges for. Read by this crate's own suite.
    #[must_use]
    pub fn impersonating_count(&self) -> usize {
        self.impersonating.len()
    }

    /// Turns on the exchanged-credential cache, `docs/adr/0031` - off unless a composition root
    /// calls this. `capacity` bounds the number of live entries; `window` is the operator's own
    /// ceiling on top of the credential's own life, never the other way - see
    /// `sutura_config::identity_cache::CacheWindow`'s own doc for why a window cannot LENGTHEN what
    /// was minted.
    #[must_use]
    pub fn with_cache(mut self, capacity: NonZeroUsize, window: Duration) -> Self {
        self.cache = Some(Arc::new(CredentialCache::new(capacity, window)));
        self
    }

    /// The capacity a composition root's boot line names, read from THIS broker's own state rather
    /// than the setting that (maybe) built it - `None` when [`Self::with_cache`] was never called.
    ///
    /// A boot line built from the setting alone can drift from what the broker actually holds: the
    /// two agree only because one `if` gates both today, and nothing stops a future edit widening
    /// one arm without the other. Reading it back through this accessor is what keeps the printed
    /// line and the broker's own state the same fact.
    #[must_use]
    pub fn cache_capacity(&self) -> Option<NonZeroUsize> {
        self.cache.as_ref().map(|cache| cache.capacity())
    }
}

/// The lifetime this broker requests at the second hop - Google's own documented ceiling for
/// `iamcredentials.generateAccessToken`.
///
/// **A constant rather than a value derived from the source's own configured query timeout.** The
/// broker's own expiry FLOOR already refuses a credential that would age out mid-answer, whatever its
/// lifetime; requesting anything less here would only narrow the cache's own hit window
/// (`sts/cache.rs`) for no compensating safety, since nothing downstream trusts a longer lifetime as
/// a wider grant - the source's own authorization decides that per query, every time.
const IMPERSONATED_LIFETIME: Duration = Duration::from_secs(3_600);

impl<E, I, C> CredentialBroker for WorkloadIdentityBroker<E, I, C>
where
    E: StsExchange,
    I: ImpersonateAsAccount,
    C: UnixClock,
{
    type Error = ExchangeUnusable;

    fn mint(&self, context: &RequestContext, sources: &SourceSet) -> Result<Minted, Self::Error> {
        // Pass one is a refusal and pass two a build, and the order is the point: nothing is
        // minted before every source's credential is known to exist. A source this broker holds
        // NEITHER half for is refused rather than answered as the process, and the caller is told
        // which source rather than told to retry.
        for source in sources.iter() {
            if !self.shared.contains_key(source) && !self.impersonating.contains_key(source) {
                return Ok(Minted::Refused { source: source.clone() });
            }
        }
        // The asker's own token, which a broker that exchanges REQUIRES - a subject with no
        // credential at a source is refused, never answered as the process.
        let assertion = context.assertion();
        // The subject `LegCredentials::minted` takes as `asked_by` below - one field, so N legs
        // cannot disagree about who asked.
        let asked_by = context.chain().subject();
        // The WHOLE chain, which is what `docs/adr/0031`'s cache keys on - not just `asked_by`
        // above. A chain the transport built from an RFC 8693 `act` claim differs from the same
        // subject's direct chain, and the cache has to tell them apart even though `asked_by` alone
        // would not.
        let chain = context.chain();
        // Read once, only when a cache exists to consult at all - the same "ask only when needed"
        // shape the floor already holds, extended by one more reason to need the time. A second,
        // independent read happens further down for the floor itself when both are configured;
        // that duplication is cheap and left alone rather than restructured around a clock this
        // change did not otherwise need to touch.
        let cache_now = if self.cache.is_some() {
            Some(
                self.clock
                    .unix_seconds()
                    .map_err(|cause| ExchangeUnusable::NoClock { cause })?,
            )
        } else {
            None
        };

        let mut presented = BTreeMap::new();
        // The exchanged deadlines and the source each came from, so the FLOOR can name the source
        // whose credential would age out mid-answer. A shared leg contributes nothing - a static
        // credential never expires, which is the case `clears_floor` answers without a clock read.
        //
        // The name is BORROWED from `sources`, which outlives this call: only the refusal arm needs
        // an owned one, and it clones once on the path that is already returning.
        let mut deadlines: Vec<(&SourceName, Expiry)> = Vec::new();
        for source in sources.iter() {
            if let Some(declared) = self.shared.get(source) {
                drop(presented.insert(
                    source.clone(),
                    Presented::SharedServiceUser {
                        declared: declared.clone(),
                    },
                ));
                continue;
            }
            // An impersonating source. The asker's own credential is the thing being exchanged, so
            // without one the leg cannot run as the asker.
            let Some(workload) = self.impersonating.get(source) else {
                return Ok(Minted::Refused { source: source.clone() });
            };
            let Some(assertion) = assertion else {
                return Ok(Minted::Refused { source: source.clone() });
            };
            // **The twin-root link, `telekom/sutura#817`, refused here before any round trip.** When
            // a source declares the issuer/audience its pool accepts, a subject token that does not
            // carry them is a document leg 1 would verify but the pool would decline - so it is
            // refused as a provider defect rather than offered to an exchange that rejects it.
            if !workload.assertion_matches_expectations(assertion) {
                return Err(ExchangeUnusable::PoolExpectation);
            }
            // The hop's target, if this source declares one for THIS subject - resolved before any
            // round trip because it is part of the cache key (`sts/cache.rs`'s own doc: the SA is
            // itself part of "what was asked for") and part of the decision whether to call the
            // second hop at all.
            //
            // Keyed on the FULL verified `sub` (`asked_by.key()`), never on the masked `SubjectId`
            // (`asked_by.id()`): the hop is the authorization decision "which declared account may
            // this caller become", and a masked lookup would hand a declared subject's account to
            // every undeclared caller sharing its mask.
            let target_sa = asked_by.key().and_then(|key| workload.target_for(key));

            // **REFUSAL, `docs/adr/0032`'s "absent from the map is refused before any network call":**
            // a source that DECLARES a non-empty map has said who may execute here, and a verified
            // caller with no entry either becomes one of those declared accounts or is a bare
            // federated principal the source never sanctioned. Refused at the door - before the
            // cache, before either round trip - naming the SOURCE and never the subject. An EMPTY
            // map is the additive case and keeps today's bare exchange for everyone.
            if !workload.impersonate.is_empty() && target_sa.is_none() {
                return Ok(Minted::Refused { source: source.clone() });
            }

            // A live entry, if the cache holds one for this exact chain, this exact (audience,
            // scope), AND this exact target SA - never for anything less, see `cache`'s own module
            // doc. A hit skips both round trips entirely; nothing below this arm runs for that
            // source.
            if let (Some(cache), Some(now)) = (&self.cache, cache_now)
                && let Some(hit) = cache.get(chain, workload, target_sa, now)
            {
                deadlines.push((source, hit.not_after));
                drop(presented.insert(source.clone(), Presented::SubjectToken { material: hit.material }));
                continue;
            }

            let federated = self
                .exchange
                .exchange(workload.audience(), workload.scope(), assertion)
                .map_err(|cause| ExchangeUnusable::Provider { cause: Box::new(cause) })?;
            // **The hop, immediately after a successful exchange.** A target names the FINAL
            // credential this leg presents - the federated one is never itself presented once a
            // target is declared, which is the property `the_resulting_bearer_is_the_sa_token_...`
            // pins: a broker that skipped this and reused `federated` is the mutation that cell
            // exists to kill.
            let credential = match target_sa {
                Some(target) => self
                    .impersonation
                    .impersonate(federated.access_token(), target, workload.scope(), IMPERSONATED_LIFETIME)
                    .map_err(|cause| ExchangeUnusable::Provider { cause: Box::new(cause) })?,
                None => federated,
            };
            deadlines.push((source, credential.not_after()));
            // Populated from exactly this arm, right after a successful exchange (and, where
            // declared, a successful hop) - there is no other call to `put` anywhere in this broker,
            // which is what makes "never cache a refusal or an error" true by absence rather than by
            // a check. Two round trips are cached as the one entry the FINAL credential is, keyed on
            // the target too - never two entries for one leg.
            if let (Some(cache), Some(now)) = (&self.cache, cache_now) {
                cache.put(
                    chain,
                    workload,
                    target_sa,
                    credential.access_token().clone(),
                    credential.not_after(),
                    self.floor,
                    now,
                );
            }
            drop(presented.insert(
                source.clone(),
                Presented::SubjectToken {
                    material: credential.access_token().clone(),
                },
            ));
        }
        // **One deadline for the whole answer, from the earliest exchange** - the field an audit
        // record names. Nothing from configuration contributes, which `Expiry::earliest`'s
        // `NothingExpires` identity already folds away; the per-source pair is folded here so the
        // FLOOR can name the source it refuses.
        let not_after = Expiry::earliest(deadlines.iter().map(|(_, expiry)| *expiry));
        // **The FLOOR, `docs/adr/0008` part 6, and it is why `with_floor` exists:** this broker
        // refuses to hand back a credential already inside the floor rather than presenting it and
        // letting a leg fail at the source mid-query. It lives here because this is the component
        // holding (via the composition root) the configured query timeout.
        //
        // **The clock is CONSULTED only when the floor can fire, and it is an input.** A broker with
        // no floor cannot refuse anything, and neither can one whose sources all contributed static
        // credentials - so neither asks what time it is, and a clock that cannot answer does not
        // fail a mint with no need of one. Three tests hold that against a clock that always fails.
        if let Some(floor) = self.floor
            && !deadlines.is_empty()
        {
            let now_unix = self
                .clock
                .unix_seconds()
                .map_err(|cause| ExchangeUnusable::NoClock { cause })?;
            if let Some((source, _)) = deadlines.iter().find(|(_, expiry)| !clears_floor(*expiry, now_unix, floor)) {
                return Ok(Minted::Refused {
                    source: (*source).clone(),
                });
            }
        }
        LegCredentials::minted(asked_by.clone(), not_after, sources, presented)
            .map(|credentials| Minted::Granted { credentials })
            .map_err(|cause| ExchangeUnusable::Coverage { cause })
    }
}

/// Does `not_after` leave more life than an answer taking `floor` could need, after `now`?
///
/// **It asks the domain rather than comparing.** `Expiry::passed_by` is documented as *the*
/// comparison - "there is one direction to get wrong and one place it is written" - so this asks it
/// about an instant a floor into the future instead of writing a second `>=` beside it. What that
/// buys is the boundary: the domain counts the boundary second as PASSED, deliberately, because
/// `not_after` is whole seconds and equality leaves under a second of life at the source. A
/// credential with EXACTLY `floor` seconds left is therefore refused here - it would expire at the
/// last instant of the budget it was checked against, and this control rounds against the deployment
/// the same way the domain's does.
///
/// `NothingExpires` clears any floor: a static credential has no deadline to age out, which is what
/// `passed_by` answers `None` for.
const fn clears_floor(not_after: Expiry, now_unix_seconds: u64, floor: NonZeroU64) -> bool {
    not_after.passed_by(now_unix_seconds.saturating_add(floor.get())).is_none()
}

#[cfg(test)]
mod tests;
