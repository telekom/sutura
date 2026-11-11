//! Multi-verifier admission through the assembled router: a primary `direct` verifier and a
//! secondary `behind-gateway` verifier, each with its own header location.
//!
//! **Finding 1 of PR #1884's review**: the gate must read the token from the SELECTED verifier's
//! configured header, not always from the primary gate's header. These tests confirm that:
//! - A direct caller presenting their token in `Authorization: Bearer` is admitted.
//! - A gateway caller presenting their assertion in the secondary header is admitted when
//!   `Authorization` is absent.
//! - Presenting a token in the wrong header for its issuer is refused.

use axum::http::StatusCode;
use sutura_dev::issuer::MockIssuer;

use crate::testing::{
    accepted_by, an_issuer, broker, bundle, declared_inbound, direct_overlay, fake_warehouse, serving, settings_with,
};

use super::super::fixtures::gate_over_multi;
use super::{UNREAD, assertion_from, status_of};

/// A secondary `behind-gateway` issuer, using a different host to avoid the audience and issuer
/// overlap refusals that would prevent the settings from loading.
const GATEWAY_ISSUER: &str = "https://gateway.example.com";
/// A distinct path-form audience for the secondary gateway verifier.
const GATEWAY_AUDIENCE: &str = "/hub/sutura/v1";

/// Builds an app serving a two-verifier deployment:
/// - primary: `direct` mode, `Authorization: Bearer`, primary issuer and resource
/// - secondary: `behind-gateway` mode, `X-Transit-Proof`, gateway issuer and audience
///
/// Each issuer's key set is handed in directly rather than read from a file.
fn app_multi_verifier(primary: &MockIssuer, secondary: &MockIssuer) -> axum::Router {
    let overlay = format!(
        "{}  also_accept:\n    - mode: \"behind-gateway\"\n      transit_header: \"X-Transit-Proof\"\n      \
         transit_issuer: \"{GATEWAY_ISSUER}\"\n      transit_audience: \"{GATEWAY_AUDIENCE}\"\n      \
         key_set_file: \"{UNREAD}\"\n      algorithms: [\"ES256\"]\n      transit_token_type: \"at+jwt\"\n",
        direct_overlay(primary, UNREAD)
    );
    let settings = settings_with(&overlay);
    let primary_decl = declared_inbound(&settings);
    let also_accept = settings.security().also_accept();
    let gate = gate_over_multi(&primary_decl, &primary.key_set(), &[(&also_accept[0], &secondary.key_set())]);
    serving(bundle(), fake_warehouse(), broker(), settings, Some(gate))
}

/// A short-lived gateway assertion for `subject`, carrying every scope.
///
/// Gateway assertions live for 60 s rather than the full access-token lifetime, because the
/// behind-gateway validator enforces a lifetime ceiling by default.
fn gateway_assertion(secondary: &MockIssuer, subject: &str) -> String {
    secondary
        .mint(&accepted_by(subject).living_for(60))
        .expect("the secondary issuer signs an assertion")
}

#[tokio::test]
async fn a_direct_caller_in_the_primary_header_is_admitted() {
    // A verified token from the primary direct issuer, arriving in `Authorization: Bearer`,
    // is admitted through the multi-verifier gate and reaches the route handler.
    let primary = an_issuer();
    let secondary =
        MockIssuer::generating(GATEWAY_ISSUER, GATEWAY_AUDIENCE, "gw-key").expect("the secondary issuer generates a key pair");
    let app = app_multi_verifier(&primary, &secondary);
    let token = assertion_from(&primary, "direct-caller@example.com");
    assert_eq!(
        status_of(&app, ("POST", "/v1/query"), Some(&token), None, None).await,
        StatusCode::OK,
        "a direct caller's token in Authorization is admitted"
    );
}

#[tokio::test]
async fn a_gateway_caller_in_the_secondary_header_is_admitted() {
    // A verified assertion from the secondary behind-gateway issuer, arriving in
    // `X-Transit-Proof` with `Authorization` absent, is admitted through the multi-verifier gate.
    // This is the core property of Finding 1: the gate reads from the SELECTED verifier's header.
    let primary = an_issuer();
    let secondary =
        MockIssuer::generating(GATEWAY_ISSUER, GATEWAY_AUDIENCE, "gw-key").expect("the secondary issuer generates a key pair");
    let app = app_multi_verifier(&primary, &secondary);
    let assertion = gateway_assertion(&secondary, "gateway-caller@example.com");
    assert_eq!(
        status_of(&app, ("POST", "/v1/query"), None, Some(&assertion), None).await,
        StatusCode::OK,
        "a gateway caller's assertion in X-Transit-Proof is admitted when Authorization is absent"
    );
}

#[tokio::test]
async fn a_token_for_the_secondary_issuer_in_the_wrong_header_is_refused() {
    // If the primary header (`Authorization`) carries a token whose `iss` matches the secondary
    // verifier, the gate selects the secondary and tries to read from its configured header
    // (`X-Transit-Proof`). If that header is absent, the request is refused — the token cannot
    // be verified because it was not presented in the right place.
    let primary = an_issuer();
    let secondary =
        MockIssuer::generating(GATEWAY_ISSUER, GATEWAY_AUDIENCE, "gw-key").expect("the secondary issuer generates a key pair");
    let app = app_multi_verifier(&primary, &secondary);
    // A gateway assertion presented in Authorization (which is the primary's header, not the
    // secondary's). The gate sees iss=GATEWAY_ISSUER, selects the secondary verifier, then tries
    // to read from X-Transit-Proof which is absent → refused.
    let misrouted = gateway_assertion(&secondary, "gateway-caller@example.com");
    assert_eq!(
        status_of(&app, ("POST", "/v1/query"), Some(&misrouted), None, None).await,
        StatusCode::UNAUTHORIZED,
        "a secondary-issuer token presented in the primary header is refused when the secondary header is absent"
    );
}

#[tokio::test]
async fn both_headers_present_with_tokens_for_distinct_issuers_routes_by_the_primary_issuer() {
    // If both headers are present, `iss` routing selects the verifier for whoever sent the
    // primary header token. The secondary header token for a different issuer does not interfere
    // as long as the primary header's `iss` resolves to a configured verifier.
    let primary = an_issuer();
    let secondary =
        MockIssuer::generating(GATEWAY_ISSUER, GATEWAY_AUDIENCE, "gw-key").expect("the secondary issuer generates a key pair");
    let app = app_multi_verifier(&primary, &secondary);
    let direct_token = assertion_from(&primary, "direct-caller@example.com");
    let gateway_assertion = gateway_assertion(&secondary, "gateway-caller@example.com");
    // Both Authorization (direct token) and X-Transit-Proof (gateway assertion) are present.
    // The gate reads Authorization first, finds iss=primary, selects the primary verifier, admits.
    assert_eq!(
        status_of(
            &app,
            ("POST", "/v1/query"),
            Some(&direct_token),
            Some(&gateway_assertion),
            None
        )
        .await,
        StatusCode::OK,
        "when both headers carry valid tokens, the primary header's issuer routes the request"
    );
}
