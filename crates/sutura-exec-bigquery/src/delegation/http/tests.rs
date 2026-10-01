//! The real exchange: the request it sends, against a loopback fake, and every refusal of the
//! answer through the pure [`answered`].

use base64::Engine as _;
use sutura_domain::identity::Secret;
use sutura_http_client::test_support::{FakeServer, Scripted};
use sutura_http_client::{InvalidEndpoint, ReadBounds};

use super::{ExchangeClient, OverHttp, TokenEndpoint, UnusableClientId, answered};
use crate::delegation::{DelegationExchange as _, DelegationFailed, RequestedAudience};

const AUDIENCE: &str = "https://workforce-pool.example.com";
const NOW: u64 = 4_000_000_000;
const SUBJECT_TOKEN: &str = "inbound.subject.token";
const CLIENT_SECRET: &str = "client-secret-never-logged";

fn audience() -> RequestedAudience {
    RequestedAudience::parse(AUDIENCE).expect("a test audience parses")
}

fn jwt(claims: &serde_json::Value) -> String {
    let encode = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    format!(
        "{}.{}.signature",
        encode(br#"{"alg":"RS256","typ":"JWT"}"#),
        encode(claims.to_string().as_bytes())
    )
}

fn issued(token: &str) -> serde_json::Value {
    serde_json::json!({
        "access_token": token,
        "issued_token_type": "urn:ietf:params:oauth:token-type:access_token",
        "token_type": "Bearer",
        "expires_in": 300,
    })
}

fn good_token() -> String {
    jwt(&serde_json::json!({"aud": AUDIENCE, "exp": NOW + 300, "sub": "s"}))
}

fn exchanging_at(endpoint: &str, cap: u64) -> OverHttp {
    let bounds = ReadBounds::parse(5, cap).expect("test bounds are nonzero");
    OverHttp::new(
        TokenEndpoint::parse(endpoint).expect("a loopback endpoint parses"),
        ExchangeClient::new("https://sutura.example.com", Secret::new(CLIENT_SECRET)).expect("a client id parses"),
        sutura_http_client::fixed(bounds, None),
        bounds,
    )
}

/// `application/x-www-form-urlencoded`, decoded - just enough for the fields this exchange sends.
fn form(body: &str) -> Vec<(String, String)> {
    let decode = |raw: &str| {
        let bytes = raw.as_bytes();
        let mut out = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            match bytes[at] {
                b'+' => out.push(b' '),
                b'%' => {
                    let hex = std::str::from_utf8(&bytes[at + 1..at + 3]).expect("an ASCII escape");
                    out.push(u8::from_str_radix(hex, 16).expect("a percent escape"));
                    at += 2;
                }
                other => out.push(other),
            }
            at += 1;
        }
        String::from_utf8(out).expect("a UTF-8 field")
    };
    body.split('&')
        .map(|pair| {
            let (name, value) = pair.split_once('=').expect("a name=value pair");
            (decode(name), decode(value))
        })
        .collect()
}

#[test]
#[expect(clippy::disallowed_methods, reason = "the cell asserts on the token the exchange returned")]
fn the_request_is_an_rfc_8693_exchange_of_the_callers_token_for_the_requested_audience() {
    let token = good_token();
    let server = FakeServer::start(vec![Scripted::ok(&issued(&token))]);
    let endpoint = format!("{}/realms/r/protocol/openid-connect/token", server.endpoint());
    let delegated = exchanging_at(&endpoint, 64 * 1024)
        .exchange(&Secret::new(SUBJECT_TOKEN), &audience())
        .expect("a well-formed answer is a delegated token");
    assert_eq!(delegated.not_after_unix_seconds(), NOW + 300);
    assert_eq!(delegated.into_token().expose_secret(), token);

    let requests = server.finish();
    let [request] = requests.as_slice() else {
        panic!("expected exactly one request, got {requests:?}");
    };
    assert_eq!(
        request.request_line(),
        "POST /realms/r/protocol/openid-connect/token HTTP/1.1"
    );
    let mut sent = form(request.body());
    sent.sort();
    let mut wanted: Vec<(String, String)> = [
        ("audience", AUDIENCE),
        ("client_id", "https://sutura.example.com"),
        ("client_secret", CLIENT_SECRET),
        ("grant_type", "urn:ietf:params:oauth:grant-type:token-exchange"),
        ("requested_token_type", "urn:ietf:params:oauth:token-type:access_token"),
        ("subject_token", SUBJECT_TOKEN),
        ("subject_token_type", "urn:ietf:params:oauth:token-type:access_token"),
    ]
    .iter()
    .map(|&(name, value)| (String::from(name), String::from(value)))
    .collect();
    wanted.sort();
    assert_eq!(sent, wanted);
}

