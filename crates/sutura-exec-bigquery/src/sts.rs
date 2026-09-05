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
use std::num::NonZeroU64;

use sutura_domain::identity::{CredentialBroker, Expiry, LegCredentials, Minted, Presented, RequestContext, Secret, SourceSet};
use sutura_domain::model::SourceName;
use sutura_domain::source::SharedIdentityDeclared;

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
}

impl WorkloadIdentity {
    /// Names a provider audience and a scope for one impersonating source.
    #[must_use]
    pub const fn of(audience: String, scope: String) -> Self {
        Self { audience, scope }
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
pub struct WorkloadIdentityBroker<E, C = SystemClock> {
    exchange: E,
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
    /// Only reachable from a mint whose floor CAN fire - a declared floor over at least one exchanged
    /// deadline. A purely shared mint and a broker with no floor never ask, which is a property
    /// under test rather than a claim.
    #[error("this process could not read the time, so the exchanged-token expiry floor could not be applied")]
    NoClock {
        #[source]
        cause: std::time::SystemTimeError,
    },
}

impl<E> WorkloadIdentityBroker<E, SystemClock> {
    /// An empty broker, on the wall clock. The two `with_*` constructors add the per-source halves.
    ///
    /// **No floor**, which is the honest default for a broker whose caller has not said how long a
    /// query may take: nothing is refused here - not even an already-past expiry, which is left to
    /// the domain's `Expiry::passed_by` at the leg. It is `None` rather than a zero, so there is no
    /// second reading of what no floor means.
    #[must_use]
    pub const fn empty(exchange: E) -> Self {
        Self {
            exchange,
            clock: SystemClock,
            impersonating: BTreeMap::new(),
            shared: BTreeMap::new(),
            floor: None,
        }
    }
}

impl<E, C> WorkloadIdentityBroker<E, C> {
    /// Measures the floor against `clock` instead of the wall clock.
    ///
    /// A test hands a fixed instant and asserts the floor's decision at it; the served path never
    /// calls this and keeps [`SystemClock`]. It consumes and rebuilds rather than mutating because
    /// the clock is a type parameter - a broker on a fixed instant is a different TYPE from one on
    /// the wall clock, which is what stops a composition root acquiring one by accident.
    #[must_use]
    pub fn measured_against<K>(self, clock: K) -> WorkloadIdentityBroker<E, K>
    where
        K: UnixClock,
    {
        WorkloadIdentityBroker {
            exchange: self.exchange,
            clock,
            impersonating: self.impersonating,
            shared: self.shared,
            floor: self.floor,
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
}

impl<E, C> CredentialBroker for WorkloadIdentityBroker<E, C>
where
    E: StsExchange,
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
            let credential = self
                .exchange
                .exchange(workload.audience(), workload.scope(), assertion)
                .map_err(|cause| ExchangeUnusable::Provider { cause: Box::new(cause) })?;
            deadlines.push((source, credential.not_after()));
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
        LegCredentials::minted(context.chain().subject().clone(), not_after, sources, presented)
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
mod tests {
    use sutura_domain::identity::{
        Agreed, CredentialBroker as _, CredentialsDoNotFitTheRequest, Expiry, Minted, Presented, PrincipalChain, RequestContext,
        Secret, SourceSet, Subject, SubjectId,
    };
    use sutura_domain::model::SourceName;
    use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared};

    use super::{ExchangeUnusable, NonZeroU64, StsCredential, StsExchange, UnixClock, WorkloadIdentity, WorkloadIdentityBroker};

    /// The instant every floor decision below is measured against: 2096-10-02.
    ///
    /// **Deliberately seventy years out.** The suite this replaced minted a fixed 2027 expiry and
    /// compared it against the real `SystemTime::now()`, so it was a test SCHEDULED to go red in
    /// early 2027 - worse than a red test, because nobody would have been looking for it. The clock
    /// is an input now, so an instant far past that date is one this suite runs at today, which is
    /// the only honest demonstration that its verdict does not depend on the wall clock.
    const A_FIXED_NOW: u64 = 4_000_000_000;

    /// How long the deployment says an answer may take, as a floor. A test passing it gets a floor
    /// that CAN fire, and therefore actually consults the clock rather than skipping past it.
    const A_QUERY_BUDGET: u64 = 30;

    /// A clock frozen at one instant - what the [`UnixClock`] port exists for.
    struct Frozen(u64);

    impl UnixClock for Frozen {
        fn unix_seconds(&self) -> Result<u64, std::time::SystemTimeError> {
            Ok(self.0)
        }
    }

    /// A clock that cannot answer, so *this mint never asks what time it is* is provable rather than
    /// commented.
    ///
    /// The error is a genuine `SystemTimeError`, which has no public constructor - the only way to
    /// one is to ask for the distance from an instant to one before it, and there has never been a
    /// clock for which the epoch is in the future.
    struct NeverKnowsTheTime;

    impl UnixClock for NeverKnowsTheTime {
        fn unix_seconds(&self) -> Result<u64, std::time::SystemTimeError> {
            Err(std::time::UNIX_EPOCH
                .duration_since(std::time::SystemTime::now())
                .expect_err("the epoch is not in the future"))
        }
    }

    /// A fake exchange that mints a token echoing the caller's, so a test can assert WHOSE credential
    /// reached the leg, with a lifetime the test chose.
    ///
    /// **The lifetime is a parameter, not a constant**, so one fake covers the static credential,
    /// the one that clears the floor, the one inside it and the one already dead - and none of them
    /// can drift from the instant the broker is measured against, because both come from the test.
    struct FakeExchange {
        not_after: Expiry,
        exchanged: std::cell::RefCell<std::collections::BTreeMap<String, String>>,
    }

    impl FakeExchange {
        fn minting(not_after: Expiry) -> Self {
            Self {
                not_after,
                exchanged: std::cell::RefCell::default(),
            }
        }

        /// An exchange yielding a credential that expires `seconds` after [`A_FIXED_NOW`].
        fn minting_one_lasting(seconds: u64) -> Self {
            Self::minting(Expiry::At {
                unix_seconds: A_FIXED_NOW.saturating_add(seconds),
            })
        }
    }

    impl StsExchange for FakeExchange {
        type Error = std::convert::Infallible;

        #[expect(
            clippy::disallowed_methods,
            reason = "a fake exchange echoes the caller's token so a test can assert WHOSE credential reached the leg"
        )]
        fn exchange(&self, _audience: &str, _scope: &str, subject_token: &Secret) -> Result<StsCredential, Self::Error> {
            let raw = String::from(subject_token.expose_secret());
            self.exchanged.borrow_mut().insert(raw.clone(), raw.clone());
            Ok(StsCredential::of(Secret::new(format!("exchanged-for-{raw}")), self.not_after))
        }
    }

    fn source(raw: &str) -> SourceName {
        SourceName::parse(raw).expect("a test source is a source")
    }

    fn declared() -> SharedIdentityDeclared {
        SharedIdentityDeclared::of(
            AcknowledgementReason::parse("a read-only reporting replica every caller is entitled to see")
                .expect("a test reason is a reason"),
        )
    }

    fn caller(assertion: Option<&str>) -> RequestContext {
        let chain = PrincipalChain::of(Subject::Verified {
            id: SubjectId::parse("someone@example.com").expect("a test subject is a subject"),
        });
        match assertion {
            Some(raw) => RequestContext::with_assertion(chain, Secret::new(raw)),
            None => RequestContext::of(chain),
        }
    }

    /// One impersonating source, on the floor and the clock a test names - the served shape, minus
    /// the network and the wall clock.
    ///
    /// `floor` is an `Option` for the same reason the broker's own field is: `None` is *no floor*,
    /// and there is no third thing a zero could mean. Both it and the clock are parameters because
    /// both are what the cases below differ by.
    fn impersonating_warehouse<C>(
        exchange: FakeExchange,
        floor_seconds: Option<u64>,
        clock: C,
    ) -> WorkloadIdentityBroker<FakeExchange, C>
    where
        C: UnixClock,
    {
        let declared = WorkloadIdentityBroker::empty(exchange);
        match floor_seconds {
            Some(seconds) => declared.with_floor(seconds),
            None => declared,
        }
        .measured_against(clock)
        .impersonating(
            source("warehouse"),
            WorkloadIdentity::of(
                String::from("//iam.googleapis.com/.../providers/sso"),
                String::from("https://www.googleapis.com/auth/bigquery.readonly"),
            ),
        )
    }

    /// The credentials a mint granted, or a panic naming which of the two ways it did not.
    ///
    /// It takes no token: `caller` builds the one subject this suite has whatever assertion it is
    /// handed, and the agreement is checked against the subject, not the assertion.
    fn granted(minted: Minted) -> sutura_domain::identity::BoundToTheRequest {
        let agreed = minted
            .agreeing_with(
                caller(None).chain().subject(),
                &SourceSet::of(source("warehouse")),
                A_FIXED_NOW,
            )
            .expect("the grant agrees with the request");
        let Agreed::Granted { credentials } = agreed else {
            panic!("expected granted");
        };
        credentials
    }

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "reading the minted material IS the assertion: that the asker's own token is what was exchanged"
    )]
    fn an_impersonating_source_exchanges_the_askers_own_token_for_the_leg() {
        // **Run at an instant seventy years past the date the old shape was scheduled to fail on**,
        // over a DATED credential and a declared floor, so the floor is genuinely consulted here
        // rather than made inert by a credential with no deadline.
        let broker = impersonating_warehouse(
            FakeExchange::minting_one_lasting(3_600),
            Some(A_QUERY_BUDGET),
            Frozen(A_FIXED_NOW),
        );
        let minted = broker
            .mint(&caller(Some("caller-token")), &SourceSet::of(source("warehouse")))
            .expect("the exchange does not fail");
        let credentials = granted(minted);
        let Presented::SubjectToken { material } = credentials.presented_for(&source("warehouse")).expect("a leg") else {
            panic!("an impersonating source gets a subject token");
        };
        assert_eq!(material.expose_secret(), "exchanged-for-caller-token");
        assert_eq!(
            credentials.not_after(),
            Expiry::At {
                unix_seconds: A_FIXED_NOW + 3_600
            }
        );
    }

    #[test]
    fn an_impersonating_source_with_no_asker_token_is_refused_not_answered_as_the_process() {
        let broker = impersonating_warehouse(
            FakeExchange::minting(Expiry::NothingExpires),
            Some(A_QUERY_BUDGET),
            Frozen(A_FIXED_NOW),
        );
        let minted = broker
            .mint(&caller(None), &SourceSet::of(source("warehouse")))
            .expect("a refusal is an Ok");
        assert!(matches!(minted, Minted::Refused { source } if source.as_str() == "warehouse"));
        assert!(broker.exchange.exchanged.borrow().is_empty(), "nothing was exchanged");
    }

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "comparing the two minted values IS the assertion that two subjects are kept apart"
    )]
    fn two_subjects_get_two_different_credentials() {
        // **The acceptance criterion, at the broker boundary.** Two askers, two tokens, two distinct
        // exchanged credentials - which is exactly what lets a dataset with row-level security read a
        // different row set for each.
        let broker = impersonating_warehouse(
            FakeExchange::minting_one_lasting(3_600),
            Some(A_QUERY_BUDGET),
            Frozen(A_FIXED_NOW),
        );
        let ask = |token: &str| -> String {
            let minted = broker
                .mint(&caller(Some(token)), &SourceSet::of(source("warehouse")))
                .expect("the exchange does not fail");
            let credentials = granted(minted);
            let Presented::SubjectToken { material } = credentials.presented_for(&source("warehouse")).expect("a leg") else {
                panic!("expected a subject token");
            };
            String::from(material.expose_secret())
        };
        let first = ask("subject-a");
        let second = ask("subject-b");
        assert_eq!(first, "exchanged-for-subject-a");
        assert_eq!(second, "exchanged-for-subject-b");
        assert_ne!(first, second);
    }

    #[test]
    fn the_floor_is_a_pure_comparison_readable_with_fixed_instants() {
        use super::clears_floor;
        let now = A_FIXED_NOW;
        let floor = |seconds: u64| NonZeroU64::new(seconds).expect("a test floor is not zero");
        // A static credential never expires, so it always clears any floor.
        assert!(clears_floor(Expiry::NothingExpires, now, floor(u64::MAX)));
        // **The boundary is REFUSED, and it is the domain that decides that**: `Expiry::passed_by`
        // counts the boundary second as passed, so a credential with exactly the floor left would
        // expire at the last instant of the budget it was checked against. One second more clears.
        assert!(!clears_floor(Expiry::At { unix_seconds: now + 30 }, now, floor(30)));
        assert!(clears_floor(Expiry::At { unix_seconds: now + 31 }, now, floor(30)));
        assert!(!clears_floor(Expiry::At { unix_seconds: now + 29 }, now, floor(30)));
        // An already-passed deadline is inside any floor.
        assert!(!clears_floor(Expiry::At { unix_seconds: now }, now, floor(30)));
        assert!(!clears_floor(
            Expiry::At {
                unix_seconds: now - 100_000
            },
            now,
            floor(30)
        ));
    }

    #[test]
    fn a_credential_inside_the_floor_is_refused_rather_than_presented() {
        // The broker-level half of the floor, at the BOUNDARY rather than a decade out: one second
        // short of the query budget is refused naming the source, rather than presented and left to
        // fail at the source mid-query. Deterministic because both the deadline and the instant it
        // is compared against come from this test.
        let broker = impersonating_warehouse(
            FakeExchange::minting_one_lasting(A_QUERY_BUDGET - 1),
            Some(A_QUERY_BUDGET),
            Frozen(A_FIXED_NOW),
        );
        let minted = broker
            .mint(&caller(Some("caller-token")), &SourceSet::of(source("warehouse")))
            .expect("a refusal is an Ok");
        assert!(matches!(minted, Minted::Refused { source } if source.as_str() == "warehouse"));
    }

    #[test]
    fn a_floor_grants_a_credential_with_more_life_than_the_budget() {
        // The other side of that boundary. One second MORE than the budget clears it; exactly the
        // budget does not, because the domain counts the boundary second as already passed and this
        // adapter asks the domain rather than writing a second comparison.
        let broker = impersonating_warehouse(
            FakeExchange::minting_one_lasting(A_QUERY_BUDGET + 1),
            Some(A_QUERY_BUDGET),
            Frozen(A_FIXED_NOW),
        );
        let minted = broker
            .mint(&caller(Some("caller-token")), &SourceSet::of(source("warehouse")))
            .expect("the exchange does not fail");
        assert!(matches!(minted, Minted::Granted { .. }));
    }

    #[test]
    fn a_broker_with_no_floor_leaves_an_already_dead_credential_to_the_domain() {
        // **The `empty()` contract, and both halves of it.** With no floor declared the adapter
        // grants an already-dead credential - and the domain then refuses it at `agreeing_with`, so
        // "left to the domain" is a handoff that arrives rather than a place the check is lost.
        let broker = impersonating_warehouse(
            FakeExchange::minting(Expiry::At { unix_seconds: 1 }),
            None,
            Frozen(A_FIXED_NOW),
        );
        let minted = broker
            .mint(&caller(Some("caller-token")), &SourceSet::of(source("warehouse")))
            .expect("the exchange does not fail");
        assert!(matches!(minted, Minted::Granted { .. }), "the adapter's floor is disabled");
        let refused = minted
            .agreeing_with(
                caller(Some("caller-token")).chain().subject(),
                &SourceSet::of(source("warehouse")),
                A_FIXED_NOW,
            )
            .expect_err("the domain refuses a credential that is already dead");
        assert!(matches!(
            refused,
            CredentialsDoNotFitTheRequest::Expired {
                deadline_unix_seconds: 1,
                ..
            }
        ));
    }

    #[test]
    fn a_purely_shared_mint_never_asks_what_time_it_is() {
        // A declared floor over a source with NO exchanged deadline cannot refuse anything, so it
        // must not consult the clock - and a clock that always fails is the only way to assert
        // that it did not, rather than describe it.
        let broker = WorkloadIdentityBroker::empty(FakeExchange::minting_one_lasting(3_600))
            .with_floor(A_QUERY_BUDGET)
            .measured_against(NeverKnowsTheTime)
            .shared(source("replica"), declared());
        let minted = broker
            .mint(&caller(Some("caller-token")), &SourceSet::of(source("replica")))
            .expect("a shared mint has no need of a clock");
        assert!(matches!(minted, Minted::Granted { .. }));
    }

    #[test]
    fn a_broker_with_no_floor_never_asks_what_time_it_is() {
        // The second guard, asserted the same way: there is no floor, so there is nothing for an
        // instant to be compared against even though an exchanged deadline exists.
        let broker = impersonating_warehouse(FakeExchange::minting(Expiry::At { unix_seconds: 1 }), None, NeverKnowsTheTime);
        let minted = broker
            .mint(&caller(Some("caller-token")), &SourceSet::of(source("warehouse")))
            .expect("a zero-floor mint has no need of a clock");
        assert!(matches!(minted, Minted::Granted { .. }));
    }

    #[test]
    fn a_floor_that_can_fire_and_no_clock_to_fire_it_is_a_broker_failure() {
        // **What keeps the two tests above from being vacuous.** The same unreadable clock, over the
        // one shape that DOES need an instant - a positive floor and an exchanged deadline - fails
        // the mint as `NoClock` rather than granting. Without this, a broker that had quietly stopped
        // consulting its clock at all would pass both of them.
        let broker = impersonating_warehouse(
            FakeExchange::minting_one_lasting(3_600),
            Some(A_QUERY_BUDGET),
            NeverKnowsTheTime,
        );
        let failure = broker
            .mint(&caller(Some("caller-token")), &SourceSet::of(source("warehouse")))
            .expect_err("a floor that can fire needs an instant to fire against");
        assert!(matches!(failure, ExchangeUnusable::NoClock { .. }));
    }
}
