//! `WorkloadIdentityBroker`'s own suite - a separate file for the reason `crate::wire::tests` is
//! one: `sts.rs` plus its inline tests crossed the workspace's 1000-line ceiling once the second
//! hop's own fake-port cells landed, and `AGENTS.md` asks for a split rather than trimmed prose. A
//! child module of `sts`, so every private item `sts.rs` declares is reachable through `super::`,
//! the same way `sts::cache`'s own tests reach it.

use sutura_domain::identity::{
    Agreed, CredentialBroker as _, CredentialsDoNotFitTheRequest, Expiry, Minted, Presented, PrincipalChain, RequestContext,
    Secret, SourceSet, Subject, SubjectId,
};
use sutura_domain::model::SourceName;
use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared};

use super::{
    Duration, ExchangeUnusable, ImpersonateAsAccount, NoImpersonation, NonZeroU64, StsCredential, StsExchange, UnixClock,
    WorkloadIdentity, WorkloadIdentityBroker,
};

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
) -> WorkloadIdentityBroker<FakeExchange, NoImpersonation, C>
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

// --------------------------------------------------------- telekom/sutura#376's second hop ---

/// A refusal a test plants, redacted the same way `wire::iamcredentials::RedactedSa` is - so
/// `a_403_at_the_hop_is_a_named_refusal_not_the_free_text_body` can prove the broker's own
/// `Display`/`Debug` never carry it, rather than merely not choosing to print it.
struct RedactedPlant(
    #[expect(
        dead_code,
        reason = "held only so the redaction can be proven against a real value, never read by its own Debug"
    )]
    &'static str,
);

impl core::fmt::Debug for RedactedPlant {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "<a planted reason, redacted>")
    }
}

#[derive(Debug, thiserror::Error)]
#[error("the pool subject may not impersonate the declared account")]
struct FakeImpersonationRefused {
    reason: RedactedPlant,
}

/// (federated bearer, target SA), one entry per [`FakeImpersonation::impersonate`] call. Named
/// for `clippy::type_complexity`'s own threshold, the same reason `Calls` is in `sts/cache.rs`'s
/// own test module.
type HopCalls = std::cell::RefCell<Vec<(String, String)>>;

/// A fake [`ImpersonateAsAccount`] port - the second hop's own `FakeExchange`. Grants a
/// distinguishable token, or refuses, and always records what it was asked with: the federated
/// bearer it received and the target SA it was asked to impersonate.
struct FakeImpersonation {
    grant: Option<String>,
    planted_reason: &'static str,
    calls: HopCalls,
}

impl FakeImpersonation {
    fn granting(token: &str) -> Self {
        Self {
            grant: Some(String::from(token)),
            planted_reason: "",
            calls: std::cell::RefCell::default(),
        }
    }

    fn refusing(planted_reason: &'static str) -> Self {
        Self {
            grant: None,
            planted_reason,
            calls: std::cell::RefCell::default(),
        }
    }
}

impl ImpersonateAsAccount for FakeImpersonation {
    type Error = FakeImpersonationRefused;

    #[expect(
        clippy::disallowed_methods,
        reason = "a fake hop records the federated bearer it was handed so a test can assert WHAT reached it"
    )]
    fn impersonate(
        &self,
        federated: &Secret,
        target_sa: &str,
        _scope: &str,
        _lifetime: Duration,
    ) -> Result<StsCredential, Self::Error> {
        self.calls
            .borrow_mut()
            .push((String::from(federated.expose_secret()), String::from(target_sa)));
        self.grant.as_ref().map_or_else(
            || {
                Err(FakeImpersonationRefused {
                    reason: RedactedPlant(self.planted_reason),
                })
            },
            |token| {
                Ok(StsCredential::of(
                    Secret::new(token.clone()),
                    Expiry::At {
                        unix_seconds: A_FIXED_NOW + 3_600,
                    },
                ))
            },
        )
    }
}

/// The one subject `caller()` ever builds, as the map key an impersonating source declares - so a
/// test says "this source hops for THIS caller" without hand-writing the id twice.
fn the_callers_subject() -> SubjectId {
    SubjectId::parse("someone@example.com").expect("a test subject is a subject")
}

/// One impersonating source declaring the hop for `the_callers_subject()`, with the fake exchange
/// and fake impersonation a test names. Mirrors [`impersonating_warehouse`], plus the second hop.
fn hopping_warehouse(
    exchange: FakeExchange,
    impersonation: FakeImpersonation,
    target_sa: &str,
) -> WorkloadIdentityBroker<FakeExchange, FakeImpersonation, Frozen> {
    WorkloadIdentityBroker::empty(exchange)
        .measured_against(Frozen(A_FIXED_NOW))
        .impersonating_via(impersonation)
        .impersonating(
            source("warehouse"),
            WorkloadIdentity::of(
                String::from("//iam.googleapis.com/.../providers/sso"),
                String::from("https://www.googleapis.com/auth/bigquery.readonly"),
            )
            .with_impersonation(std::collections::BTreeMap::from([(
                the_callers_subject(),
                String::from(target_sa),
            )])),
        )
}

#[test]
fn the_hop_is_called_with_the_federated_token_and_the_declared_sa() {
    let broker = hopping_warehouse(
        FakeExchange::minting_one_lasting(3_600),
        FakeImpersonation::granting("sa-token"),
        "target-sa@acme-analytics.iam.gserviceaccount.com",
    );
    let minted = broker
        .mint(&caller(Some("caller-token")), &SourceSet::of(source("warehouse")))
        .expect("the hop does not fail");
    assert!(matches!(minted, Minted::Granted { .. }));
    let WorkloadIdentityBroker { impersonation, .. } = &broker;
    let calls = impersonation.calls.borrow();
    assert_eq!(calls.len(), 1, "one leg is one hop");
    assert_eq!(
        calls[0],
        (
            String::from("exchanged-for-caller-token"),
            String::from("target-sa@acme-analytics.iam.gserviceaccount.com")
        ),
        "the hop must receive the FIRST exchange's own access token as its bearer, and the declared SA"
    );
}

