//! A gateway assertion declared single use, through the gate - `docs/adr/20261015094920`.
//!
//! The declaration is loaded from settings, as a deployment's is, so each cell also says that
//! `security.inbound.transit_single_use` is a key `sutura-config` reads. The store's own bound, its
//! expiry and its full state are asserted beside it in `crate::inbound::replay`.

use std::time::{Duration, Instant};

use crate::inbound::gate::InboundGate;

use super::{ISSUER, KID, RESOURCE, TRANSIT_HEADER, cache_over, expiry_in, jwks, key_pair, signed, transit};

/// A `behind-gateway` gate over `pair`'s key that declares single use, as a deployment loads it.
fn single_use_gate(pair: &rcgen::KeyPair) -> Result<InboundGate, sutura_config::SettingsLoadError> {
    gate_declaring(pair, true)
}

/// A `behind-gateway` gate over `pair`'s key, with `single_use` as its `transit_single_use`.
fn gate_declaring(pair: &rcgen::KeyPair, single_use: bool) -> Result<InboundGate, sutura_config::SettingsLoadError> {
    let overlay = format!(
        "security:\n  inbound:\n    mode: \"behind-gateway\"\n    transit_header: \"{TRANSIT_HEADER}\"\n    \
         transit_issuer: \"{ISSUER}\"\n    transit_audience: \"{RESOURCE}\"\n    key_set_file: \"/unread.json\"\n    \
         algorithms: [\"ES256\"]\n    transit_token_type: \"at+jwt\"\n    transit_single_use: {single_use}\n"
    );
    let settings = sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(overlay),
    )?;
    let (cache, _) = cache_over(&[jwks(KID, pair)], Duration::from_secs(30), Instant::now());
    Ok(InboundGate::over(&crate::testing::declared_inbound(&settings), cache))
}

/// An assertion issued `issued_ago` seconds back, living `lifetime` seconds, carrying `jti` if any.
fn assertion_with_jti(pair: &rcgen::KeyPair, issued_ago: i64, lifetime: i64, jti: Option<&str>) -> String {
    let issued = expiry_in(-issued_ago);
    let mut claims = serde_json::json!({
        "sub": "someone@example.com",
        "aud": RESOURCE,
        "iss": ISSUER,
        "iat": issued,
        "exp": issued + lifetime,
    });
    if let Some(jti) = jti {
        claims["jti"] = serde_json::Value::from(jti);
    }
    signed(pair, KID, &claims)
}

/// An assertion living 60 seconds from now whose `jti` is `jti`, whatever its JSON type.
fn assertion_claiming_jti(pair: &rcgen::KeyPair, jti: &serde_json::Value) -> String {
    let issued = expiry_in(0);
    let claims = serde_json::json!({
        "sub": "someone@example.com",
        "aud": RESOURCE,
        "iss": ISSUER,
        "iat": issued,
        "exp": issued + 60,
        "jti": jti,
    });
    signed(pair, KID, &claims)
}

#[tokio::test]
async fn a_replayed_assertion_is_refused_when_single_use_is_declared() {
    let gateway = key_pair();
    let gate = single_use_gate(&gateway).expect("`transit_single_use` is a setting");
    let once = assertion_with_jti(&gateway, 0, 60, Some("one"));

    drop(
        gate.establish(&transit(&once), Instant::now())
            .await
            .expect("the first presentation establishes a caller"),
    );
    let replayed = gate
        .establish(&transit(&once), Instant::now())
        .await
        .expect_err("the second presentation of one assertion establishes nobody");
    assert!(replayed.to_string().contains("already presented"), "{replayed}");

    let other = assertion_with_jti(&gateway, 0, 60, Some("two"));
    drop(
        gate.establish(&transit(&other), Instant::now())
            .await
            .expect("another `jti` is another assertion"),
    );
}

#[tokio::test]
async fn an_assertion_without_a_jti_is_refused_when_single_use_is_declared() {
    let gateway = key_pair();
    let gate = single_use_gate(&gateway).expect("`transit_single_use` is a setting");
    let refused = gate
        .establish(&transit(&assertion_with_jti(&gateway, 0, 60, None)), Instant::now())
        .await
        .expect_err("an assertion with no `jti` cannot be admitted once");
    assert!(refused.to_string().contains("no `jti`"), "{refused}");
}

#[tokio::test]
async fn an_assertion_a_claim_check_refuses_never_takes_a_place_in_the_store() {
    let gateway = key_pair();
    let gate = single_use_gate(&gateway).expect("`transit_single_use` is a setting");
    // Signed by the component and over the default 120-second ceiling, so a claim check refuses it.
    let too_long = assertion_with_jti(&gateway, 0, 600, Some("shared"));
    drop(
        gate.establish(&transit(&too_long), Instant::now())
            .await
            .expect_err("over the ceiling"),
    );

    let within = assertion_with_jti(&gateway, 0, 60, Some("shared"));
    drop(
        gate.establish(&transit(&within), Instant::now())
            .await
            .expect("the refused assertion never took the `jti`"),
    );
}

#[tokio::test]
async fn an_assertion_inside_the_leeway_is_still_single_use() {
    let gateway = key_pair();
    let gate = single_use_gate(&gateway).expect("`transit_single_use` is a setting");
    // Expired ten seconds ago, so only the 30-second leeway admits it - and holds its `jti` through it.
    let late = assertion_with_jti(&gateway, 70, 60, Some("late"));
    drop(
        gate.establish(&transit(&late), Instant::now())
            .await
            .expect("the leeway admits it once"),
    );
    let replayed = gate
        .establish(&transit(&late), Instant::now())
        .await
        .expect_err("the store holds the `jti` until the leeway ends, not until `exp`");
    assert!(replayed.to_string().contains("already presented"), "{replayed}");
}

#[tokio::test]
async fn a_jti_the_store_cannot_hold_is_refused_when_single_use_is_declared() {
    let gateway = key_pair();
    let gate = single_use_gate(&gateway).expect("`transit_single_use` is a setting");
    for jti in [serde_json::Value::from(7), serde_json::Value::from("x".repeat(129))] {
        let refused = gate
            .establish(&transit(&assertion_claiming_jti(&gateway, &jti)), Instant::now())
            .await
            .expect_err("a `jti` that is not a string of 1 to 128 bytes cannot be admitted once");
        assert!(refused.to_string().contains("is not a string of 1 to 128 bytes"), "{refused}");
    }
}

#[tokio::test]
async fn a_jti_that_is_not_a_string_is_admitted_when_single_use_is_off() {
    let gateway = key_pair();
    let gate = gate_declaring(&gateway, false).expect("`transit_single_use` is a setting");
    let numbered = assertion_claiming_jti(&gateway, &serde_json::Value::from(7));
    for _ in 0..2_u8 {
        drop(
            gate.establish(&transit(&numbered), Instant::now())
                .await
                .expect("with single use off, the `jti` is not read"),
        );
    }
}
