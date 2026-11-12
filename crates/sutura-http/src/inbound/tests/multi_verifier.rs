//! Signed admission, bounded routing and the credential boundary across verifiers.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use sutura_config::{DeclaredPrincipalBroker, InboundVerifier};
use sutura_dev::issuer::{MockIssuer, Token};
use sutura_domain::identity::{
    CredentialBroker as _, Delegated, Delegation, DelegationExchange, DelegationFailed, Minted, PrincipalName, RequestedAudience,
    Secret, SourceSet, Subject, VerifierMode,
};

use super::fixtures::{bearer, gate_over_multi, transit};
use crate::inbound::{InboundGate, TokenRejected};
use crate::testing::{an_issuer, named_direct_entry, settings_with, source};

fn gateway(issuer: &str, audience: &str) -> MockIssuer {
    MockIssuer::generating(issuer, audience, "gateway-key").expect("a fixture issuer generates a key")
}

fn secondary(id: usize, issuer: &MockIssuer, header: &str, limit: usize) -> String {
    format!(
        "    - id: gateway{id}\n      path: /mcp\n      mode: behind-gateway\n      transit_header: '{header}'\n      transit_issuer: '{}'\n      \
         transit_audience: '{}'\n      key_set_file: /unread.json\n      algorithms: [ES256]\n      \
         transit_token_type: at+jwt\n      max_token_bytes: {limit}\n      granted_scopes: ['sutura:metrics.ask']\n",
        issuer.issuer(),
        issuer.audience(),
    )
}

type Secondary<'issuer> = (&'issuer MockIssuer, &'issuer str, usize);

