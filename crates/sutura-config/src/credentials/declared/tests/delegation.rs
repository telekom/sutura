//! The broker's delegation hook, against a fake identity provider.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use sutura_domain::identity::{
    Agreed, CredentialBroker as _, Minted, Presented, PrincipalChain, PrincipalName, RequestContext, Secret, SourceSet, Subject,
};
use sutura_domain::model::SourceName;

use sutura_domain::identity::{Delegated, Delegation, DelegationExchange, DelegationFailed, RequestedAudience};

use crate::{DeclaredPrincipalBroker, DeclaredPrincipalsUnusable};

/// 2096-10-02, before both expiries below, so every grant agrees.
const NOW: u64 = 4_000_000_000;
/// When a fixture caller's inbound token stops being one.
const INBOUND_EXPIRES: u64 = 4_102_444_800;
/// When the fake identity provider's exchanged token does - EARLIER than the inbound one on purpose.
const DELEGATED_EXPIRES: u64 = 4_000_000_600;

/// One exchange the fake was asked for: the subject token it was handed, and the audience.
type Asked = (String, String);

/// An identity provider that reports every exchange it was asked for and mints a token naming the
/// subject.
#[derive(Debug)]
struct FakeIdp {
    asked: Sender<Asked>,
    refuses: bool,
}

/// The fake, and the end its reports arrive at.
type Faked = (Arc<FakeIdp>, Receiver<Asked>);

fn fake(refuses: bool) -> Faked {
    let (asked, reported) = channel();
    (Arc::new(FakeIdp { asked, refuses }), reported)
}

impl DelegationExchange for FakeIdp {
    #[expect(
        clippy::disallowed_methods,
        reason = "the fake records what it was handed, which is the assertion"
    )]
    fn exchange(&self, subject: &Secret, audience: &RequestedAudience) -> Result<Delegated, DelegationFailed> {
        drop(
            self.asked
                .send((subject.expose_secret().to_owned(), audience.as_str().to_owned())),
        );
        if self.refuses {
            return Err(DelegationFailed::Refused {
                status: 400,
                error: Some(String::from("invalid_grant")),
            });
        }
        Ok(Delegated::new(
            Secret::new(format!("delegated:{}", subject.expose_secret())),
            DELEGATED_EXPIRES,
        ))
    }
}

fn source(name: &str) -> SourceName {
    SourceName::parse(name).expect("a test source name is a name")
}

fn pool_audience() -> RequestedAudience {
    RequestedAudience::parse("https://workforce-pool.example.com").expect("a test audience parses")
}

fn asking(raw: &str) -> RequestContext {
    RequestContext::with_assertion(
        PrincipalChain::of(Subject::verified(raw).expect("a test subject is a subject")),
        Secret::new(format!("inbound:{raw}")),
        INBOUND_EXPIRES,
    )
    .admitted_by(super::direct())
}

fn delegated_warehouse(idp: &Arc<FakeIdp>) -> DeclaredPrincipalBroker {
    super::broker().federating(
        source("warehouse"),
        Some(Delegation::through(Arc::<FakeIdp>::clone(idp), pool_audience())),
    )
}

/// What the broker presented at `warehouse` for `raw`, read through `agreeing_with` like an adapter.
#[expect(
    clippy::disallowed_methods,
    reason = "the cell compares the presented material, which is the claim"
)]
fn presented_to(broker: &DeclaredPrincipalBroker, raw: &str) -> String {
    let asked = SourceSet::of(source("warehouse"));
    let minted = broker.mint(&asking(raw), &asked).expect("the broker answers");
    let Agreed::Granted { credentials } = minted
        .agreeing_with(&Subject::verified(raw).expect("a subject"), &asked, NOW)
        .expect("the grant agrees with the request")
    else {
        panic!("expected a grant for {raw}");
    };
    match credentials.presented_for(&source("warehouse")).expect("asked for") {
        Presented::SubjectToken { material } => material.expose_secret().to_owned(),
        other => panic!("expected a subject token, got {other:?}"),
    }
}

