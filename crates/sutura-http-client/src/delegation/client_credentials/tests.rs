//! The client credentials grant against a loopback stand-in token endpoint: what each client
//! authentication puts on the wire, how long a token is held, and a refusal for each answer shape it
//! must not present.

use std::net::TcpListener;

use sutura_domain::identity::Secret;

use super::{ClientAuth, ClientCredentials, ClientCredentialsFailed, answered};
use crate::ReadBounds;
use crate::delegation::{ExchangeClient, TokenEndpoint};
use crate::test_support::{CapturedAuthorizations, CapturedRequest, FakeServer, Scripted};

const CLIENT_ID: &str = "sutura:catalog";
const CLIENT_SECRET: &str = "s3cr+t/value";
const ISSUED: &str = "an-opaque-access-token";

fn answer(token: &str, expires_in: u64) -> serde_json::Value {
    serde_json::json!({ "access_token": token, "token_type": "Bearer", "expires_in": expires_in })
}

fn granting(endpoint: &str, auth: ClientAuth, cap: u64) -> ClientCredentials {
    let bounds = ReadBounds::parse(5, cap).expect("test bounds are nonzero");
    ClientCredentials::new(
        TokenEndpoint::parse(&format!("{endpoint}/oauth2/token")).expect("a loopback endpoint parses"),
        ExchangeClient::new(CLIENT_ID, Secret::new(CLIENT_SECRET)).expect("a printable identifier is usable"),
        auth,
        crate::fixed(bounds, None),
        bounds,
    )
}

/// Every token `asks` bearer calls returned against a stand-in answering `scripted`, and every
/// request it received.
fn asked(scripted: Vec<Scripted>, auth: ClientAuth, asks: usize) -> (Vec<String>, CapturedAuthorizations) {
    let server = FakeServer::start(scripted);
    let grant = granting(&server.endpoint(), auth, 64 * 1024);
    #[expect(clippy::disallowed_methods, reason = "the cell asserts on the tokens the grant returned")]
    let tokens =
        std::iter::repeat_with(|| String::from(grant.bearer().expect("a well-formed answer is a token").expose_secret()))
            .take(asks)
            .collect();
    (tokens, server.finish())
}

/// The form body as sorted `name=value` pairs, still encoded.
fn fields(request: &CapturedRequest) -> Vec<&str> {
    let mut pairs: Vec<&str> = request.body().split('&').collect();
    pairs.sort_unstable();
    pairs
}

#[test]
fn basic_auth_sends_the_form_encoded_client_in_the_header_and_only_the_grant_in_the_body() {
    let (tokens, requests) = asked(vec![Scripted::ok(&answer(ISSUED, 3600))], ClientAuth::Basic, 1);
    assert_eq!(tokens, [ISSUED]);
    let [request] = requests.as_slice() else {
        panic!("expected exactly one request, got {requests:?}");
    };
    assert_eq!(request.request_line(), "POST /oauth2/token HTTP/1.1");
    // base64 of `sutura%3Acatalog:s3cr%2Bt%2Fvalue`, encoded outside this crate.
    assert_eq!(
        request.authorization(),
        Some("Basic c3V0dXJhJTNBY2F0YWxvZzpzM2NyJTJCdCUyRnZhbHVl")
    );
    assert_eq!(fields(request), ["grant_type=client_credentials"]);
}

#[test]
fn post_auth_sends_the_client_in_the_body_and_no_header() {
    let (tokens, requests) = asked(vec![Scripted::ok(&answer(ISSUED, 3600))], ClientAuth::Post, 1);
    assert_eq!(tokens, [ISSUED]);
    let [request] = requests.as_slice() else {
        panic!("expected exactly one request, got {requests:?}");
    };
    assert_eq!(request.authorization(), None);
    assert_eq!(
        fields(request),
        [
            "client_id=sutura%3Acatalog",
            "client_secret=s3cr%2Bt%2Fvalue",
            "grant_type=client_credentials"
        ]
    );
}

#[test]
fn a_held_token_is_presented_again_without_a_request() {
    let (tokens, requests) = asked(vec![Scripted::ok(&answer(ISSUED, 3600))], ClientAuth::Basic, 3);
    assert_eq!(tokens, [ISSUED, ISSUED, ISSUED]);
    assert_eq!(requests.len(), 1, "{requests:?}");
}

#[test]
fn a_token_within_its_last_minute_is_asked_for_again() {
    let (tokens, requests) = asked(
        vec![Scripted::ok(&answer("first", 60)), Scripted::ok(&answer("second", 3600))],
        ClientAuth::Post,
        3,
    );
    assert_eq!(tokens, ["first", "second", "second"]);
    assert_eq!(requests.len(), 2, "{requests:?}");
}

