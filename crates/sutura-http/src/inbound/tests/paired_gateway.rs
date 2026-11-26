//! A `direct` verifier paired with `requires_gateway` - `github.com/telekom/sutura#2089` - and the
//! one explicit exception it carves out of refusing two configured credential locations at once
//! (`github.com/telekom/sutura#1884`).
//!
//! Every cell below builds the gate through [`InboundGate::with_required_gateway`], the same
//! test-only seam [`InboundGate::with_also_accept`] already uses for `also_accept`: production
//! attaches the pairing in [`InboundGate::from_verifiers_with_outbound`], which reads the gateway's
//! key set from the filesystem - these tests hand it a cache built from an in-memory document
//! instead, same as every other cell in this module.

use std::time::Instant;

use sutura_dev::issuer::{MockIssuer, Token};
use sutura_domain::identity::Subject;

use super::fixtures::{gate_over, header_named, paired, paired_direct};
use crate::inbound::{InboundGate, TokenRejected};
use crate::testing::{ISSUER, RESOURCE};

const TOKEN_HEADER: &str = "x-user-token";

fn gateway(issuer: &str, audience: &str) -> MockIssuer {
    MockIssuer::generating(issuer, audience, "gateway-key").expect("a fixture issuer generates a key")
}

fn signed_for(issuer: &MockIssuer, subject: &str) -> String {
    issuer
        .mint(&Token::for_subject(subject).living_for(60))
        .expect("a fixture signs")
}

/// A `direct` gate paired with a standalone `behind-gateway` sub-gate, over fresh issuers each call
/// so no cell can depend on another's key.
fn paired_gate() -> (InboundGate, MockIssuer, MockIssuer) {
    let caller_issuer = MockIssuer::generating(ISSUER, RESOURCE, "caller-key").expect("a fixture issuer generates a key");
    let gateway_issuer = gateway("https://gateway.example.com", "https://gateway.example.com/transit");
    let caller_identity = paired_direct("/unread/caller.json", TOKEN_HEADER);
    // The target `behind-gateway` entry must read its assertion from `Authorization` - the one
    // `NotFitToServe::RequiresGatewayNotInAuthorization` enforces at startup - never from its own
    // `transit_header`, which is what every other `behind-gateway` test in this module uses instead.
    let gateway_identity = sutura_config::InboundIdentity::BehindGateway {
        transit: sutura_config::TransitProof::new(
            sutura_config::ProofHeader::parse("authorization").expect("a fixture header is a header"),
            sutura_config::IssuerUrl::parse_transit_issuer(gateway_issuer.issuer()).expect("a fixture issuer is an issuer"),
            sutura_config::ResourceIdentifier::parse_transit_audience(gateway_issuer.audience()).expect("a fixture audience"),
            sutura_config::KeySetLocation::File(sutura_config::KeySetFile::parse("/unread/gateway.json").expect("a test path")),
            sutura_config::PinnedAlgorithms::of(sutura_config::SigningAlgorithm::Es256),
            sutura_config::RequiredTokenType::Any,
            sutura_config::ProofLifetime::parse(3600).expect("a fixture lifetime is a lifetime"),
        ),
    };
    let gate = gate_over(&caller_identity, caller_issuer.key_set().as_str())
        .with_required_gateway(gate_over_gateway(&gateway_identity, &gateway_issuer));
    (gate, caller_issuer, gateway_issuer)
}

fn gate_over_gateway(identity: &sutura_config::InboundIdentity, issuer: &MockIssuer) -> InboundGate {
    gate_over(identity, issuer.key_set().as_str())
}