#[test]
fn a_delegated_source_presents_the_exchanged_token_and_never_the_inbound_one() {
    let (idp, asked) = fake(false);
    let presented = presented_to(&delegated_warehouse(&idp), "analyst-a@example.com");
    assert_eq!(presented, "delegated:inbound:analyst-a@example.com");
    assert_eq!(
        asked.try_iter().collect::<Vec<_>>(),
        vec![(
            String::from("inbound:analyst-a@example.com"),
            String::from("https://workforce-pool.example.com")
        )],
        "the exchange must be asked with the caller's own inbound token for the requested audience"
    );
}

#[test]
fn two_subjects_never_share_an_exchanged_token() {
    let (idp, asked) = fake(false);
    let broker = delegated_warehouse(&idp);
    let a = presented_to(&broker, "analyst-a@example.com");
    let b = presented_to(&broker, "analyst-b@example.com");
    let a_again = presented_to(&broker, "analyst-a@example.com");
    assert_eq!(a, "delegated:inbound:analyst-a@example.com");
    assert_eq!(b, "delegated:inbound:analyst-b@example.com");
    assert_eq!(a_again, a);
    assert_eq!(
        asked.try_iter().count(),
        3,
        "every mint reaches the identity provider: there is no store of exchanged tokens for a second subject to hit"
    );
}

#[test]
fn the_presented_lifetime_is_the_earlier_of_the_inbound_and_the_exchanged_token() {
    let (idp, _asked) = fake(false);
    let asked = SourceSet::of(source("warehouse"));
    let minted = delegated_warehouse(&idp)
        .mint(&asking("analyst-a@example.com"), &asked)
        .expect("the broker answers");
    let subject = Subject::verified("analyst-a@example.com").expect("a subject");
    let Agreed::Granted { credentials } = minted.agreeing_with(&subject, &asked, NOW).expect("the grant agrees") else {
        panic!("expected a grant");
    };
    credentials
        .still_usable_at(DELEGATED_EXPIRES - 1)
        .expect("usable until the exchanged token's last second");
    assert!(
        credentials.still_usable_at(DELEGATED_EXPIRES).is_err(),
        "a grant outliving the exchanged token was presented as usable - the inbound token's later expiry won"
    );
}

#[test]
fn a_plan_refused_at_one_source_sends_nothing_to_the_idp() {
    // `yard` sorts AFTER `warehouse`, so the delegated source clears its own checks first and only
    // an exchange made before the whole refusal pass ends can reach the fake. `yard` switches, and
    // its deployment user IS the asker, so it refuses them.
    let (idp, asked) = fake(false);
    let broker = delegated_warehouse(&idp).switching(
        source("yard"),
        PrincipalName::parse("analyst-a@example.com").expect("a test principal is a principal"),
    );
    let minted = broker
        .mint(
            &asking("analyst-a@example.com"),
            &SourceSet::of(source("warehouse")).and(source("yard")),
        )
        .expect("a source that refuses the asker is a refusal, not an error");
    assert!(
        matches!(minted, Minted::Refused { ref source } if source.as_str() == "yard"),
        "{minted:?}"
    );
    let reached: Vec<Asked> = asked.try_iter().collect();
    assert!(
        reached.is_empty(),
        "a refused plan reached the identity provider: {reached:?}"
    );
}

#[test]
fn a_failed_exchange_is_an_error_and_names_no_token() {
    let (idp, _asked) = fake(true);
    let failed = delegated_warehouse(&idp)
        .mint(&asking("analyst-a@example.com"), &SourceSet::of(source("warehouse")))
        .expect_err("an exchange that failed presents nothing");
    assert!(
        matches!(failed, DeclaredPrincipalsUnusable::Delegation { ref source, .. } if source.as_str() == "warehouse"),
        "{failed:?}"
    );
    let chain = format!("{failed} / {failed:?}");
    assert!(!chain.contains("inbound:"), "the error leaked the caller's token: {chain}");
}

/// One authenticating source whose callers' assertions are exchanged at `idp`.
fn authenticating_warehouse(idp: &Arc<FakeIdp>) -> DeclaredPrincipalBroker {
    super::broker().authenticating(
        source("warehouse"),
        Delegation::through(Arc::<FakeIdp>::clone(idp), pool_audience()),
    )
}