fn gate(primary: &MockIssuer, limit: usize, others: &[Secondary<'_>]) -> InboundGate {
    let mut overlay = format!(
        "security:\n  inbound:\n{}      max_token_bytes: {limit}\n",
        named_direct_entry(primary, "primary", "/mcp", "/unread.json")
    );
    for (id, (issuer, header, bound)) in others.iter().enumerate() {
        overlay.push_str(&secondary(id, issuer, header, *bound));
    }
    let settings = settings_with(&overlay);
    let documents: Vec<String> = others.iter().map(|(issuer, _, _)| issuer.key_set()).collect();
    let secondaries: Vec<(&InboundVerifier, &str)> = settings
        .security()
        .inbound()
        .iter()
        .skip(1)
        .zip(&documents)
        .map(|(declaration, document)| (declaration, document.as_str()))
        .collect();
    gate_over_multi(
        settings.security().inbound().first().expect("a primary verifier"),
        &primary.key_set(),
        &secondaries,
    )
}

fn signed(issuer: &MockIssuer, subject: &str, scope: &str) -> String {
    issuer
        .mint(&Token::for_subject(subject).granting(scope).living_for(60))
        .expect("a fixture signs")
}

#[tokio::test]
async fn each_signed_verifier_admits_its_subject_and_scopes_and_refuses_swapped_audiences() {
    let direct = an_issuer();
    let gateway = gateway("https://gateway.example.com", "/transit");
    let gate = gate(&direct, 8192, &[(&gateway, "X-Transit-Proof", 8192)]);
    for (issuer, subject, scope, mode) in [
        (&direct, "person@example.com", "sutura:catalog.read", VerifierMode::Direct),
        (&gateway, "application", "sutura:query", VerifierMode::Gateway),
    ] {
        let token = signed(issuer, subject, scope);
        let headers = match mode {
            VerifierMode::Direct => bearer(&token),
            VerifierMode::Gateway => transit(&token),
        };
        let caller = gate
            .establish(&headers, Instant::now())
            .await
            .expect("the signed caller is admitted");
        assert_eq!(
            caller.chain().subject(),
            &Subject::verified_by(caller.verifier().id().clone(), subject).expect("a fixture subject")
        );
        assert_eq!(caller.scopes().iter().collect::<Vec<_>>(), vec![scope]);
        assert_eq!(caller.verifier().issuer(), issuer.issuer());
        assert_eq!(caller.verifier().mode(), mode);
        let context = crate::principal::of_verified(&caller);
        assert_eq!(context.verifier(), Some(caller.verifier()));
        let other_audience = if mode == VerifierMode::Direct {
            gateway.audience()
        } else {
            direct.audience()
        };
        let swapped = issuer
            .mint(
                &Token::for_subject(subject)
                    .granting(scope)
                    .living_for(60)
                    .for_audience(other_audience),
            )
            .expect("the fixture signs a wrong-audience token");
        let headers = match mode {
            VerifierMode::Direct => bearer(&swapped),
            VerifierMode::Gateway => transit(&swapped),
        };
        assert!(
            gate.establish(&headers, Instant::now()).await.is_err(),
            "a swapped audience admitted"
        );
    }
    let unknown = direct
        .mint(&Token::for_subject("person@example.com").claiming_issuer("https://unknown.example.com"))
        .expect("the fixture signs an unknown issuer");
    assert!(matches!(
        gate.establish(&bearer(&unknown), Instant::now()).await,
        Err(TokenRejected::UnknownIssuer)
    ));
}

#[tokio::test]
async fn selected_verifiers_keep_configured_grants_separate() {
    let direct = an_issuer();
    let gateway = gateway("https://gateway.example.com", "/transit");
    for location in ["Authorization", "X-Transit-Proof"] {
        let gate = gate(&direct, 8192, &[(&gateway, location, 8192)]);
        for (issuer, declared_scope, expected) in [
            (&direct, None, Vec::new()),
            (&gateway, None, vec!["sutura:metrics.ask"]),
            (&gateway, Some("sutura:catalog.read"), vec!["sutura:catalog.read"]),
            (&gateway, Some(""), Vec::new()),
        ] {
            let token = Token::for_subject("application").living_for(60);
            let token = match declared_scope {
                None => token,
                Some(scope) => token.granting(scope),
            };
            let token = issuer.mint(&token).expect("the fixture signs");
            let headers = if issuer.issuer() == direct.issuer() || location == "Authorization" {
                bearer(&token)
            } else {
                transit(&token)
            };
            let caller = gate.establish(&headers, Instant::now()).await.expect("signed admission");
            assert_eq!(caller.scopes().iter().collect::<Vec<_>>(), expected);
        }
    }
}

#[tokio::test]
async fn one_location_routes_each_issuer_and_competing_locations_are_refused() {
    let direct = an_issuer();
    let first = gateway("https://gateway.example.com", "/transit");
    let second = gateway("https://second.example.com", "/second");
    for location in ["Authorization", "X-Transit-Proof"] {
        let gate = gate(&direct, 8192, &[(&first, location, 8192), (&second, location, 8192)]);
        for issuer in [&first, &second] {
            let token = signed(issuer, "application", "sutura:query");
            let headers = if location == "Authorization" {
                bearer(&token)
            } else {
                transit(&token)
            };
            let caller = gate
                .establish(&headers, Instant::now())
                .await
                .expect("one shared location admits either issuer");
            assert_eq!(caller.verifier().issuer(), issuer.issuer());
        }
    }
    let gate = gate(&direct, 8192, &[(&first, "X-Transit-Proof", 8192)]);
    let direct_token = signed(&direct, "person@example.com", "sutura:query");
    let gateway_token = signed(&first, "application", "sutura:query");
    let mut competing = bearer(&direct_token);
    competing.extend(transit(&gateway_token));
    assert!(matches!(
        gate.establish(&competing, Instant::now()).await,
        Err(TokenRejected::AmbiguousProof)
    ));
    assert!(matches!(
        gate.establish(&bearer(&gateway_token), Instant::now()).await,
        Err(TokenRejected::UnknownIssuer)
    ));
}

#[tokio::test]
async fn every_location_is_bounded_before_issuer_decoding_and_the_selected_limit_still_applies() {
    let direct = an_issuer();
    let gateway = gateway("https://gateway.example.com", "/transit");
    for location in ["Authorization", "X-Transit-Proof"] {
        let gate = gate(&direct, 1024, &[(&gateway, location, 2048)]);
        let malformed = "x".repeat(2049);
        let headers = if location == "Authorization" {
            bearer(&malformed)
        } else {
            transit(&malformed)
        };
        assert!(matches!(
            gate.establish(&headers, Instant::now()).await,
            Err(TokenRejected::TooLong {
                found: 2049,
                limit: 2048
            })
        ));
    }
    let gate = gate(&direct, 1024, &[(&gateway, "Authorization", 4096)]);
    let token = direct
        .mint(&Token::for_subject("person@example.com").claiming("padding", serde_json::json!("p".repeat(1100))))
        .expect("the fixture signs a token between the configured bounds");
    assert!(token.len() > 1024 && token.len() < 4096);
    assert!(matches!(
        gate.establish(&bearer(&token), Instant::now()).await,
        Err(TokenRejected::TooLong { limit: 1024, .. })
    ));
    let (_, payload_and_signature) = token.split_once('.').expect("a signed token has a header");
    let unreadable_header = format!("!.{payload_and_signature}");
    assert!(unreadable_header.len() > 1024 && unreadable_header.len() < 4096);
    assert!(matches!(
        gate.establish(&bearer(&unreadable_header), Instant::now()).await,
        Err(TokenRejected::TooLong { limit: 1024, .. })
    ));
    let wider = gateway
        .mint(
            &Token::for_subject("application")
                .living_for(60)
                .claiming("padding", serde_json::json!("p".repeat(1100))),
        )
        .expect("the wider verifier signs");
    assert!(
        gate.establish(&bearer(&wider), Instant::now()).await.is_ok(),
        "routing must not apply the primary's smaller bound to the other issuer"
    );
}

#[derive(Debug, Default)]
struct Exchange(AtomicUsize);

impl DelegationExchange for Exchange {
    fn exchange(&self, _subject: &Secret, _audience: &RequestedAudience) -> Result<Delegated, DelegationFailed> {
        let _previous = self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Delegated::new(Secret::new("exchanged"), 4_102_444_800))
    }
}

