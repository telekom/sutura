//! The client credentials grant against a loopback stand-in token endpoint: what HTTP Basic puts on
//! the wire, a request per token, and a refusal for each answer shape it must not present.

use std::net::TcpListener;

use sutura_domain::identity::Secret;

use super::{ClientCredentials, ClientCredentialsFailed, Scope, UnusableScope, answered};
use crate::ReadBounds;
use crate::delegation::{ExchangeClient, TokenEndpoint};
use crate::test_support::{CapturedAuthorizations, CapturedRequest, FakeServer, Scripted};

const CLIENT_ID: &str = "sutura:catalog";
const CLIENT_SECRET: &str = "s3cr+t/value";
const ISSUED: &str = "an-opaque-access-token";

fn answer(token: &str) -> serde_json::Value {
    serde_json::json!({ "access_token": token, "token_type": "Bearer", "expires_in": 3600 })
}

fn granting(endpoint: &str, scope: Option<Scope>, cap: u64) -> ClientCredentials {
    let bounds = ReadBounds::parse(5, cap).expect("test bounds are nonzero");
    ClientCredentials::new(
        TokenEndpoint::parse(&format!("{endpoint}/oauth2/token")).expect("a loopback endpoint parses"),
        ExchangeClient::new(CLIENT_ID, Secret::new(CLIENT_SECRET)).expect("a printable identifier is usable"),
        scope,
        crate::fixed(bounds, None),
        bounds,
    )
}

/// Every token `asks` bearer calls returned against a stand-in answering `scripted`, and every
/// request it received.
fn asked(scripted: Vec<Scripted>, asks: usize) -> (Vec<String>, CapturedAuthorizations) {
    let server = FakeServer::start(scripted);
    let grant = granting(&server.endpoint(), None, 64 * 1024);
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
    let (tokens, requests) = asked(vec![Scripted::ok(&answer(ISSUED))], 1);
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
fn a_declared_scope_is_sent_with_the_grant() {
    let server = FakeServer::start(vec![Scripted::ok(&answer(ISSUED))]);
    let scope = Scope::parse("mcp:read catalog").expect("two printable tokens are a scope");
    let grant = granting(&server.endpoint(), Some(scope), 64 * 1024);
    drop(grant.bearer().expect("a well-formed answer is a token"));
    let requests = server.finish();
    let [request] = requests.as_slice() else {
        panic!("expected exactly one request, got {requests:?}");
    };
    assert_eq!(fields(request), ["grant_type=client_credentials", "scope=mcp%3Aread+catalog"]);
}

#[test]
fn a_scope_outside_the_rfc_token_syntax_is_unusable() {
    for raw in ["", " mcp", "mcp ", "mcp  read", "mcp\tread", "a\"b", "a\\b", "caf\u{e9}"] {
        assert_eq!(Scope::parse(raw), Err(UnusableScope), "{raw:?}");
    }
    assert_ne!(Scope::parse("mcp:read"), Err(UnusableScope));
}

#[test]
fn every_call_asks_for_a_new_token() {
    let (tokens, requests) = asked(vec![Scripted::ok(&answer("first")), Scripted::ok(&answer("second"))], 2);
    assert_eq!(tokens, ["first", "second"]);
    assert_eq!(requests.len(), 2, "{requests:?}");
}

#[test]
fn an_answer_that_is_not_a_bearer_token_is_refused() {
    for token_type in [serde_json::json!("DPoP"), serde_json::json!("N_A"), serde_json::Value::Null] {
        let mut typed = answer(ISSUED);
        typed["token_type"] = token_type;
        let outcome = answered(200, &typed.to_string());
        assert!(
            matches!(outcome, Err(ClientCredentialsFailed::WrongTokenType)),
            "{typed}: {outcome:?}"
        );
    }
    let lower = answered(200, &answer(ISSUED).to_string().replace("Bearer", "bearer"));
    assert!(lower.is_ok(), "the token type is case-insensitive: {lower:?}");
}

#[test]
fn an_answer_without_a_token_is_malformed() {
    for token in [serde_json::json!(""), serde_json::Value::Null, serde_json::json!(7)] {
        let mut empty = answer(ISSUED);
        empty["access_token"] = token;
        let outcome = answered(200, &empty.to_string());
        assert!(
            matches!(outcome, Err(ClientCredentialsFailed::Malformed { what: "no access_token" })),
            "{empty}: {outcome:?}"
        );
    }
    let outcome = answered(200, "<html>");
    assert!(
        matches!(outcome, Err(ClientCredentialsFailed::Malformed { what: "not JSON" })),
        "{outcome:?}"
    );
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
    let grant = granting(&server.endpoint(), None, 64 * 1024);
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
fn a_token_answer_over_the_cap_is_too_large() {
    let server = FakeServer::start(vec![Scripted::ok(&answer(ISSUED))]);
    let outcome = granting(&server.endpoint(), None, 16).bearer();
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
    let outcome = granting(&closed, None, 64 * 1024).bearer();
    assert!(
        matches!(outcome, Err(ClientCredentialsFailed::Unreachable { .. })),
        "{outcome:?}"
    );
}