/// An authenticating source lists no caller: any verified asker is presented the token their own
/// assertion is exchanged for, with no principal beside it.
#[test]
fn an_authenticating_source_presents_the_exchanged_token_and_no_principal() {
    let (idp, asked) = fake(false);
    let broker = authenticating_warehouse(&idp);
    let at = source("warehouse");
    let sources = SourceSet::of(at.clone());
    let mint = || {
        broker
            .mint(&asking("anyone@example.com"), &sources)
            .expect("the broker answers")
    };
    assert_eq!(
        super::presented_at(mint(), "anyone@example.com", &sources, &at),
        "delegated:inbound:anyone@example.com"
    );
    drop(mint());
    let expected = (
        String::from("inbound:anyone@example.com"),
        String::from("https://workforce-pool.example.com"),
    );
    assert_eq!(
        asked.try_iter().collect::<Vec<_>>(),
        vec![expected.clone(), expected],
        "each mint exchanges the caller's own inbound token for the declared audience"
    );
}

/// A request with no verified caller is refused even when it carries an assertion, and so is a
/// verified caller with no assertion - and neither reaches the identity provider.
#[test]
fn an_authenticating_source_refuses_an_anonymous_or_assertionless_caller_and_asks_no_idp() {
    let (idp, asked) = fake(false);
    let anonymous = RequestContext::with_assertion(
        PrincipalChain::of(Subject::TheDeploymentItself),
        Secret::new("inbound:nobody"),
        INBOUND_EXPIRES,
    )
    .admitted_by(super::direct());
    for context in [anonymous, super::no_caller(), super::no_assertion("analyst-a@example.com")] {
        let refused = authenticating_warehouse(&idp)
            .mint(&context, &SourceSet::of(source("warehouse")))
            .expect("a refusal, not an error");
        assert!(
            matches!(refused, Minted::Refused { ref source } if source.as_str() == "warehouse"),
            "{refused:?}"
        );
    }
    let reached: Vec<Asked> = asked.try_iter().collect();
    assert!(
        reached.is_empty(),
        "a refused request reached the identity provider: {reached:?}"
    );
}

/// An authenticating source's leg lapses at the earlier of the asker's assertion and the token it
/// was exchanged for, whichever of the two that is.
#[test]
fn an_authenticating_sources_leg_lapses_with_the_earlier_of_the_assertion_and_the_exchanged_token() {
    const ASSERTION_FIRST: u64 = NOW + 60;
    let (idp, _asked) = fake(false);
    let broker = authenticating_warehouse(&idp);
    let sources = SourceSet::of(source("warehouse"));
    let short_lived = RequestContext::with_assertion(
        PrincipalChain::of(Subject::verified("analyst-a@example.com").expect("a test subject is a subject")),
        Secret::new("inbound:analyst-a@example.com"),
        ASSERTION_FIRST,
    )
    .admitted_by(super::direct());
    for (context, lapses) in [
        (asking("analyst-a@example.com"), DELEGATED_EXPIRES),
        (short_lived, ASSERTION_FIRST),
    ] {
        let minted = broker.mint(&context, &sources).expect("the broker answers");
        let credentials = super::granted(minted, "analyst-a@example.com", &sources);
        credentials
            .still_usable_at(lapses - 1)
            .expect("usable until the earlier lifetime's last second");
        assert!(
            credentials.still_usable_at(lapses).is_err(),
            "a leg outlived the earlier of its two lifetimes, which ends at {lapses}"
        );
    }
}

/// A signing-in source presents the caller's verified name beside the token their assertion is
/// exchanged for, never the inbound one, and refuses the deployment's own user before any exchange.
#[test]
fn a_signing_in_source_presents_the_verified_name_and_the_exchanged_token() {
    let (idp, asked) = fake(false);
    let broker = super::broker().signing_in(
        source("warehouse"),
        PrincipalName::parse("sutura").expect("a test principal is a principal"),
        Delegation::through(Arc::<FakeIdp>::clone(&idp), pool_audience()),
    );
    let at = source("warehouse");
    let sources = SourceSet::of(at.clone());
    let mint = |raw: &str| broker.mint(&asking(raw), &sources).expect("the broker answers");
    let analyst = "analyst-a@example.com";
    #[expect(
        clippy::disallowed_methods,
        reason = "the cell compares the presented material, which is the claim"
    )]
    let signed_in = match super::granted(mint(analyst), analyst, &sources)
        .presented_for(&at)
        .expect("asked for")
    {
        Presented::SubjectSignIn { name, material } => (name.as_str().to_owned(), material.expose_secret().to_owned()),
        other => panic!("expected a sign-in, got {other:?}"),
    };
    assert_eq!(
        signed_in,
        (analyst.to_owned(), String::from("delegated:inbound:analyst-a@example.com"))
    );
    assert_eq!(
        asked.try_iter().count(),
        1,
        "the mint exchanges the caller's own inbound token"
    );
    assert!(
        matches!(mint("sutura"), Minted::Refused { .. }),
        "the deployment's own user is refused"
    );
    assert_eq!(asked.try_iter().count(), 0, "a refused caller reaches no identity provider");
}