#[tokio::test]
async fn an_accepted_pairing_establishes_the_caller_from_the_user_token_not_the_gateway() {
    let (gate, caller_issuer, gateway_issuer) = paired_gate();
    let caller_token = signed_for(&caller_issuer, "a-user@example.com");
    let gateway_token = signed_for(&gateway_issuer, "the-fronting-component");
    let headers = paired(TOKEN_HEADER, &caller_token, &gateway_token);
    let caller = gate
        .establish(&headers, Instant::now())
        .await
        .expect("a valid caller token beside a valid gateway assertion is admitted");
    assert_eq!(
        caller.chain().subject(),
        &Subject::verified_by(caller.verifier().id().clone(), "a-user@example.com").expect("a fixture subject")
    );
}

#[tokio::test]
async fn an_absent_gateway_assertion_refuses_the_whole_request() {
    let (gate, caller_issuer, _gateway_issuer) = paired_gate();
    let caller_token = signed_for(&caller_issuer, "a-user@example.com");
    let headers = header_named(TOKEN_HEADER, &caller_token);
    assert!(matches!(
        gate.establish(&headers, Instant::now()).await,
        Err(TokenRejected::GatewayAssertionRequired(_))
    ));
}

#[tokio::test]
async fn an_invalid_gateway_assertion_refuses_the_whole_request() {
    let (gate, caller_issuer, _gateway_issuer) = paired_gate();
    let caller_token = signed_for(&caller_issuer, "a-user@example.com");
    // Signed by an issuer the gateway sub-gate does not trust at all.
    let forged_issuer = gateway(
        "https://not-the-real-gateway.example.com",
        "https://gateway.example.com/transit",
    );
    let forged = signed_for(&forged_issuer, "the-fronting-component");
    let headers = paired(TOKEN_HEADER, &caller_token, &forged);
    assert!(matches!(
        gate.establish(&headers, Instant::now()).await,
        Err(TokenRejected::GatewayAssertionRequired(_))
    ));
}

#[tokio::test]
async fn a_user_token_in_authorization_does_not_satisfy_the_gateway_check() {
    let (gate, caller_issuer, _gateway_issuer) = paired_gate();
    let caller_token = signed_for(&caller_issuer, "a-user@example.com");
    // The caller's own token, signed by the caller issuer, placed where the gateway assertion
    // belongs - it must fail the gateway sub-gate's own issuer check, never be accepted as it.
    let headers = paired(TOKEN_HEADER, &caller_token, &caller_token);
    assert!(matches!(
        gate.establish(&headers, Instant::now()).await,
        Err(TokenRejected::GatewayAssertionRequired(_))
    ));
}

#[tokio::test]
async fn an_anonymous_request_is_still_refused() {
    let (gate, _caller_issuer, _gateway_issuer) = paired_gate();
    let headers = axum::http::HeaderMap::new();
    assert!(matches!(
        gate.establish(&headers, Instant::now()).await,
        Err(TokenRejected::Absent { .. })
    ));
}

// -------------------------------------------------- through the real builder ----

/// Builds the pairing through [`InboundGate::from_verifiers_with_outbound`] itself, the entry
/// point production uses, rather than [`InboundGate::with_required_gateway`] - every cell above
/// hands the gate an in-memory key-set document because that seam takes one, but the critical
/// regression this module exists for (a `behind-gateway` target becoming independently
/// admissible, `github.com/telekom/sutura#2089` review item 1) lives in how
/// `from_verifiers_with_outbound` decides which declarations get their own mounted path, which no
/// test-seam cell can exercise. Two real files on disk, named after the process and removed after,
/// same as the one other filesystem cell in this suite (`refresh.rs`).
struct RealPairedGate {
    gate: InboundGate,
    caller_issuer: MockIssuer,
    gateway_issuer: MockIssuer,
    caller_path: std::path::PathBuf,
    gateway_path: std::path::PathBuf,
}

impl Drop for RealPairedGate {
    fn drop(&mut self) {
        drop(std::fs::remove_file(&self.caller_path));
        drop(std::fs::remove_file(&self.gateway_path));
    }
}

