//! Governor-layer proofs; these do not establish transport wiring.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use sutura_config::{ClientAddressSource, Quota, TrustedProxies};
use sutura_domain::identity::{
    AdmittingVerifier, ClientClaims, PrincipalChain, Secret, Subject, VerifiedAddress, VerifierId, VerifierMode,
};

use super::{ClientAddress, RateKey, RateKeyExtractor, RateLimit, api_rate_limit_layer, caller_rate_limit_layer};
use crate::inbound::{Groups, Scopes, VerifiedCaller};
use crate::metrics::Metrics;
use crate::testing::call;

const SUBJECT: &str = "user@example.com";

fn quota(burst: u32) -> Quota {
    Quota::parse("test", 1, burst).expect("a test quota is a quota")
}

fn metrics() -> (Metrics, sutura_runtime::metrics::Registry) {
    let mut builder = sutura_runtime::metrics::RegistryBuilder::default();
    let metrics = Metrics::install(&mut builder);
    (metrics, builder.build())
}

fn caller(subject: Subject) -> VerifiedCaller {
    let verifier = subject.verifier_id().cloned().unwrap_or_else(VerifierId::primary);
    VerifiedCaller::established(
        PrincipalChain::of(subject),
        Scopes::none(),
        Groups::none(),
        Secret::new("fixture assertion"),
        4_102_444_800,
        ClientClaims::default(),
        AdmittingVerifier::of(verifier, String::from("https://issuer.example.com"), VerifierMode::Direct),
    )
    .addressed(Some(VerifiedAddress::parse(SUBJECT).expect("the fixture address parses")))
}

fn verified(verifier: &str, subject: &str) -> VerifiedCaller {
    caller(
        Subject::verified_by(VerifierId::parse(verifier).expect("a fixture verifier id parses"), subject)
            .expect("a fixture subject parses"),
    )
}

fn request(path: &str, caller: Option<VerifiedCaller>, peer: u8) -> Request<Body> {
    let mut request = Request::builder()
        .uri(path)
        .body(Body::empty())
        .expect("a fixture request builds");
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, peer], 44444))));
    if let Some(caller) = caller {
        drop(request.extensions_mut().insert(caller));
    }
    request
}

fn app(layer: &RateLimit) -> Router {
    Router::new()
        .route("/first", get(|| async { StatusCode::OK }))
        .route_layer(layer.clone())
        .merge(
            Router::new()
                .route("/second", get(|| async { StatusCode::OK }))
                .route_layer(layer.clone()),
        )
}

#[tokio::test]
async fn a_verified_pair_has_one_quota_across_cloned_layers_and_changing_ips() {
    let (metrics, _registry) = metrics();
    let (layer, handle) = caller_rate_limit_layer(&metrics, quota(2)).expect("the caller tier builds");
    let app = app(&layer);
    for (path, peer) in [("/first", 1), ("/second", 2)] {
        assert_eq!(
            call(&app, request(path, Some(verified("first", SUBJECT)), peer)).await.0,
            StatusCode::OK
        );
    }
    assert_eq!(
        call(&app, request("/second", Some(verified("first", SUBJECT)), 3)).await.0,
        StatusCode::TOO_MANY_REQUESTS,
        "changing the peer or layer must not replenish the verified pair"
    );
    for (verifier, subject, peer) in [
        ("second", SUBJECT, 4),
        ("first", "another-subject", 5),
        ("first", " user@example.com ", 6),
    ] {
        assert_eq!(
            call(&app, request("/first", Some(verified(verifier, subject)), peer)).await.0,
            StatusCode::OK,
            "another verifier or exact signed subject needs its own bucket, even with the same address claim"
        );
    }
    assert_eq!(handle.tracked(), 4);
}

#[tokio::test]
async fn a_caller_quota_refuses_missing_and_deployment_local_identity_without_ip_fallback() {
    let (metrics, _registry) = metrics();
    let (layer, handle) = caller_rate_limit_layer(&metrics, quota(1)).expect("the caller tier builds");
    let app = app(&layer);
    for identity in [None, Some(caller(Subject::TheDeploymentItself))] {
        assert_eq!(
            call(&app, request("/first", identity, 1)).await.0,
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(
            handle.tracked(),
            0,
            "an absent verified subject must not mint an address bucket"
        );
    }
}

#[tokio::test]
async fn network_and_caller_quotas_charge_separately_and_share_bounded_metrics() {
    let (metrics, registry) = metrics();
    let network_key = ClientAddress::new(ClientAddressSource::Peer, Arc::new(TrustedProxies::default()));
    let (network, network_handle) = api_rate_limit_layer(&metrics, quota(1), network_key).expect("the network tier builds");
    let (callers, caller_handle) = caller_rate_limit_layer(&metrics, quota(1)).expect("the caller tier builds");
    let app = app(&callers).layer(network);
    assert_eq!(
        call(&app, request("/first", Some(verified("first", SUBJECT)), 1)).await.0,
        StatusCode::OK,
        "a valid request must not spend two cells of the network burst"
    );
    assert_eq!(network_handle.tracked(), 1);
    assert_eq!(caller_handle.tracked(), 1);
    assert_eq!(
        call(&app, request("/second", Some(verified("first", SUBJECT)), 2)).await.0,
        StatusCode::TOO_MANY_REQUESTS,
        "a fresh network bucket must not bypass the caller quota"
    );
    assert_eq!(
        call(&app, request("/first", Some(verified("second", SUBJECT)), 1)).await.0,
        StatusCode::TOO_MANY_REQUESTS,
        "another caller still shares the retained network cap"
    );
    assert_eq!(
        caller_handle.tracked(),
        1,
        "a network refusal must not reach the inner caller store"
    );
    let watched = [
        super::WatchedTier {
            tier: crate::metrics::GENERAL_TIER,
            config: network_handle.watch(),
            metrics: metrics.clone(),
        },
        super::WatchedTier {
            tier: crate::metrics::GENERAL_TIER,
            config: caller_handle.watch(),
            metrics,
        },
    ];
    assert_eq!(super::sweep_once(&watched), 2);
    let rendered = registry.render();
    assert!(
        rendered.contains("sutura_rate_limit_buckets{tier=\"general\"} 3"),
        "{rendered}"
    );
    assert!(
        rendered.contains("sutura_rate_limited_total{tier=\"general\"} 2"),
        "{rendered}"
    );
    assert!(!rendered.contains(SUBJECT), "identity text must not become a metric label");
}

#[test]
fn caller_rate_buckets_are_reaped_and_debug_keeps_the_signed_subject_masked() {
    let (metrics, _registry) = metrics();
    let quota = Quota::parse("test", u32::MAX, u32::MAX).expect("a fast test quota parses");
    let (_layer, handle) = caller_rate_limit_layer(&metrics, quota).expect("the caller tier builds");
    let caller = verified("first", SUBJECT);
    let request = request("/first", Some(caller), 1);
    let key = tower_governor::key_extractor::KeyExtractor::extract(&RateKeyExtractor::VerifiedCaller, &request)
        .expect("the verified subject is a key");
    assert!(matches!(key, RateKey::Caller(_)));
    assert!(!format!("{key:?}").contains(SUBJECT));
    handle.config.limiter().check_key(&key).expect("the caller has quota");
    assert_eq!(handle.tracked(), 1);
    std::thread::sleep(Duration::from_millis(20));
    handle.reap();
    assert_eq!(handle.tracked(), 0, "the caller store must be reaped like the network store");
}