/// A signing-in source refuses a verified caller with no assertion: there is no token to exchange,
/// so the caller's name alone never signs in.
#[test]
fn a_signing_in_source_refuses_a_caller_with_no_assertion_and_asks_no_idp() {
    let (idp, asked) = fake(false);
    let broker = super::broker().signing_in(
        source("warehouse"),
        PrincipalName::parse("sutura").expect("a test principal is a principal"),
        Delegation::through(Arc::<FakeIdp>::clone(&idp), pool_audience()),
    );
    let refused = broker
        .mint(
            &super::no_assertion("analyst-a@example.com"),
            &SourceSet::of(source("warehouse")),
        )
        .expect("a refusal, not an error");
    assert!(
        matches!(refused, Minted::Refused { ref source } if source.as_str() == "warehouse"),
        "{refused:?}"
    );
    assert_eq!(asked.try_iter().count(), 0, "a refused caller reaches no identity provider");
}

/// An address outside the addressing source's domains is refused by its own name, and the delegated
/// source asked beside it - sorted first - sends nothing to the identity provider.
#[test]
fn an_address_outside_the_domains_is_refused_before_any_exchange() {
    let (idp, asked) = fake(false);
    let domains = crate::sources::address_domains::AddressDomains::parse(&[String::from("example.com")])
        .expect("a test domain is a domain");
    let broker = delegated_warehouse(&idp).addressing(source("yard"), Some(domains));
    let plan = SourceSet::of(source("warehouse")).and(source("yard"));
    let address = |raw| sutura_domain::identity::VerifiedAddress::parse(raw).expect("a test address is an address");
    let outside = broker
        .mint(&asking("analyst-a").addressed(Some(address("a@evil-example.com"))), &plan)
        .expect("a refusal, not an error");
    assert!(
        matches!(outside, Minted::AddressOutsideDomains { ref source } if source.as_str() == "yard"),
        "{outside:?}"
    );
    assert_eq!(asked.try_iter().count(), 0, "a refused request reached the identity provider");
    let inside = broker
        .mint(&asking("analyst-a").addressed(Some(address("a@Example.com"))), &plan)
        .expect("an answer");
    assert!(matches!(inside, Minted::Granted { .. }), "{inside:?}");
}

// -------------------------------------------------- gateway-admitted callers (#1884) ----

/// A caller the multi-verifier gate admitted via a non-challenging (behind-gateway) verifier.
fn gateway_caller(raw: &str) -> RequestContext {
    asking(raw).admitted_by(sutura_domain::identity::AdmittingVerifier::of(
        sutura_domain::identity::VerifierId::primary(),
        String::from("https://gateway.example.com"),
        sutura_domain::identity::VerifierMode::Gateway,
    ))
}

/// A gateway-admitted caller is refused at every exchange-touching source without reaching the
/// identity provider.
///
/// **Finding 2 of PR #1884's review**: the gateway already performed the credential exchange; a
/// second exchange inside this deployment would re-negotiate something the gateway already decided.
/// The refusal is `Minted::Refused` - the same shape as a declined source - so callers see a
/// permission boundary rather than an error.
///
/// **The exchange port is proven unreached**: if `Refused` is returned, the code path that calls
/// `idp.exchange()` was bypassed, and the fake's report channel stays empty.
#[test]
fn a_gateway_admitted_caller_is_refused_at_a_federating_source_and_the_idp_is_not_called() {
    let (idp, asked) = fake(false);
    let broker = delegated_warehouse(&idp);
    let caller = gateway_caller("analyst-a@example.com");
    let sources = SourceSet::of(source("warehouse"));
    let minted = broker.mint(&caller, &sources).expect("a refusal, not an error");
    assert!(
        matches!(minted, Minted::Refused { ref source } if source.as_str() == "warehouse"),
        "a gateway-admitted caller must be refused at a federating source: {minted:?}"
    );
    let reached: Vec<Asked> = asked.try_iter().collect();
    assert!(
        reached.is_empty(),
        "the identity provider was called for a gateway-admitted caller: {reached:?}"
    );
}