#[test]
fn an_unreachable_idp_is_unreachable_and_names_no_token() {
    let server = FakeServer::start(Vec::new());
    let endpoint = format!("{}/token", server.endpoint());
    drop(server.finish());
    let failed = exchanging_at(&endpoint, 1024)
        .exchange(&Secret::new(SUBJECT_TOKEN), &audience())
        .expect_err("nothing is listening");
    assert!(matches!(failed, DelegationFailed::Unreachable { .. }), "{failed:?}");
    let chain = format!("{failed} / {failed:?}");
    assert!(!chain.contains(SUBJECT_TOKEN) && !chain.contains(CLIENT_SECRET), "{chain}");
}

#[test]
fn an_answer_over_the_cap_is_too_large() {
    let server = FakeServer::start(vec![Scripted::ok(&issued(&good_token()))]);
    let failed = exchanging_at(&format!("{}/token", server.endpoint()), 16)
        .exchange(&Secret::new(SUBJECT_TOKEN), &audience())
        .expect_err("the answer is over a 16-byte cap");
    assert!(matches!(failed, DelegationFailed::TooLarge { cap: 16 }), "{failed:?}");
    drop(server.finish());
}

#[test]
fn a_refusal_keeps_a_registered_error_code_and_drops_everything_else() {
    let refused = |body: &serde_json::Value| answered(400, &body.to_string(), &audience(), NOW).expect_err("a 400 is refused");
    let kept = refused(&serde_json::json!({"error": "invalid_request", "error_description": SUBJECT_TOKEN}));
    assert!(
        matches!(kept, DelegationFailed::Refused { status: 400, error: Some(ref code) } if code == "invalid_request"),
        "{kept:?}"
    );
    assert!(
        !format!("{kept} {kept:?}").contains(SUBJECT_TOKEN),
        "the description was kept: {kept:?}"
    );
    for unregistered in ["Invalid.Request", "a".repeat(65).as_str()] {
        let dropped = refused(&serde_json::json!({ "error": unregistered }));
        assert!(
            matches!(dropped, DelegationFailed::Refused { error: None, .. }),
            "an unregistered code survived: {dropped:?}"
        );
    }
    let echoed = refused(&serde_json::json!({"error": good_token()}));
    assert!(
        matches!(echoed, DelegationFailed::Refused { error: None, .. }),
        "a JWT-shaped code survived: {echoed:?}"
    );
    assert!(matches!(
        answered(502, "<html>bad gateway</html>", &audience(), NOW),
        Err(DelegationFailed::Refused {
            status: 502,
            error: None
        })
    ));
}

#[test]
fn an_answer_of_the_wrong_type_is_refused() {
    let mut other = issued(&good_token());
    other["issued_token_type"] = serde_json::json!("urn:ietf:params:oauth:token-type:id_token");
    assert!(matches!(
        answered(200, &other.to_string(), &audience(), NOW),
        Err(DelegationFailed::WrongTokenType)
    ));
    let mut not_bearer = issued(&good_token());
    not_bearer["token_type"] = serde_json::json!("N_A");
    assert!(matches!(
        answered(200, &not_bearer.to_string(), &audience(), NOW),
        Err(DelegationFailed::WrongTokenType)
    ));
}

