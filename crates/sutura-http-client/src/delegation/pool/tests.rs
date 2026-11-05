//! The pool exchange against a loopback stand-in for Google's token service: the form it sends, and
//! a refusal for each answer shape it must not present.

use sutura_domain::identity::{Delegated, PoolExchange as _, PoolExchangeFailed, PoolScope, Secret, WorkloadPool};

use super::GoogleSts;
use crate::ReadBounds;
use crate::delegation::TokenEndpoint;
use crate::test_support::{CapturedRequest, FakeServer, Scripted};

const AUDIENCE: &str = "//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/analysts/providers/sso";
const ASSERTION: &str = "the.callers.assertion";
const ISSUED: &str = "ya29.an-opaque-access-token";
const ACCESS_TOKEN: &str = "urn:ietf:params:oauth:token-type:access_token";

fn pool() -> WorkloadPool {
    WorkloadPool::parse(AUDIENCE).expect("a provider resource is usable")
}

fn answer(issued_token_type: &str, token_type: &str, expires_in: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "access_token": ISSUED,
        "issued_token_type": issued_token_type,
        "token_type": token_type,
        "expires_in": expires_in,
    })
}

/// What one exchange answered, beside the one request the stand-in received.
type Exchanged = (Result<Delegated, PoolExchangeFailed>, CapturedRequest);

/// One exchange asked for [`PoolScope::Query`] against a stand-in that answers `scripted`, and the
/// one request it received.
fn exchanged(scripted: Scripted, cap: u64) -> Exchanged {
    let server = FakeServer::start(vec![scripted]);
    let bounds = ReadBounds::parse(5, cap).expect("test bounds are nonzero");
    let endpoint = TokenEndpoint::parse(&format!("{}/v1/token", server.endpoint())).expect("a loopback endpoint parses");
    let outcome =
        GoogleSts::new(endpoint, crate::fixed(bounds, None), bounds).exchange(&Secret::new(ASSERTION), &pool(), PoolScope::Query);
    let mut requests = server.finish().into_iter();
    let (Some(request), None) = (requests.next(), requests.next()) else {
        panic!("expected exactly one request");
    };
    (outcome, request)
}

/// One form field, percent-decoded.
fn decoded(raw: &str) -> String {
    let spaced = raw.replace('+', " ");
    let mut escapes = spaced.split('%');
    let mut bytes = escapes.next().unwrap_or_default().as_bytes().to_vec();
    for escape in escapes {
        let hex = escape.get(..2).expect("two hex digits follow a percent");
        bytes.push(u8::from_str_radix(hex, 16).expect("a percent escape"));
        bytes.extend_from_slice(escape.get(2..).unwrap_or_default().as_bytes());
    }
    String::from_utf8(bytes).expect("a UTF-8 field")
}

#[test]
#[expect(clippy::disallowed_methods, reason = "the cell asserts on the token the exchange returned")]
fn the_exchange_asks_for_an_access_token_at_the_pool_for_the_callers_assertion_and_the_bigquery_scope() {
    let (outcome, request) = exchanged(Scripted::ok(&answer(ACCESS_TOKEN, "Bearer", &3599.into())), 64 * 1024);
    let delegated = outcome.expect("a well-formed answer is an access token");
    assert!(delegated.not_after_unix_seconds() > 3599, "the expiry counts from now");
    assert_eq!(delegated.into_token().expose_secret(), ISSUED);
    assert_eq!(request.request_line(), "POST /v1/token HTTP/1.1");
    assert_eq!(
        request.authorization(),
        None,
        "the provider authenticates no client without a declared one"
    );
    let mut sent: Vec<(String, String)> = request
        .body()
        .split('&')
        .map(|pair| {
            let (name, value) = pair.split_once('=').expect("a name=value pair");
            (decoded(name), decoded(value))
        })
        .collect();
    sent.sort();
    let mut wanted: Vec<(String, String)> = [
        ("audience", AUDIENCE),
        ("grant_type", "urn:ietf:params:oauth:grant-type:token-exchange"),
        ("requested_token_type", ACCESS_TOKEN),
        ("scope", "https://www.googleapis.com/auth/bigquery"),
        ("subject_token", ASSERTION),
        ("subject_token_type", "urn:ietf:params:oauth:token-type:jwt"),
    ]
    .iter()
    .map(|&(name, value)| (String::from(name), String::from(value)))
    .collect();
    wanted.sort();
    assert_eq!(sent, wanted);
}

