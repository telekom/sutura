//! The `behind-gateway` mode with the assertion in `Authorization: Bearer`, through the assembled
//! router. A child of `super` for `super`'s `max-lines` reason, so it reuses that harness as it is.

use axum::http::{HeaderValue, StatusCode, header::AUTHORIZATION};

use crate::testing::{A_QUESTION, accepted_by, an_issuer, asked, request};

use super::{METADATA_PATH, app, assertion_from, call, gateway_overlay, status_of};

#[tokio::test]
async fn a_gateway_assertion_in_authorization_bearer_reaches_the_versioned_surface() {
    // A component that replaced the caller's token puts its assertion in `Authorization: Bearer`.
    let issuer = an_issuer();
    let declared = gateway_overlay(&issuer).replacen("X-Transit-Proof", "Authorization", 1);
    let app = app(&declared, Some(&issuer.key_set()));
    let assertion = assertion_from(&issuer, "someone@example.com");
    assert_eq!(
        call(&app, Some(&assertion)).await.status,
        StatusCode::OK,
        "a verified assertion is answered"
    );

    // Refused with nothing to follow: the component's issuer is not one a client can use.
    let refused = call(&app, None).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    assert_eq!(refused.challenge, None, "a gateway deployment issues no Bearer challenge");
    assert_eq!(
        asked(&app, "GET", METADATA_PATH, None).await.status,
        StatusCode::NOT_FOUND,
        "a gateway deployment publishes no protected-resource metadata"
    );

    // Without its scheme, with two spaces after it, or in another header, the assertion is not read.
    assert_eq!(
        authorized_as(&app, &assertion).await,
        StatusCode::UNAUTHORIZED,
        "an assertion with no Bearer scheme is refused"
    );
    assert_eq!(
        authorized_as(&app, &format!("Bearer  {assertion}")).await,
        StatusCode::UNAUTHORIZED,
        "an assertion after two spaces is refused"
    );
    assert_eq!(
        status_of(&app, ("POST", "/v1/query"), None, Some(&assertion), None).await,
        StatusCode::UNAUTHORIZED,
        "an assertion in another header is not read"
    );

    // A client that bypasses the component sends its own token in this header, so `aud`, the key set
    // and the lifetime ceiling are what refuse it.
    let unaddressed = issuer
        .mint(&accepted_by("someone@example.com").living_for(60).for_nobody_in_particular())
        .expect("the issuer signs an assertion");
    assert_eq!(
        call(&app, Some(&unaddressed)).await.status,
        StatusCode::UNAUTHORIZED,
        "an assertion with no audience is refused"
    );
    let forged = issuer
        .mint_signed_by_a_stranger(&accepted_by("someone@example.com").living_for(60))
        .expect("a stranger signs an assertion");
    assert_eq!(
        call(&app, Some(&forged)).await.status,
        StatusCode::UNAUTHORIZED,
        "an assertion signed by a key outside the key set is refused"
    );
    let long_lived = issuer
        .mint(&accepted_by("someone@example.com").living_for(3600))
        .expect("the issuer signs an assertion");
    assert_eq!(
        call(&app, Some(&long_lived)).await.status,
        StatusCode::UNAUTHORIZED,
        "an assertion living past the lifetime ceiling is refused"
    );
    let undated = issuer
        .mint(&accepted_by("someone@example.com").living_for(60).stating_no_issued_at())
        .expect("the issuer signs an assertion");
    assert_eq!(
        call(&app, Some(&undated)).await.status,
        StatusCode::UNAUTHORIZED,
        "an assertion with no iat is refused"
    );
}

/// The status of a query whose `Authorization` value is `value`, exactly as written.
async fn authorized_as(app: &axum::Router, value: &str) -> StatusCode {
    let mut query = request("POST", "/v1/query", None, axum::body::Body::from(A_QUESTION));
    let value = HeaderValue::from_str(value).expect("a test value is a header value");
    let _previous = query.headers_mut().insert(AUTHORIZATION, value);
    crate::testing::call(app, query).await.0
}
