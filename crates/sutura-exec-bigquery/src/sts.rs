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

/// A broker that mints a per-subject credential for impersonating sources and a declared witness for
/// shared ones.
#[derive(Debug, Clone)]
pub struct WorkloadIdentityBroker<E> {
    exchange: E,
    impersonating: BTreeMap<SourceName, WorkloadIdentity>,
    shared: BTreeMap<SourceName, SharedIdentityDeclared>,
    /// How much life a minted credential must leave for one answer, in seconds.
    ///
    /// The FLOOR `docs/adr/0008` part 6 puts in the broker adapter - the only component with both a
    /// clock and the configured query timeout. An exchanged credential that would age out *during*
    /// the answer is refused here as `Minted::Refused` rather than presented and left to fail at the
    /// source mid-query, where there is nothing for sutura to refuse.
    floor_seconds: u64,
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
    /// This process could not read a wall clock, so the expiry floor could not be applied.
    #[error("this process could not read the time, so the exchanged-token expiry floor could not be applied")]
    NoClock {
        #[source]
        cause: std::time::SystemTimeError,
    },
}

impl<E> WorkloadIdentityBroker<E> {
    /// An empty broker. The two `with_*` constructors add the per-source halves.
    ///
    /// **No floor**, which is the honest default for a broker whose caller has not said how long a
    /// query may take: `floor_seconds` of zero refuses nothing, so an expiry that far in the past is
    /// still caught only by the domain's `Expiry::passed_by` check, not by this adapter's floor.
    #[must_use]
    pub const fn empty(exchange: E) -> Self {
        Self {
            exchange,
            impersonating: BTreeMap::new(),
            shared: BTreeMap::new(),
            floor_seconds: 0,
        }
    }

    /// Declares the expiry FLOOR: the minimum life, in seconds, a minted credential must have left
    /// for one answer (the configured query timeout). A composition root that knows the timeout wires
    /// it here; a test that does not want one leaves the default.
    #[must_use]
    pub const fn with_floor(mut self, floor_seconds: u64) -> Self {
        self.floor_seconds = floor_seconds;
        self
    }

    /// Declares a source this broker mis as the deployment's own shared identity.
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

impl<E> CredentialBroker for WorkloadIdentityBroker<E>
where
    E: StsExchange,
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
        let mut deadlines: Vec<(SourceName, Expiry)> = Vec::new();
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
            deadlines.push((source.clone(), credential.not_after()));
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
        // with both a clock and (via the composition root) the configured query timeout.
        //
        // **The clock is read only when the floor can fire.** A purely shared mint has no exchanged
        // deadline to age out, and a ZERO floor means floor disabled (the `empty()` contract) - in
        // both of those shapes nothing can be refused here, so no `SystemTime` read happens and a
        // `SystemTimeError` cannot fail a mint that has no need of the time. Both guards are on the
        // one branch below: `!deadlines.is_empty()` and `self.floor_seconds > 0`.
        if !deadlines.is_empty() && self.floor_seconds > 0 {
            let now_unix = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|cause| ExchangeUnusable::NoClock { cause })?
                .as_secs();
            if let Some((source, _)) = deadlines
                .iter()
                .find(|(_, expiry)| !clears_floor(*expiry, now_unix, self.floor_seconds))
            {
                return Ok(Minted::Refused { source: source.clone() });
            }
        }
        LegCredentials::minted(context.chain().subject().clone(), not_after, sources, presented)
            .map(|credentials| Minted::Granted { credentials })
            .map_err(|cause| ExchangeUnusable::Coverage { cause })
    }
}

/// Does `not_after` leave enough life for an answer that may take `floor_seconds`, after `now`?
///
/// **The FLOOR comparison, written once and purely**, so a composition root can wire a query timeout
/// as the floor and the timeout's exact semantics are testable with fixed instants rather than against
/// a wall clock. `NothingExpires` always clears the floor - a static credential has no deadline to
/// age out - and a deadline clears it when it is at least a full floor away.
///
/// **A ZERO floor means floor disabled** (the `empty()` contract): nothing at all is refused, so even
/// an already-past deadline clears it and is left to the domain's `Expiry::passed_by` check at the
/// leg. That guard lives here rather than only at [`WorkloadIdentityBroker::mint`]'s call site so the
/// pure function and the broker cannot disagree about what zero means.
const fn clears_floor(not_after: Expiry, now_unix_seconds: u64, floor_seconds: u64) -> bool {
    if floor_seconds == 0 {
        return true;
    }
    match not_after.unix_seconds() {
        None => true,
        Some(unix) => unix >= now_unix_seconds.saturating_add(floor_seconds),
    }
}