#[test]
fn an_answer_of_another_token_type_or_not_bearer_is_refused() {
    for (issued_token_type, token_type) in [
        ("urn:ietf:params:oauth:token-type:id_token", "Bearer"),
        ("urn:ietf:params:oauth:token-type:jwt", "Bearer"),
        (ACCESS_TOKEN, "DPoP"),
        (ACCESS_TOKEN, "N_A"),
    ] {
        let (outcome, ..) = exchanged(Scripted::ok(&answer(issued_token_type, token_type, &3599.into())), 64 * 1024);
        assert!(
            matches!(outcome, Err(PoolExchangeFailed::WrongTokenType)),
            "{issued_token_type} / {token_type}: {outcome:?}"
        );
    }
    let mut untyped = answer(ACCESS_TOKEN, "Bearer", &3599.into());
    untyped.as_object_mut().expect("an object").remove("token_type");
    let (outcome, ..) = exchanged(Scripted::ok(&untyped), 64 * 1024);
    assert!(matches!(outcome, Err(PoolExchangeFailed::WrongTokenType)), "{outcome:?}");
}

#[test]
fn an_answer_without_a_positive_integer_lifetime_is_refused() {
    for lifetime in [
        serde_json::Value::Null,
        0.into(),
        (-1).into(),
        serde_json::json!(3599.5),
        serde_json::json!("3599"),
    ] {
        let (outcome, ..) = exchanged(Scripted::ok(&answer(ACCESS_TOKEN, "Bearer", &lifetime)), 64 * 1024);
        assert!(
            matches!(
                outcome,
                Err(PoolExchangeFailed::Malformed {
                    what: "no positive integer expires_in"
                })
            ),
            "{lifetime}: {outcome:?}"
        );
    }
}

#[test]
fn an_answer_with_an_empty_or_absent_token_is_refused() {
    for token in [serde_json::json!(""), serde_json::Value::Null] {
        let mut empty = answer(ACCESS_TOKEN, "Bearer", &3599.into());
        empty["access_token"] = token;
        let (outcome, ..) = exchanged(Scripted::ok(&empty), 64 * 1024);
        assert!(
            matches!(outcome, Err(PoolExchangeFailed::Malformed { what: "no access_token" })),
            "{outcome:?}"
        );
    }
}

#[test]
fn an_answer_over_the_cap_is_too_large() {
    let (outcome, ..) = exchanged(Scripted::ok(&answer(ACCESS_TOKEN, "Bearer", &3599.into())), 16);
    assert!(
        matches!(outcome, Err(PoolExchangeFailed::TooLarge { cap: 16 })),
        "{outcome:?}"
    );
}

#[test]
fn a_refusal_keeps_the_registered_error_code_and_never_the_assertion() {
    let echoing = format!(r#"{{"error":"invalid_grant","error_description":"rejected {ASSERTION}"}}"#);
    let (outcome, ..) = exchanged(Scripted::status(400, &echoing), 64 * 1024);
    let refused = outcome.expect_err("a 400 is a refusal");
    assert!(
        matches!(refused, PoolExchangeFailed::Refused { status: 400, error: Some(ref code) } if code == "invalid_grant"),
        "{refused:?}"
    );
    let rendered = format!("{refused} / {refused:?}");
    assert!(!rendered.contains(ASSERTION), "{rendered}");
    let (outcome, ..) = exchanged(Scripted::status(401, &format!(r#"{{"error":"{ASSERTION}"}}"#)), 64 * 1024);
    assert!(
        matches!(
            outcome,
            Err(PoolExchangeFailed::Refused {
                status: 401,
                error: None
            })
        ),
        "{outcome:?}"
    );
}