fn real_paired_gate() -> RealPairedGate {
    let caller_issuer = MockIssuer::generating(ISSUER, RESOURCE, "caller-key").expect("a fixture issuer generates a key");
    let gateway_issuer = gateway("https://real-gateway.example.com", "https://real-gateway.example.com/transit");
    let caller_path = std::env::temp_dir().join(format!("sutura-paired-caller-{}.json", std::process::id()));
    let gateway_path = std::env::temp_dir().join(format!("sutura-paired-gateway-{}.json", std::process::id()));
    caller_issuer.publish(&caller_path).expect("the caller key set is writable");
    gateway_issuer
        .publish(&gateway_path)
        .expect("the gateway key set is writable");
    let overlay = format!(
        "security:\n  inbound:\n    - id: staff\n      path: /staff\n      mode: direct\n      \
         resource: 'https://sutura.example.com/staff'\n      audience: '{}'\n      authorization_server: '{}'\n      \
         key_set_file: '{}'\n      algorithms: [ES256]\n      token_header: '{TOKEN_HEADER}'\n      requires_gateway: gateway\n    \
         - id: gateway\n      path: /gateway\n      mode: behind-gateway\n      transit_header: authorization\n      \
         transit_issuer: '{}'\n      transit_audience: '{}'\n      transit_token_type: at+jwt\n      key_set_file: '{}'\n      \
         algorithms: [ES256]\n",
        caller_issuer.audience(),
        caller_issuer.issuer(),
        caller_path.display(),
        gateway_issuer.issuer(),
        gateway_issuer.audience(),
        gateway_path.display(),
    );
    let settings = crate::testing::settings_with(&overlay);
    let gate = InboundGate::from_verifiers_with_outbound(settings.security().inbound(), None)
        .expect("the real builder accepts the pairing")
        .expect("at least one verifier is declared");
    RealPairedGate {
        gate,
        caller_issuer,
        gateway_issuer,
        caller_path,
        gateway_path,
    }
}

#[tokio::test]
async fn the_real_builder_admits_a_valid_pairing_but_refuses_the_gateway_token_presented_alone() {
    let fixture = real_paired_gate();
    let gate = &fixture.gate;
    let caller_token = signed_for(&fixture.caller_issuer, "a-user@example.com");
    let gateway_token = signed_for(&fixture.gateway_issuer, "the-fronting-component");

    // The accepted case: both headers present, caller established from the user token.
    let headers = paired(TOKEN_HEADER, &caller_token, &gateway_token);
    let caller = gate
        .establish(&headers, Instant::now())
        .await
        .expect("a valid caller token beside a valid gateway assertion is admitted");
    assert_eq!(
        caller.chain().subject(),
        &Subject::verified_by(caller.verifier().id().clone(), "a-user@example.com").expect("a fixture subject")
    );

    // The regression this cell exists for: the gateway's own token, presented alone in
    // `Authorization` with no caller token in `x-user-token`, must never be admitted as the
    // gateway's own identity. Before review item 1's fix, the `behind-gateway` target built by
    // `from_verifiers_with_outbound` was also mounted on its own path and routed by issuer in
    // `also_accept`, making exactly this request succeed with the gateway as caller.
    let gateway_only = header_named("authorization", &format!("Bearer {gateway_token}"));
    assert!(matches!(
        gate.establish(&gateway_only, Instant::now()).await,
        Err(TokenRejected::Absent { .. })
    ));
}

/// Review item 1: a token the GATEWAY issuer signed, placed in the caller's own `x-user-token`
/// header, beside a valid gateway assertion in `Authorization`. The caller slot is routed by the
/// `staff` verifier's own issuer/audience, which the gateway issuer never satisfies, so this must
/// fail as an ordinary caller-token rejection - never fall back to treating the gateway's own
/// signature as proof of the caller.
#[tokio::test]
async fn a_gateway_signed_token_in_the_caller_header_is_refused_even_beside_a_valid_gateway_assertion() {
    let fixture = real_paired_gate();
    let gate = &fixture.gate;
    let impostor = signed_for(&fixture.gateway_issuer, "the-fronting-component");
    let gateway_token = signed_for(&fixture.gateway_issuer, "the-fronting-component");
    let headers = paired(TOKEN_HEADER, &impostor, &gateway_token);
    // `establish`'s single-verifier fast path (`also_accept` is empty here) never peeks at `iss` -
    // it hands the presented token straight to `staff`'s own `establish_token`. The impostor's
    // `kid` names the gateway issuer's signing key, which `staff`'s own key set never carries, so
    // this fails at key lookup rather than at issuer selection.
    assert!(matches!(
        gate.establish(&headers, Instant::now()).await,
        Err(TokenRejected::NoKey { .. })
    ));
}