#[cfg(test)]
mod tests {
    use sutura_domain::identity::{
        Agreed, CredentialBroker as _, Expiry, Minted, Presented, PrincipalChain, RequestContext, Secret, SourceSet, Subject,
        SubjectId,
    };
    use sutura_domain::model::SourceName;

    use super::{StsCredential, StsExchange, WorkloadIdentity, WorkloadIdentityBroker};

    /// A fake exchange that mints a token echoing the caller's, so a test can assert WHOSE credential
    /// reached the leg.
    ///
    /// **It mints with NO lifetime on purpose.** These tests are about WHO was exchanged, not WHEN it
    /// expires, so a fixed expiry would make them depend on the wall clock (an expiry far enough ahead
    /// today is one that ages out next year); a static credential has no deadline, which is the honest
    /// shape for a test that asserts the asker, and it is also what keeps them deterministic forever.
    #[derive(Default)]
    struct FakeExchange {
        exchanged: std::cell::RefCell<std::collections::BTreeMap<String, String>>,
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
            Ok(StsCredential::of(
                Secret::new(format!("exchanged-for-{raw}")),
                Expiry::NothingExpires,
            ))
        }
    }

    /// A fake exchange that mints a credential ALREADY DEAD, so the FLOOR's two branches - refuse a
    /// positive floor, leave a zero floor to the domain - are both deterministic against any clock:
    /// unix second `1` is in the past for every clock there has been.
    #[derive(Default)]
    struct DyingFake;

    impl StsExchange for DyingFake {
        type Error = std::convert::Infallible;

        fn exchange(&self, _audience: &str, _scope: &str, _subject_token: &Secret) -> Result<StsCredential, Self::Error> {
            Ok(StsCredential::of(
                Secret::new("exchanged-for-a-credential-already-dead"),
                Expiry::At { unix_seconds: 1 },
            ))
        }
    }

    fn source(raw: &str) -> SourceName {
        SourceName::parse(raw).expect("a test source is a source")
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

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "reading the minted material IS the assertion: that the asker's own token is what was exchanged"
    )]
    fn an_impersonating_source_exchanges_the_askers_own_token_for_the_leg() {
        let broker = WorkloadIdentityBroker::empty(FakeExchange::default()).impersonating(
            source("warehouse"),
            WorkloadIdentity::of(
                String::from("//iam.googleapis.com/.../providers/sso"),
                String::from("https://www.googleapis.com/auth/bigquery.readonly"),
            ),
        );
        let minted = broker
            .mint(&caller(Some("caller-token")), &SourceSet::of(source("warehouse")))
            .expect("the exchange does not fail");
        let agreed = minted
            .agreeing_with(
                caller(Some("caller-token")).chain().subject(),
                &SourceSet::of(source("warehouse")),
                1_799_999_999,
            )
            .expect("the grant agrees with the request");
        let Agreed::Granted { credentials } = agreed else {
            panic!("expected granted");
        };
        let Presented::SubjectToken { material } = credentials.presented_for(&source("warehouse")).expect("a leg") else {
            panic!("an impersonating source gets a subject token");
        };
        assert_eq!(material.expose_secret(), "exchanged-for-caller-token");
        // No lifetime: the fake mints a static credential, so the answer carries `NothingExpires` -
        // what this test is about is WHOSE material reached the leg, asserted just above.
        assert_eq!(credentials.not_after(), Expiry::NothingExpires);
    }

    #[test]
    fn an_impersonating_source_with_no_ascer_token_is_refused_not_answered_as_the_process() {
        let exchanger = FakeExchange::default();
        let broker = WorkloadIdentityBroker::empty(exchanger).impersonating(
            source("warehouse"),
            WorkloadIdentity::of(String::from("audience"), String::from("scope")),
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
        let broker = WorkloadIdentityBroker::empty(FakeExchange::default()).impersonating(
            source("warehouse"),
            WorkloadIdentity::of(String::from("audience"), String::from("scope")),
        );
        let ask = |token: &str| -> String {
            let minted = broker
                .mint(&caller(Some(token)), &SourceSet::of(source("warehouse")))
                .expect("the exchange does not fail");
            let agreed = minted
                .agreeing_with(
                    caller(Some(token)).chain().subject(),
                    &SourceSet::of(source("warehouse")),
                    1_799_999_999,
                )
                .expect("agrees");
            let Agreed::Granted { credentials } = agreed else {
                panic!("expected granted");
            };
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
        let now = 1_800_000_000u64;
        // A static credential never expires, so it always clears any floor.
        assert!(clears_floor(Expiry::NothingExpires, now, u64::MAX));
        // Exactly the floor left is enough; one second less is refused.
        assert!(clears_floor(Expiry::At { unix_seconds: now + 30 }, now, 30));
        assert!(!clears_floor(Expiry::At { unix_seconds: now + 29 }, now, 30));
        // An already-passed deadline is inside any positive floor.
        assert!(!clears_floor(Expiry::At { unix_seconds: now }, now, 30));
        // **A ZERO floor is floor disabled, including for an already-past deadline** - that an
        // already-dead token is refused is the DOMAIN's `Expiry::passed_by` job, not this adapter's,
        // which is exactly what the `empty()` contract promises.
        assert!(clears_floor(Expiry::At { unix_seconds: now }, now, 0));
        assert!(clears_floor(
            Expiry::At {
                unix_seconds: now - 100_000
            },
            now,
            0
        ));
    }

    #[test]
    fn a_credential_inside_the_floor_is_refused_rather_than_presented() {
        // The broker-level half of the floor, deterministic against any clock: the fake mints an
        // already-dead credential (unix second `1`), and a positive floor refuses it naming the
        // source rather than presenting it and letting the leg fail at the source mid-query.
        let broker = WorkloadIdentityBroker::empty(DyingFake).with_floor(30).impersonating(
            source("warehouse"),
            WorkloadIdentity::of(String::from("audience"), String::from("scope")),
        );
        let minted = broker
            .mint(&caller(Some("caller-token")), &SourceSet::of(source("warehouse")))
            .expect("a refusal is an Ok");
        assert!(matches!(minted, Minted::Refused { source } if source.as_str() == "warehouse"));
    }

    #[test]
    fn a_zero_floor_leaves_an_already_dead_credential_to_the_domain() {
        // The other half of the floor's contract, and what keeps the refusal above honest: with the
        // default (zero) floor the SAME already-dead credential is granted - the adapter does not
        // refuse it here, because the domain's `Expiry::passed_by` rejects a dead deadline at the
        // leg before anything is presented. The refusal above is the floor, and this is not a red
        // mint.
        let broker = WorkloadIdentityBroker::empty(DyingFake).impersonating(
            source("warehouse"),
            WorkloadIdentity::of(String::from("audience"), String::from("scope")),
        );
        let minted = broker
            .mint(&caller(Some("caller-token")), &SourceSet::of(source("warehouse")))
            .expect("the exchange does not fail");
        assert!(matches!(minted, Minted::Granted { .. }));
    }

    #[test]
    fn a_positive_floor_grants_a_credential_that_clears_it() {
        // A positive floor rejects only what ages out within it: a credential with no lifetime (the
        // shape `FakeExchange` mints) always clears it, so a broker wired with a query budget still
        // serves a source whose exchange yields a static credential.
        let broker = WorkloadIdentityBroker::empty(FakeExchange::default())
            .with_floor(30)
            .impersonating(
                source("warehouse"),
                WorkloadIdentity::of(String::from("audience"), String::from("scope")),
            );
        let minted = broker
            .mint(&caller(Some("caller-token")), &SourceSet::of(source("warehouse")))
            .expect("the exchange does not fail");
        assert!(matches!(minted, Minted::Granted { .. }));
    }
}