fn admitted_broker() -> DeclaredPrincipalBroker {
    use sutura_config::credentials::SourceAdmissions;
    use sutura_domain::identity::VerifierId;
    DeclaredPrincipalBroker::empty().admitting(SourceAdmissions::default().and(
        source(),
        Some(std::collections::BTreeSet::from([
            VerifierId::primary(),
            VerifierId::parse("gateway0").expect("a fixture verifier"),
        ])),
    ))
}

#[tokio::test]
async fn signed_gateway_admission_reaches_no_exchange_and_direct_admission_does() {
    let direct = an_issuer();
    let gateway = gateway("https://gateway.example.com", "/transit");
    for location in ["Authorization", "X-Transit-Proof"] {
        let gate = gate(&direct, 8192, &[(&gateway, location, 8192)]);
        let exchange = Arc::new(Exchange::default());
        let delegation = || {
            Delegation::through(
                Arc::<Exchange>::clone(&exchange),
                RequestedAudience::parse("https://pool.example.com").expect("a fixture audience"),
            )
        };
        let deployment = PrincipalName::parse("deployment").expect("a fixture principal");
        for broker in [
            admitted_broker().federating(source(), Some(delegation())),
            admitted_broker().authenticating(source(), delegation()),
            admitted_broker().signing_in(source(), deployment.clone(), delegation()),
        ] {
            let gateway_token = signed(&gateway, "application", "sutura:query");
            let headers = if location == "Authorization" {
                bearer(&gateway_token)
            } else {
                transit(&gateway_token)
            };
            let admitted = gate.establish(&headers, Instant::now()).await.expect("the gateway admits");
            let context = crate::principal::of_verified(&admitted);
            assert!(matches!(
                broker.mint(&context, &SourceSet::of(source())).expect("the broker answers"),
                Minted::Refused { .. }
            ));
            assert_eq!(
                exchange.0.load(Ordering::SeqCst),
                0,
                "gateway admission reached the exchange port"
            );
            let token = signed(&direct, "person@example.com", "sutura:query");
            let admitted = gate
                .establish(&bearer(&token), Instant::now())
                .await
                .expect("direct admission");
            let context = crate::principal::of_verified(&admitted);
            assert!(matches!(
                broker.mint(&context, &SourceSet::of(source())).expect("the broker answers"),
                Minted::Granted { .. }
            ));
            assert_eq!(
                exchange.0.swap(0, Ordering::SeqCst),
                1,
                "direct admission must reach the same port"
            );
        }
    }
}

type RecordedVerifier = Option<(String, VerifierMode)>;

#[derive(Clone)]
struct Audit(std::sync::mpsc::Sender<RecordedVerifier>);

impl sutura_domain::audit::AuditSink for Audit {
    fn record(&self, record: &sutura_domain::audit::CallRecord<'_>) {
        self.0
            .send(
                record
                    .verifier()
                    .map(|verifier| (String::from(verifier.issuer()), verifier.mode())),
            )
            .expect("the collector is listening");
    }
}

