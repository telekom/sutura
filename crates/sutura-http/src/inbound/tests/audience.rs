//! An audience that is not the resource identifier.

use std::time::Instant;

use super::fixtures::{bearer, claims, gate_over, jwks, key_pair, signed_as};
use crate::testing::{ISSUER, KID, RESOURCE};

/// The application id an Entra v2 access token carries as its `aud`.
pub(super) const CLIENT_ID: &str = "6f1c2c84-3b1e-4c8a-9d2e-5a0b7e9f1d23";

/// A `direct` declaration over [`RESOURCE`] that accepts a `typ: JWT` token, with `extra` appended.
pub(super) fn entra_declared(extra: &str) -> sutura_config::Settings {
    let overlay = format!(
        "security:\n  inbound:\n    mode: \"direct\"\n    resource: \"{RESOURCE}\"\n    authorization_server: \"{ISSUER}\"\n    \
         key_set_file: \"/unread.json\"\n    algorithms: [\"ES256\"]\n    token_type: \"any\"\n    accept_any_token_type: true\n{extra}"
    );
    sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(overlay),
    )
    .expect("an Entra-shaped declaration starts")
}

/// The scopes a caller was established with, in order.
fn scope_list(caller: &crate::inbound::VerifiedCaller) -> Vec<&str> {
    caller.scopes().iter().collect()
}

#[tokio::test]
async fn an_entra_shaped_token_grants_exactly_its_scp_or_roles_and_only_where_declared() {
    let pair = key_pair();
    let gate_for = |settings: &sutura_config::Settings| {
        gate_over(settings.security().inbound().expect("an inbound identity"), &jwks(KID, &pair))
    };
    let declared = entra_declared(&format!(
        "    audience: \"{CLIENT_ID}\"\n    extra_scope_claims: [\"scp\", \"roles\"]\n"
    ));
    let entra = gate_for(&declared);
    let user = signed_as(
        &pair,
        KID,
        &claims(
            "someone@example.com",
            CLIENT_ID,
            ISSUER,
            r#"{"azp":"0b7c1d4e-2f3a-4b5c-8d6e-7f8091a2b3c4","scp":"sutura:metrics.ask"}"#,
        ),
        Some("JWT"),
    );
    let app_only = signed_as(
        &pair,
        KID,
        &claims(
            "daemon@example.com",
            CLIENT_ID,
            ISSUER,
            r#"{"azp":"0b7c1d4e-2f3a-4b5c-8d6e-7f8091a2b3c4","roles":["sutura:catalog.read"]}"#,
        ),
        Some("JWT"),
    );
    let caller = entra
        .establish(&bearer(&user), Instant::now())
        .await
        .expect("a user token establishes");
    assert_eq!(scope_list(&caller), ["sutura:metrics.ask"], "a user token grants its `scp`");
    let caller = entra
        .establish(&bearer(&app_only), Instant::now())
        .await
        .expect("an app-only token establishes");
    assert_eq!(
        scope_list(&caller),
        ["sutura:catalog.read"],
        "an app-only token grants its `roles`"
    );

    let undeclared = gate_for(&entra_declared(&format!("    audience: \"{CLIENT_ID}\"\n")));
    let caller = undeclared
        .establish(&bearer(&user), Instant::now())
        .await
        .expect("the token still verifies");
    assert!(
        scope_list(&caller).is_empty(),
        "`scp` grants nothing where it is not declared"
    );
    let caller = undeclared
        .establish(&bearer(&app_only), Instant::now())
        .await
        .expect("the token still verifies");
    assert!(
        scope_list(&caller).is_empty(),
        "`roles` grants nothing where it is not declared"
    );

    let resource_audience = gate_for(&entra_declared("    extra_scope_claims: [\"scp\", \"roles\"]\n"));
    assert!(
        resource_audience.establish(&bearer(&user), Instant::now()).await.is_err(),
        "without `audience`, `aud` must be the resource identifier"
    );
}

#[test]
fn a_direct_audience_leaves_the_published_resource_https() {
    let settings = entra_declared(&format!("    audience: \"{CLIENT_ID}\"\n"));
    let gate = gate_over(
        settings.security().inbound().expect("an inbound identity"),
        &jwks(KID, &key_pair()),
    );
    let published = gate
        .protected_resource()
        .expect("a `direct` deployment publishes its metadata");
    assert_eq!(
        published.url(),
        "https://sutura.example.com/.well-known/oauth-protected-resource"
    );
    let challenge = gate
        .challenge(&"/v1/query".parse().expect("a request URI"), &axum::http::HeaderMap::new())
        .expect("a `direct` deployment challenges");
    assert!(
        challenge.starts_with("Bearer realm=\"https://sutura.example.com\""),
        "{challenge}"
    );
}
