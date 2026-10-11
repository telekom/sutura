//! The broker's delegation hook, against a fake identity provider.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use sutura_domain::identity::{
    Agreed, CredentialBroker as _, Minted, Presented, PrincipalChain, PrincipalName, RequestContext, Secret, SourceSet, Subject,
    SubjectKey,
};
use sutura_domain::model::SourceName;

use sutura_domain::identity::{Delegated, Delegation, DelegationExchange, DelegationFailed, RequestedAudience};

use crate::{DeclaredPrincipalBroker, DeclaredPrincipals, DeclaredPrincipalsUnusable};

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
}

fn declaring(subjects: &[&str]) -> DeclaredPrincipals {
    DeclaredPrincipals::parse(
        subjects
            .iter()
            .map(|raw| {
                (
                    SubjectKey::parse(raw).expect("a test key is a key"),
                    PrincipalName::parse("bq@sutura.example.com").expect("a test principal is a principal"),
                )
            })
            .collect::<BTreeMap<_, _>>(),
    )
    .expect("a declaration naming somebody parses")
}

fn delegated_warehouse(idp: &Arc<FakeIdp>) -> DeclaredPrincipalBroker {
    DeclaredPrincipalBroker::empty().impersonating_delegated(
        source("warehouse"),
        declaring(&["analyst-a@example.com", "analyst-b@example.com"]),
        Delegation::through(Arc::<FakeIdp>::clone(idp), pool_audience()),
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
        Presented::SubjectToken { material, .. } => material.expose_secret().to_owned(),
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
    // `yard` sorts AFTER `warehouse`, so the admitted, delegated source clears its own checks first
    // and only an exchange made before the whole refusal pass ends can reach the fake.
    let (idp, asked) = fake(false);
    let broker = delegated_warehouse(&idp).impersonating_delegated(
        source("yard"),
        declaring(&["analyst-b@example.com"]),
        Delegation::through(Arc::<FakeIdp>::clone(&idp), pool_audience()),
    );
    let minted = broker
        .mint(
            &asking("analyst-a@example.com"),
            &SourceSet::of(source("warehouse")).and(source("yard")),
        )
        .expect("an undeclared caller is a refusal, not an error");
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
    DeclaredPrincipalBroker::empty().authenticating(
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
    assert_eq!(super::target_at(mint(), "anyone@example.com", &sources, &at), None);
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
    );
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
    );
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
