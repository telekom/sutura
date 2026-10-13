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

    // Without its scheme, or in another header, the assertion is not read.
    let mut bare = request("POST", "/v1/query", None, axum::body::Body::from(A_QUESTION));
    let value = HeaderValue::from_str(&assertion).expect("a test assertion is a header value");
    let _previous = bare.headers_mut().insert(AUTHORIZATION, value);
    assert_eq!(
        crate::testing::call(&app, bare).await.0,
        StatusCode::UNAUTHORIZED,
        "an assertion with no Bearer scheme is refused"
    );
    assert_eq!(
        status_of(&app, ("POST", "/v1/query"), None, Some(&assertion), None).await,
        StatusCode::UNAUTHORIZED,
        "an assertion in another header is not read"
    );

    // A client that bypasses the component sends its own token in this header, so `aud` and the key
    // set are what refuse it.
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
}