#[tokio::test]
async fn a_shared_answer_and_an_exchange_refusal_record_the_signed_admitting_verifier() {
    use crate::testing::{a_question, broker, bundle, catalog_of, deadline, fake_warehouse};
    use sutura_app::surface::{LocalService, Surface as _};
    use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared};

    let direct = an_issuer();
    let gateway = gateway("https://gateway.example.com", "/transit");
    let gate = gate(&direct, 8192, &[(&gateway, "X-Transit-Proof", 8192)]);
    let (sent, received) = std::sync::mpsc::channel();
    let shared = LocalService::start(
        &catalog_of(bundle()),
        fake_warehouse(),
        Audit(sent.clone()),
        broker(),
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("the shared service starts");
    let exchange = Arc::new(Exchange::default());
    let delegated = admitted_broker().federating(
        source(),
        Some(Delegation::through(
            Arc::<Exchange>::clone(&exchange),
            RequestedAudience::parse("https://pool.example.com").expect("a fixture audience"),
        )),
    );
    let refusing = LocalService::start(
        &catalog_of(bundle()),
        fake_warehouse(),
        Audit(sent),
        delegated,
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("the refusing service starts");
    for (issuer, mode) in [(&direct, VerifierMode::Direct), (&gateway, VerifierMode::Gateway)] {
        let token = signed(issuer, "same-subject", "sutura:query");
        let headers = match mode {
            VerifierMode::Direct => bearer(&token),
            VerifierMode::Gateway => transit(&token),
        };
        let admitted = gate.establish(&headers, Instant::now()).await.expect("signed admission");
        let context = crate::principal::of_verified(&admitted);
        let outcome = shared
            .answer(&context, &a_question(), deadline())
            .expect("the shared source answers");
        assert!(matches!(outcome, sutura_domain::query::ToolOutcome::Answer { .. }));
        assert_eq!(
            received.try_recv().expect("an answer is audited"),
            Some((String::from(issuer.issuer()), mode))
        );
        if mode == VerifierMode::Gateway {
            let outcome = refusing
                .answer(&context, &a_question(), deadline())
                .expect("the broker refuses");
            assert!(matches!(outcome, sutura_domain::query::ToolOutcome::Refusal { .. }));
            assert_eq!(exchange.0.load(Ordering::SeqCst), 0);
            assert_eq!(
                received.try_recv().expect("the refusal is audited"),
                Some((String::from(issuer.issuer()), mode))
            );
        }
        let shared_broker = admitted_broker().shared(
            source(),
            SharedIdentityDeclared::of(
                AcknowledgementReason::parse("a fixture source whose rows every caller may read")
                    .expect("a fixture acknowledgement"),
            ),
        );
        assert!(matches!(
            shared_broker
                .mint(&context, &SourceSet::of(source()))
                .expect("shared admission"),
            Minted::Granted { .. }
        ));
    }
}

#[tokio::test]
async fn signed_raw_outcomes_and_remote_refusals_record_the_admitting_verifier() {
    use crate::testing::{broker, bundle, catalog_of, deadline, fake_warehouse};
    use sutura_app::surface::{LocalService, Surface as _};
    use sutura_domain::plan::remote::RemoteLegOutcome;
    use sutura_domain::query::RefusalReason;
    use sutura_domain::raw::RawStatement;

    let direct = an_issuer();
    let gateway = gateway("https://gateway.example.com", "/transit");
    let (sent, received) = std::sync::mpsc::channel();
    let shared = LocalService::start(
        &catalog_of(bundle()),
        fake_warehouse(),
        Audit(sent),
        broker(),
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("the shared service starts");
    for location in ["Authorization", "X-Transit-Proof"] {
        let gate = gate(&direct, 8192, &[(&gateway, location, 8192)]);
        for (issuer, mode) in [(&direct, VerifierMode::Direct), (&gateway, VerifierMode::Gateway)] {
            let token = signed(issuer, "same-subject", "sutura:query");
            let headers = if mode == VerifierMode::Direct || location == "Authorization" {
                bearer(&token)
            } else {
                transit(&token)
            };
            let admitted = gate.establish(&headers, Instant::now()).await.expect("signed admission");
            let context = crate::principal::of_verified(&admitted);
            for (sql, refused) in [("select 1", false), ("refuse me", true)] {
                let statement = RawStatement::parse(sql).expect("a fixture statement");
                let outcome = shared
                    .run_sql(&context, &statement, deadline())
                    .expect("the raw call returns an outcome");
                assert_eq!(outcome.refusal().is_some(), refused);
                assert_eq!(
                    received.try_recv().expect("the raw outcome is audited"),
                    Some((String::from(issuer.issuer()), mode))
                );
            }
            let leg = serde_json::from_value(serde_json::json!({
                "leg": "lookup", "source": source(),
                "metric": {"metric": "revenue", "digest": "a".repeat(64)},
                "table": "orders", "joins": [], "keys": [], "filters": [],
            }))
            .expect("a well-formed lookup leg");
            assert!(matches!(
                shared
                    .run_remote_leg(&context, leg, Vec::new(), deadline())
                    .expect("a remote refusal is an outcome"),
                RemoteLegOutcome::Refusal {
                    reason: RefusalReason::RemoteLegNotCertified { .. }
                }
            ));
            assert_eq!(
                received.try_recv().expect("the shared source's remote refusal is audited"),
                Some((String::from(issuer.issuer()), mode))
            );
        }
    }
}
