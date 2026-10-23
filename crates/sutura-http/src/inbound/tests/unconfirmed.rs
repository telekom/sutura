//! A key set source that stays failing, past the declared `key_set_max_stale_seconds`.
//!
//! Every cell drives a scripted source and a clock it controls. Each script is the boot read, one
//! look that confirms nothing, then a source that answers again.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::http::{HeaderMap, StatusCode};
use sutura_config::{
    InboundIdentity, IssuerUrl, KeyFamily, KeySetLocation, KeySetUrl, PinnedAlgorithms, ProofLifetime, RequiredTokenType,
    ResourceIdentifier, SigningAlgorithm,
};
use tracing::instrument::WithSubscriber as _;

use super::fixtures::{
    StubSource, a_token, bearer, cache_around, claims, direct, gateway_declaring, jwks, key_pair, short_lived_assertion, signed,
    transit,
};
use super::{IDENTITY_UNAVAILABLE, TRANSIT_HEADER, guarded_answer};
use crate::inbound::gate::InboundGate;
use crate::inbound::keys::{MAX_KEY_SET_AGE, MIN_REFETCH_INTERVAL};
use crate::testing::{ISSUER, KID, RESOURCE};

/// A document the cache refuses, in place of an unreadable source.
const REFUSED: &str = r#"{"keys":[]}"#;

/// A declaration loaded from YAML, so the ceiling arrives the way an operator writes it.
fn declared(mode: &str, source: &str, seconds: u64) -> sutura_config::Settings {
    let mode = match mode {
        "direct" => format!("mode: \"direct\"\n    resource: \"{RESOURCE}\"\n    authorization_server: \"{ISSUER}\""),
        _ => format!(
            "mode: \"behind-gateway\"\n    transit_header: \"{TRANSIT_HEADER}\"\n    transit_issuer: \"{ISSUER}\"\n    \
             transit_audience: \"{RESOURCE}\"\n    transit_token_type: \"at+jwt\""
        ),
    };
    let overlay = format!(
        "security:\n  inbound:\n    {mode}\n    {source}\n    algorithms: [\"ES256\"]\n    key_set_max_stale_seconds: {seconds}\n"
    );
    sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(overlay),
    )
    .expect("a declared ceiling inside its range starts")
}

/// A `direct` declaration reading its keys from a URL, with the default ceiling.
fn direct_from_url() -> InboundIdentity {
    InboundIdentity::Direct {
        resource: ResourceIdentifier::parse(RESOURCE).expect("a test resource is a resource"),
        authorization_server: IssuerUrl::parse(ISSUER).expect("a test issuer is an issuer"),
        key_set: KeySetLocation::Url(KeySetUrl::parse("https://issuer.example.com/jwks").expect("a test URL is a URL")),
        algorithms: PinnedAlgorithms::of(SigningAlgorithm::Es256),
        token_type: RequiredTokenType::access_token(),
        address_claim: sutura_config::AddressClaim::VerifiedEmail,
    }
}

/// The headers a caller of this mode presents, holding a token signed by `pair`.
fn presented(declaration: &InboundIdentity, pair: &rcgen::KeyPair) -> HeaderMap {
    match *declaration {
        InboundIdentity::Direct { .. } => bearer(&a_token(pair)),
        InboundIdentity::BehindGateway { .. } => transit(&short_lived_assertion(pair)),
    }
}

/// The headers of a caller whose token names a key id the held set does not contain.
fn presented_unknown(declaration: &InboundIdentity, pair: &rcgen::KeyPair) -> HeaderMap {
    let token = signed(pair, "not-in-the-set", &claims("someone@example.com", RESOURCE, ISSUER, ""));
    match *declaration {
        InboundIdentity::Direct { .. } => bearer(&token),
        InboundIdentity::BehindGateway { .. } => transit(&token),
    }
}

/// The held key verifies one second inside `ceiling`, is refused at it, and verifies again once a
/// look succeeds. So is a key id the set does not hold: it is refused as unconfirmed, never as a
/// rate-limited re-read. `unconfirmed` is the look in between: `None` fails to read, `Some` is refused.
async fn holds_until(declaration: &InboundIdentity, ceiling: u64, unconfirmed: Option<&str>) {
    let pair = key_pair();
    let good = jwks(KID, &pair);
    let now = Instant::now();
    let script = vec![Some(good.clone()), unconfirmed.map(String::from), Some(good)];
    let (cache, _source) = cache_around(
        StubSource::scripted(script, None),
        KeyFamily::EllipticCurve,
        MIN_REFETCH_INTERVAL,
        MAX_KEY_SET_AGE,
        now,
    );
    let gate = InboundGate::over(declaration, cache);
    let headers = presented(declaration, &pair);
    let unknown = presented_unknown(declaration, &pair);
    let at = |seconds| now + Duration::from_secs(seconds);
    drop(
        gate.establish(&headers, at(ceiling - 1))
            .await
            .expect("inside the ceiling the held key verifies"),
    );
    let refused = gate
        .establish(&headers, at(ceiling))
        .await
        .expect_err("at the ceiling no held key verifies");
    let ms = ceiling * 1000;
    let unconfirmed_for_ceiling = Some(format!(
        "no look has confirmed the key set for {ms}ms, past the declared {ms}ms; no held key verifies until a look succeeds"
    ));
    assert_eq!(
        core::error::Error::source(&refused).map(ToString::to_string),
        unconfirmed_for_ceiling,
        "{declaration:?}"
    );
    let refused_unknown = gate
        .establish(&unknown, at(ceiling))
        .await
        .expect_err("at the ceiling an unknown key id is refused too");
    assert_eq!(
        core::error::Error::source(&refused_unknown).map(ToString::to_string),
        unconfirmed_for_ceiling,
        "{declaration:?}"
    );
    drop(
        gate.establish(&headers, at(ceiling + MIN_REFETCH_INTERVAL.as_secs()))
            .await
            .expect("a look that succeeds verifies again"),
    );
}