/// Review item 2: `#1884`'s general refusal of two configured credential locations still fires for
/// a combination this pairing does NOT name, even though a paired verifier exists elsewhere in the
/// same real-built gate. A third, standalone `behind-gateway` verifier mounted on `staff`'s own
/// path, reading its own token from its own header (`x-transit-proof`), is not named by any
/// `requires_gateway` and so lands in `also_accept` same as any other co-mounted verifier - a
/// caller token in `x-user-token` AND a second, unrelated token in `x-transit-proof` are two
/// distinct locations on the one gate `select_from` walks, which is exactly the shape
/// `AmbiguousProof` exists to refuse, pairing or no pairing.
#[tokio::test]
async fn ambiguous_proof_still_fires_for_an_unrelated_two_location_combination() {
    let caller_issuer = MockIssuer::generating(ISSUER, RESOURCE, "caller-key").expect("a fixture issuer generates a key");
    let gateway_issuer = gateway(
        "https://ambiguous-gateway.example.com",
        "https://ambiguous-gateway.example.com/transit",
    );
    let transit_issuer = gateway(
        "https://unrelated-transit.example.com",
        "https://unrelated-transit.example.com/transit",
    );
    let caller_path = std::env::temp_dir().join(format!("sutura-paired-ambiguous-caller-{}.json", std::process::id()));
    let gateway_path = std::env::temp_dir().join(format!("sutura-paired-ambiguous-gateway-{}.json", std::process::id()));
    let transit_path = std::env::temp_dir().join(format!("sutura-paired-ambiguous-transit-{}.json", std::process::id()));
    caller_issuer.publish(&caller_path).expect("the caller key set is writable");
    gateway_issuer
        .publish(&gateway_path)
        .expect("the gateway key set is writable");
    transit_issuer
        .publish(&transit_path)
        .expect("the unrelated transit key set is writable");
    let overlay = format!(
        "security:\n  inbound:\n    - id: staff\n      path: /staff\n      mode: direct\n      \
         resource: 'https://sutura.example.com/staff'\n      audience: '{}'\n      authorization_server: '{}'\n      \
         key_set_file: '{}'\n      algorithms: [ES256]\n      token_header: '{TOKEN_HEADER}'\n      requires_gateway: gateway\n    \
         - id: gateway\n      path: /gateway\n      mode: behind-gateway\n      transit_header: authorization\n      \
         transit_issuer: '{}'\n      transit_audience: '{}'\n      transit_token_type: at+jwt\n      key_set_file: '{}'\n      \
         algorithms: [ES256]\n    \
         - id: transit-proof\n      path: /staff\n      mode: behind-gateway\n      transit_header: x-transit-proof\n      \
         transit_issuer: '{}'\n      transit_audience: '{}'\n      transit_token_type: at+jwt\n      key_set_file: '{}'\n      \
         algorithms: [ES256]\n",
        caller_issuer.audience(),
        caller_issuer.issuer(),
        caller_path.display(),
        gateway_issuer.issuer(),
        gateway_issuer.audience(),
        gateway_path.display(),
        transit_issuer.issuer(),
        transit_issuer.audience(),
        transit_path.display(),
    );
    let settings = crate::testing::settings_with(&overlay);
    let gate = InboundGate::from_verifiers_with_outbound(settings.security().inbound(), None)
        .expect("the real builder accepts a third standalone verifier beside the pairing")
        .expect("at least one verifier is declared");

    let caller_token = signed_for(&caller_issuer, "a-user@example.com");
    let transit_token = signed_for(&transit_issuer, "someone-else");
    let mut headers = header_named(TOKEN_HEADER, &caller_token);
    headers.extend(header_named("x-transit-proof", &transit_token));
    assert!(matches!(
        gate.establish(&headers, Instant::now()).await,
        Err(TokenRejected::AmbiguousProof)
    ));

    drop(std::fs::remove_file(&caller_path));
    drop(std::fs::remove_file(&gateway_path));
    drop(std::fs::remove_file(&transit_path));
}