#[test]
fn a_malformed_answer_is_refused_by_what_it_lacks() {
    let malformed = |text: &str| match answered(200, text, &audience(), NOW) {
        Err(DelegationFailed::Malformed { what }) => what,
        other => panic!("expected a malformed refusal, got {other:?}"),
    };
    assert_eq!(malformed("not json"), "not JSON");
    let mut no_token = issued("x");
    drop(no_token.as_object_mut().expect("an object").remove("access_token"));
    assert_eq!(malformed(&no_token.to_string()), "no access_token");
    assert_eq!(malformed(&issued("opaque-reference-token").to_string()), "not a compact JWT");
    let no_exp = jwt(&serde_json::json!({"aud": AUDIENCE}));
    assert_eq!(malformed(&issued(&no_exp).to_string()), "no numeric exp");
}

#[test]
fn a_token_without_the_requested_audience_is_refused() {
    let elsewhere = jwt(&serde_json::json!({"aud": ["https://other.example.com", "account"], "exp": NOW + 300}));
    assert!(matches!(
        answered(200, &issued(&elsewhere).to_string(), &audience(), NOW),
        Err(DelegationFailed::WrongAudience)
    ));
    let none = jwt(&serde_json::json!({"exp": NOW + 300}));
    assert!(matches!(
        answered(200, &issued(&none).to_string(), &audience(), NOW),
        Err(DelegationFailed::WrongAudience)
    ));
    let among = jwt(&serde_json::json!({"aud": ["account", AUDIENCE], "exp": NOW + 300}));
    drop(answered(200, &issued(&among).to_string(), &audience(), NOW).expect("an `aud` array carrying the audience"));
}

#[test]
fn a_token_expiring_at_or_before_the_check_is_refused() {
    let at = jwt(&serde_json::json!({"aud": AUDIENCE, "exp": NOW}));
    assert!(matches!(
        answered(200, &issued(&at).to_string(), &audience(), NOW),
        Err(DelegationFailed::AlreadyExpired)
    ));
    let after = jwt(&serde_json::json!({"aud": AUDIENCE, "exp": NOW + 1}));
    drop(answered(200, &issued(&after).to_string(), &audience(), NOW).expect("one second of life left is usable"));
}

#[test]
fn a_token_endpoint_keeps_its_path_and_refuses_what_could_leak_the_secret() {
    assert_eq!(
        TokenEndpoint::parse("https://idp.example.com/realms/r/protocol/openid-connect/token")
            .expect("an https endpoint parses")
            .as_str(),
        "https://idp.example.com/realms/r/protocol/openid-connect/token"
    );
    assert!(matches!(
        TokenEndpoint::parse("http://idp.example.com/token"),
        Err(InvalidEndpoint::PlaintextBeyondLoopback { .. })
    ));
    assert!(matches!(
        TokenEndpoint::parse("https://user:pass@idp.example.com/token"),
        Err(InvalidEndpoint::CredentialsInUrl { .. })
    ));
    for beyond in ["https://idp.example.com/token?x=1", "https://idp.example.com/token#f"] {
        assert!(
            matches!(TokenEndpoint::parse(beyond), Err(InvalidEndpoint::PathBeyondRoot { .. })),
            "{beyond}"
        );
    }
    assert!(matches!(
        TokenEndpoint::parse("ftp://idp.example.com/token"),
        Err(InvalidEndpoint::NotAnHttpUrl { .. })
    ));
}

#[test]
fn a_client_id_no_form_can_carry_is_refused() {
    for unusable in ["", "has space", "tab\t", &"c".repeat(256)] {
        assert_eq!(
            ExchangeClient::new(unusable, Secret::new("s")).map(|_| ()),
            Err(UnusableClientId::Unusable),
            "{unusable:?}"
        );
    }
    let client = ExchangeClient::new("https://sutura.example.com", Secret::new(CLIENT_SECRET)).expect("parses");
    assert!(!format!("{client:?}").contains(CLIENT_SECRET), "{client:?}");
}