#[tokio::test]
async fn a_source_failing_past_the_default_ceiling_stops_verifying_until_a_look_succeeds() {
    let gateway = gateway_declaring(RequiredTokenType::access_token(), ProofLifetime::default_lifetime());
    for declaration in [direct(), direct_from_url(), gateway] {
        holds_until(&declaration, 3600, None).await;
    }
}

#[tokio::test]
async fn a_refused_document_past_the_ceiling_stops_verifying_too() {
    holds_until(&direct(), 3600, Some(REFUSED)).await;
}

#[tokio::test]
async fn the_ceiling_follows_the_declaration_for_either_source_in_either_mode() {
    let file = "key_set_file: \"/unread.json\"";
    let url = "key_set_url: \"https://issuer.example.com/jwks\"";
    for (mode, source, ceiling) in [
        ("direct", file, 600),
        ("direct", url, 600),
        ("behind-gateway", url, 300),
        ("behind-gateway", file, 86_400),
    ] {
        let settings = declared(mode, source, ceiling);
        let declaration = settings.security().inbound().expect("an inbound identity");
        holds_until(declaration, ceiling, None).await;
    }
}

#[tokio::test]
async fn every_look_that_confirms_nothing_says_how_stale_the_keys_are() {
    // The first look reads the held document again and confirms it, and logs no staleness line. The
    // third reads the same refused bytes as the second, which `adopt` records as `Unchanged`.
    let pair = key_pair();
    let now = Instant::now();
    let (cache, _source) = cache_around(
        StubSource::scripted(
            vec![Some(jwks(KID, &pair)), Some(jwks(KID, &pair)), Some(String::from(REFUSED))],
            None,
        ),
        KeyFamily::EllipticCurve,
        MIN_REFETCH_INTERVAL,
        MAX_KEY_SET_AGE,
        now,
    );
    let sink = sutura_runtime::testing::Capture::new();
    let telemetry = sutura_config::TelemetrySettings::new(
        sutura_config::ServiceName::parse("sutura-test").expect("a test service name is a name"),
        sutura_config::LogFilter::parse("info").expect("a test directive is a directive"),
        sutura_config::LogFormat::Bunyan,
        true,
    );
    let dispatch = tracing::Dispatch::new(
        sutura_runtime::telemetry::subscriber(&telemetry, sink.clone()).expect("a valid directive builds a subscriber"),
    );
    for look in 1..=3 {
        let _outcome = cache
            .poll_once(now + MIN_REFETCH_INTERVAL * look)
            .with_subscriber(dispatch.clone())
            .await;
    }
    let rendered = sink.contents();
    // A `u128` field reaches the bunyan line as its debug text, so a number is read either way.
    let text = |value: &serde_json::Value| value.as_str().map_or_else(|| value.to_string(), String::from);
    let staleness: Vec<_> = rendered
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|line| line.get("stale_for_ms").is_some())
        .map(|line| (text(&line["stale_for_ms"]), text(&line["max_stale_ms"])))
        .collect();
    let line = |stale_for: &str| (String::from(stale_for), String::from("3600000"));
    assert_eq!(staleness, [line("30000"), line("60000")], "{rendered}");
}

#[tokio::test]
async fn past_the_ceiling_a_caller_gets_503_identity_unavailable_with_no_challenge() {
    // The middleware reads the real clock, so the boot read is placed 301 seconds back. On a host up
    // for less than that the cell fails, never passes.
    let pair = key_pair();
    let booted = Instant::now()
        .checked_sub(Duration::from_secs(301))
        .expect("this host has been up for more than 301 seconds");
    let (cache, _source) = cache_around(
        StubSource::scripted(vec![Some(jwks(KID, &pair)), None], None),
        KeyFamily::EllipticCurve,
        MIN_REFETCH_INTERVAL,
        MAX_KEY_SET_AGE,
        booted,
    );
    let settings = declared("direct", "key_set_file: \"/unread.json\"", 300);
    let gate = Arc::new(InboundGate::over(
        settings.security().inbound().expect("an inbound identity"),
        cache,
    ));
    let (status, ref body, challenged) = guarded_answer(&gate, &a_token(&pair)).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body, IDENTITY_UNAVAILABLE);
    assert!(!challenged, "an outage carries no challenge to present a different token");
}