/// Review item 3: the gateway token presented alone is refused through `establish_on`, the
/// path-scoped entry point the HTTP router actually calls - on BOTH the paired verifier's own path
/// and the gateway target's path. Proves the dead-mount fix (`crates/sutura-cli/src/serve.rs`'s
/// `agent_mounts` no longer advertises `/gateway` at all) is backed by the gate itself refusing
/// that path too, not only by the router declining to mount it.
#[tokio::test]
async fn the_gateway_token_alone_is_refused_on_both_paths_through_establish_on() {
    let fixture = real_paired_gate();
    let gate = &fixture.gate;
    let gateway_token = signed_for(&fixture.gateway_issuer, "the-fronting-component");
    let headers = header_named("authorization", &format!("Bearer {gateway_token}"));
    let staff_path = sutura_config::McpPath::parse("/staff").expect("a fixture path");
    let gateway_path = sutura_config::McpPath::parse("/gateway").expect("a fixture path");
    assert!(matches!(
        gate.establish_on(&staff_path, &headers, Instant::now()).await,
        Err(TokenRejected::Absent { .. })
    ));
    assert!(matches!(
        gate.establish_on(&gateway_path, &headers, Instant::now()).await,
        Err(TokenRejected::Absent { .. })
    ));
}

/// Review item 4: a delegation/token-exchange source is driven from the paired caller's own USER
/// token, never the gateway's assertion that rode beside it.
#[tokio::test]
async fn delegation_exchanges_the_paired_callers_user_token_not_the_gateway_assertion() {
    use std::sync::{Arc, OnceLock};

    use sutura_config::DeclaredPrincipalBroker;
    use sutura_config::credentials::SourceAdmissions;
    use sutura_domain::identity::{
        CredentialBroker as _, Delegated, Delegation, DelegationExchange, DelegationFailed, RequestedAudience, Secret, SourceSet,
        VerifierId,
    };

    #[derive(Debug, Default)]
    struct Capturing(OnceLock<String>);

    impl DelegationExchange for Capturing {
        #[expect(clippy::disallowed_methods, reason = "the cell asserts which token delegation received")]
        fn exchange(&self, subject: &Secret, _audience: &RequestedAudience) -> Result<Delegated, DelegationFailed> {
            drop(self.0.set(subject.expose_secret().to_owned()));
            Ok(Delegated::new(Secret::new("exchanged"), 4_102_444_800))
        }
    }

    let fixture = real_paired_gate();
    let gate = &fixture.gate;
    let caller_token = signed_for(&fixture.caller_issuer, "a-user@example.com");
    let gateway_token = signed_for(&fixture.gateway_issuer, "the-fronting-component");
    let headers = paired(TOKEN_HEADER, &caller_token, &gateway_token);
    let caller = gate
        .establish(&headers, Instant::now())
        .await
        .expect("a valid caller token beside a valid gateway assertion is admitted");

    let capturing = Arc::new(Capturing::default());
    let delegation = Delegation::through(
        Arc::<Capturing>::clone(&capturing),
        RequestedAudience::parse("https://pool.example.com").expect("a fixture audience"),
    );
    let broker = DeclaredPrincipalBroker::empty()
        .admitting(SourceAdmissions::default().and(
            crate::testing::source(),
            Some(std::collections::BTreeSet::from([
                VerifierId::parse("staff").expect("a fixture verifier"),
            ])),
        ))
        .federating(crate::testing::source(), Some(delegation));
    let context = crate::principal::of_verified(&caller);
    let _minted = broker
        .mint(&context, &SourceSet::of(crate::testing::source()))
        .expect("the broker answers");
    assert_eq!(
        capturing.0.get().map(String::as_str),
        Some(caller_token.as_str()),
        "delegation must exchange the caller's own user token, never the gateway assertion"
    );
}