#[test]
fn a_gateway_admitted_caller_is_refused_at_an_authenticating_source_and_the_idp_is_not_called() {
    let (idp, asked) = fake(false);
    let broker = authenticating_warehouse(&idp);
    let caller = gateway_caller("analyst-a@example.com");
    let sources = SourceSet::of(source("warehouse"));
    let minted = broker.mint(&caller, &sources).expect("a refusal, not an error");
    assert!(
        matches!(minted, Minted::Refused { ref source } if source.as_str() == "warehouse"),
        "a gateway-admitted caller must be refused at an authenticating source: {minted:?}"
    );
    let reached: Vec<Asked> = asked.try_iter().collect();
    assert!(
        reached.is_empty(),
        "the identity provider was called for a gateway-admitted caller: {reached:?}"
    );
}

#[test]
fn a_later_source_denied_by_verifier_preflight_sends_nothing_to_the_idp_for_any_credential_shape() {
    use crate::credentials::SourceAdmissions;
    use sutura_domain::identity::{VerifiedAddress, VerifierId};

    for shape in ["shared", "address", "name"] {
        let (idp, reached) = fake(false);
        let primary = Some(std::iter::once(VerifierId::primary()).collect());
        let denied = SourceAdmissions::default().and(source("warehouse"), primary.clone()).and(
            source("yard"),
            Some(std::iter::once(VerifierId::parse("other").expect("an id")).collect()),
        );
        let broker = delegated_warehouse(&idp);
        let broker = match shape {
            "shared" => broker.shared(source("yard"), super::declared_shared()),
            "address" => broker.addressing(source("yard"), None),
            _ => broker.switching(source("yard"), PrincipalName::parse("deployment").expect("a principal")),
        };
        let context = asking("user@example.com").addressed(Some(VerifiedAddress::parse("user@example.com").expect("an address")));
        let refused = broker
            .clone()
            .admitting(denied)
            .mint(&context, &SourceSet::of(source("warehouse")).and(source("yard")))
            .expect("a refusal is an answer");
        assert!(
            matches!(refused, Minted::Refused { ref source } if source.as_str() == "yard"),
            "{shape}: {refused:?}"
        );
        assert!(reached.try_iter().next().is_none(), "a denied {shape} reached the IdP");

        let admitted = SourceAdmissions::default()
            .and(source("warehouse"), primary.clone())
            .and(source("yard"), primary);
        let granted = broker
            .admitting(admitted)
            .mint(&context, &SourceSet::of(source("warehouse")).and(source("yard")))
            .expect("both sources admit this verifier");
        assert!(matches!(granted, Minted::Granted { .. }), "{shape}: {granted:?}");
        assert_eq!(reached.try_iter().count(), 1, "the admitted plan reaches its exchange");
    }
}

#[test]
fn gateway_callers_reach_no_hopless_name_or_address_credential() {
    let direct = asking("user@example.com").addressed(Some(
        sutura_domain::identity::VerifiedAddress::parse("user@example.com").expect("an address"),
    ));
    let gateway = gateway_caller("user@example.com").addressed(Some(
        sutura_domain::identity::VerifiedAddress::parse("user@example.com").expect("an address"),
    ));
    let sources = SourceSet::of(source("warehouse"));
    for broker in [
        super::broker().switching(source("warehouse"), PrincipalName::parse("deployment").expect("a principal")),
        super::broker().addressing(source("warehouse"), None),
    ] {
        assert!(matches!(
            broker.mint(&direct, &sources).expect("the broker answers"),
            Minted::Granted { .. }
        ));
        assert!(
            matches!(broker.mint(&gateway, &sources).expect("the broker answers"), Minted::Refused { source: refused } if refused == source("warehouse"))
        );
    }
}