#[test]
fn an_answer_that_is_not_a_bearer_token_is_refused() {
    for token_type in [serde_json::json!("DPoP"), serde_json::json!("N_A"), serde_json::Value::Null] {
        let mut typed = answer(ISSUED, 3600);
        typed["token_type"] = token_type;
        let outcome = answered(200, &typed.to_string(), 0);
        assert!(
            matches!(outcome, Err(ClientCredentialsFailed::WrongTokenType)),
            "{typed}: {outcome:?}"
        );
    }
    let lower = answered(200, &answer(ISSUED, 3600).to_string().replace("Bearer", "bearer"), 0);
    assert!(lower.is_ok(), "the token type is case-insensitive: {lower:?}");
}

#[test]
fn an_answer_without_a_token_or_a_positive_integer_lifetime_is_malformed() {
    for token in [serde_json::json!(""), serde_json::Value::Null, serde_json::json!(7)] {
        let mut empty = answer(ISSUED, 3600);
        empty["access_token"] = token;
        let outcome = answered(200, &empty.to_string(), 0);
        assert!(
            matches!(outcome, Err(ClientCredentialsFailed::Malformed { what: "no access_token" })),
            "{empty}: {outcome:?}"
        );
    }
    for lifetime in [
        serde_json::Value::Null,
        0.into(),
        (-1).into(),
        serde_json::json!(3599.5),
        serde_json::json!("3599"),
    ] {
        let mut timed = answer(ISSUED, 3600);
        timed["expires_in"] = lifetime;
        let outcome = answered(200, &timed.to_string(), 0);
        assert!(
            matches!(
                outcome,
                Err(ClientCredentialsFailed::Malformed {
                    what: "no positive integer expires_in"
                })
            ),
            "{timed}: {outcome:?}"
        );
    }
    let outcome = answered(200, "<html>", 0);
    assert!(
        matches!(outcome, Err(ClientCredentialsFailed::Malformed { what: "not JSON" })),
        "{outcome:?}"
    );
}

#[test]
fn the_lifetime_counts_from_the_instant_given() {
    let held = answered(200, &answer(ISSUED, 3600).to_string(), 1_000).expect("a well-formed answer");
    assert_eq!(held.not_after, 4_600);
}

#[test]
fn a_refusal_keeps_the_registered_error_code_and_never_the_secret() {
    let server = FakeServer::start(vec![
        Scripted::status(
            401,
            &format!(r#"{{"error":"invalid_client","error_description":"bad secret {CLIENT_SECRET}"}}"#),
        ),
        Scripted::status(400, &format!(r#"{{"error":"{CLIENT_SECRET}"}}"#)),
    ]);
    let grant = granting(&server.endpoint(), ClientAuth::Basic, 64 * 1024);
    let refused = grant.bearer().expect_err("a 401 is a refusal");
    assert!(
        matches!(refused, ClientCredentialsFailed::Refused { status: 401, error: Some(ref code) } if code == "invalid_client"),
        "{refused:?}"
    );
    let rendered = format!("{refused} / {refused:?} / {grant:?}");
    assert!(!rendered.contains(CLIENT_SECRET), "{rendered}");
    let unregistered = grant.bearer().expect_err("a 400 is a refusal");
    assert!(
        matches!(
            unregistered,
            ClientCredentialsFailed::Refused {
                status: 400,
                error: None
            }
        ),
        "{unregistered:?}"
    );
    assert_eq!(server.finish().len(), 2);
}

#[test]
fn a_held_token_is_redacted_under_debug() {
    let server = FakeServer::start(vec![Scripted::ok(&answer(ISSUED, 3600))]);
    let grant = granting(&server.endpoint(), ClientAuth::Post, 64 * 1024);
    drop(grant.bearer().expect("a well-formed answer is a token"));
    let rendered = format!("{grant:?}");
    assert!(!rendered.contains(ISSUED) && !rendered.contains(CLIENT_SECRET), "{rendered}");
    assert_eq!(server.finish().len(), 1);
}

#[test]
fn a_token_answer_over_the_cap_is_too_large() {
    let server = FakeServer::start(vec![Scripted::ok(&answer(ISSUED, 3600))]);
    let outcome = granting(&server.endpoint(), ClientAuth::Basic, 16).bearer();
    assert!(
        matches!(outcome, Err(ClientCredentialsFailed::TooLarge { cap: 16 })),
        "{outcome:?}"
    );
    assert_eq!(server.finish().len(), 1);
}

#[test]
fn an_endpoint_nobody_listens_on_is_unreachable() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
    let closed = format!("http://{}", listener.local_addr().expect("a bound listener has an address"));
    drop(listener);
    let outcome = granting(&closed, ClientAuth::Post, 64 * 1024).bearer();
    assert!(
        matches!(outcome, Err(ClientCredentialsFailed::Unreachable { .. })),
        "{outcome:?}"
    );
}