/// Review item 5: a mixed-case `token_header` in config (`X-User-Token`) still matches a
/// lower-case incoming header name - header names are case-insensitive over the wire, and
/// `ProofHeader` folds to lower case at parse time, so the match must hold either way.
#[tokio::test]
async fn a_mixed_case_configured_token_header_matches_the_lower_case_incoming_header() {
    /// Removes its temp key-set files on every exit path, success or panic - same guarantee
    /// [`RealPairedGate`]'s own `Drop` gives the other filesystem fixture in this module.
    struct CleanupKeySets(Vec<std::path::PathBuf>);
    impl Drop for CleanupKeySets {
        fn drop(&mut self) {
            for path in &self.0 {
                drop(std::fs::remove_file(path));
            }
        }
    }

    let caller_issuer = MockIssuer::generating(ISSUER, RESOURCE, "caller-key").expect("a fixture issuer generates a key");
    let gateway_issuer = gateway("https://mixed-case.example.com", "https://mixed-case.example.com/transit");
    let caller_path = std::env::temp_dir().join(format!("sutura-paired-mixed-case-caller-{}.json", std::process::id()));
    let gateway_path = std::env::temp_dir().join(format!("sutura-paired-mixed-case-gateway-{}.json", std::process::id()));
    let _cleanup = CleanupKeySets(vec![caller_path.clone(), gateway_path.clone()]);
    caller_issuer.publish(&caller_path).expect("the caller key set is writable");
    gateway_issuer
        .publish(&gateway_path)
        .expect("the gateway key set is writable");
    let overlay = format!(
        "security:\n  inbound:\n    - id: staff\n      path: /staff\n      mode: direct\n      \
         resource: 'https://sutura.example.com/staff'\n      audience: '{}'\n      authorization_server: '{}'\n      \
         key_set_file: '{}'\n      algorithms: [ES256]\n      token_header: 'X-User-Token'\n      requires_gateway: gateway\n    \
         - id: gateway\n      path: /gateway\n      mode: behind-gateway\n      transit_header: authorization\n      \
         transit_issuer: '{}'\n      transit_audience: '{}'\n      transit_token_type: at+jwt\n      key_set_file: '{}'\n      \
         algorithms: [ES256]\n",
        caller_issuer.audience(),
        caller_issuer.issuer(),
        caller_path.display(),
        gateway_issuer.issuer(),
        gateway_issuer.audience(),
        gateway_path.display(),
    );
    let settings = crate::testing::settings_with(&overlay);
    let gate = InboundGate::from_verifiers_with_outbound(settings.security().inbound(), None)
        .expect("the real builder accepts a mixed-case token_header")
        .expect("at least one verifier is declared");

    let caller_token = signed_for(&caller_issuer, "a-user@example.com");
    let gateway_token = signed_for(&gateway_issuer, "the-fronting-component");
    // The incoming header name is sent lower-case, as `header_named`/`paired` always do.
    let headers = paired("x-user-token", &caller_token, &gateway_token);
    let caller = gate
        .establish(&headers, Instant::now())
        .await
        .expect("the lower-case incoming header matches the mixed-case configured name");
    assert_eq!(
        caller.chain().subject(),
        &Subject::verified_by(caller.verifier().id().clone(), "a-user@example.com").expect("a fixture subject")
    );
}