#[test]
fn a_403_at_the_hop_is_a_named_refusal_not_the_free_text_body() {
    let planted = "Access Denied: alice@example.com may not act as this service account";
    let broker = hopping_warehouse(
        FakeExchange::minting_one_lasting(3_600),
        FakeImpersonation::refusing(planted),
        "target-sa@acme-analytics.iam.gserviceaccount.com",
    );
    let failure = broker
        .mint(&caller(Some("caller-token")), &SourceSet::of(source("warehouse")))
        .expect_err("a refused hop is a broker failure");
    assert!(matches!(failure, ExchangeUnusable::Provider { .. }));
    let debug = format!("{failure:?}");
    let display = format!("{failure}");
    assert!(!debug.contains(planted), "{debug}");
    assert!(!debug.contains("alice"), "{debug}");
    assert!(!display.contains(planted), "{display}");
    assert!(!display.contains("alice"), "{display}");
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "reading the minted material IS the assertion: that the SA token, not the federated one, reached the leg"
)]
fn the_resulting_bearer_is_the_sa_token_not_the_federated_one() {
    let broker = hopping_warehouse(
        FakeExchange::minting_one_lasting(3_600),
        FakeImpersonation::granting("sa-token-distinct-from-the-federated-one"),
        "target-sa@acme-analytics.iam.gserviceaccount.com",
    );
    let minted = broker
        .mint(&caller(Some("caller-token")), &SourceSet::of(source("warehouse")))
        .expect("the hop does not fail");
    let credentials = granted(minted);
    let Presented::SubjectToken { material } = credentials.presented_for(&source("warehouse")).expect("a leg") else {
        panic!("an impersonating source gets a subject token");
    };
    assert_eq!(
        material.expose_secret(),
        "sa-token-distinct-from-the-federated-one",
        "a broker that skipped the hop and reused the federated credential is the mutation this cell exists to kill"
    );
    assert_ne!(material.expose_secret(), "exchanged-for-caller-token");
}

#[test]
fn a_subject_with_no_declared_impersonate_entry_keeps_todays_bare_exchange() {
    // The map names a DIFFERENT subject - additive, per `WorkloadIdentity::target_for`'s own doc:
    // a caller absent from the map is never granted a fallback identity, and never reaches the
    // hop either.
    let impersonation = FakeImpersonation::granting("sa-token-nobody-should-receive");
    let broker = WorkloadIdentityBroker::empty(FakeExchange::minting_one_lasting(3_600))
        .measured_against(Frozen(A_FIXED_NOW))
        .impersonating_via(impersonation)
        .impersonating(
            source("warehouse"),
            WorkloadIdentity::of(
                String::from("//iam.googleapis.com/.../providers/sso"),
                String::from("https://www.googleapis.com/auth/bigquery.readonly"),
            )
            .with_impersonation(std::collections::BTreeMap::from([(
                // A DIFFERENT masked form from `caller()`'s "someone@example.com" - not merely a
                // different raw string. `SubjectId::parse` masks to first-character-plus-stars per
                // segment, so two subjects sharing a first letter and a domain would mask identically
                // and this fixture would prove nothing; "zeta" and "another" both diverge from
                // "someone" in the segment `mask_principal_into` actually keeps.
                SubjectId::parse("zeta@another.example").expect("a test subject is a subject"),
                String::from("target-sa@acme-analytics.iam.gserviceaccount.com"),
            )])),
        );
    let minted = broker
        .mint(&caller(Some("caller-token")), &SourceSet::of(source("warehouse")))
        .expect("a bare exchange does not fail");
    let credentials = granted(minted);
    let Presented::SubjectToken { material } = credentials.presented_for(&source("warehouse")).expect("a leg") else {
        panic!("an impersonating source gets a subject token");
    };
    #[expect(
        clippy::disallowed_methods,
        reason = "reading the minted material IS the assertion: the bare exchange's own token, untouched by the hop"
    )]
    let observed = String::from(material.expose_secret());
    assert_eq!(observed, "exchanged-for-caller-token");
    let WorkloadIdentityBroker { impersonation, .. } = &broker;
    assert!(
        impersonation.calls.borrow().is_empty(),
        "a subject absent from the map must never reach the hop"
    );
}

#[test]
fn a_declared_target_with_no_port_wired_is_a_typed_refusal_not_a_panic() {
    // `NoImpersonation` (the default, never wired here) - a composition defect this broker
    // reports rather than crashes on: a caller whose subject resolves a target gets a named
    // `ExchangeUnusable::Provider` naming `ImpersonationNotWired`, never a panic.
    let broker = WorkloadIdentityBroker::empty(FakeExchange::minting_one_lasting(3_600))
        .measured_against(Frozen(A_FIXED_NOW))
        .impersonating(
            source("warehouse"),
            WorkloadIdentity::of(
                String::from("//iam.googleapis.com/.../providers/sso"),
                String::from("https://www.googleapis.com/auth/bigquery.readonly"),
            )
            .with_impersonation(std::collections::BTreeMap::from([(
                the_callers_subject(),
                String::from("target-sa@acme-analytics.iam.gserviceaccount.com"),
            )])),
        );
    let failure = broker
        .mint(&caller(Some("caller-token")), &SourceSet::of(source("warehouse")))
        .expect_err("a resolved target with no port wired is a broker failure, not a grant");
    assert!(matches!(failure, ExchangeUnusable::Provider { .. }));
}
