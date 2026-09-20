//! The loopback subject-token source, asserted against a real socket rather than a comment.
//!
//! **Every cell here dials the endpoint the driver would dial.** There is no fake: the thing under
//! test IS a `TcpListener` and a hand-written response, so a fake would be testing the fake.

use std::io::{Read as _, Write as _};
use std::net::TcpStream;

use sutura_domain::identity::Secret;

use super::{SECRET_HEADER, SubjectSource, UnusablePool, WorkloadPool};

/// The pool every cell below federates against.
fn pool() -> WorkloadPool {
    WorkloadPool::parse(
        "//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/analysts/providers/sso",
        "https://www.googleapis.com/auth/cloud-platform",
    )
    .expect("a provider resource and a scope are usable")
}

/// The document one source hands the driver, as JSON.
#[expect(
    clippy::disallowed_methods,
    reason = "a test asserting what the driver is handed needs the document's text; there is \
              nothing else to compare it against, and the production exposure is named separately \
              at `identity::credential_options`"
)]
fn document(source: &SubjectSource) -> serde_json::Value {
    let text = String::from(source.document(&pool()).expose_secret());
    serde_json::from_str(&text).expect("the credential document is JSON")
}

/// What one fetch of the loopback source came back as.
///
/// **An enum rather than a `Result` whose error half is a bare string**, which `check-boundaries`
/// refuses in a library for the reason it states: the error type IS the API, and a string there is
/// an untyped one - even in a suite, because the gate reads the crate rather than the intent. It
/// found this file's first draft, which is the gate working.
#[derive(Debug, PartialEq, Eq)]
enum Answered {
    /// A `200`, with the body - which for `format: text` is the subject token whole.
    Served(String),
    /// Anything else, with the status line, so a cell can assert WHICH refusal.
    Refused(String),
}

/// `GET`s the loopback source the way the driver's own provider does.
fn fetch(url: &str, header: Option<(&str, &str)>) -> Answered {
    use std::fmt::Write as _;

    let rest = url.strip_prefix("http://").unwrap_or(url);
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let Ok(mut stream) = TcpStream::connect(authority) else {
        return Answered::Refused(String::from("the loopback source is not listening"));
    };
    let mut request = format!("GET /{path} HTTP/1.1\r\nhost: {authority}\r\n");
    if let Some((name, value)) = header
        && write!(request, "{name}: {value}\r\n").is_err()
    {
        return Answered::Refused(String::from("the header could not be written"));
    }
    request.push_str("connection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return Answered::Refused(String::from("the request could not be written"));
    }
    let mut answered = String::new();
    drop(stream.read_to_string(&mut answered));
    let (head, body) = answered.split_once("\r\n\r\n").unwrap_or((answered.as_str(), ""));
    let status = head.lines().next().unwrap_or_default();
    if status.contains("200") {
        Answered::Served(String::from(body))
    } else {
        Answered::Refused(String::from(status))
    }
}

/// The URL and the header secret the document names, for a cell that has to present both.
fn credentials(document: &serde_json::Value) -> (String, String) {
    let source = &document["credential_source"];
    (
        String::from(source["url"].as_str().expect("the source names a URL")),
        String::from(source["headers"][SECRET_HEADER].as_str().expect("the source names a secret")),
    )
}

#[test]
fn the_document_names_the_pool_and_a_loopback_source_the_driver_can_read() {
    // **The shape read off the pinned Google source rather than invented**: `external_account` with
    // an `audience`, a JWT `subject_token_type`, Google's STS `token_url` and a `credential_source`
    // the library dispatches to its URL provider. `service_account_impersonation_url` is absent on
    // purpose - the credential IS the pool principal the subject resolved to, which is what makes
    // two subjects two principals without a declared per-subject map.
    let held = SubjectSource::bind(&Secret::new("an.assertion.value")).expect("loopback is bindable");
    let document = document(&held);
    assert_eq!(document["type"], "external_account");
    assert_eq!(document["audience"], pool().audience());
    assert_eq!(document["subject_token_type"], "urn:ietf:params:oauth:token-type:jwt");
    assert_eq!(document["token_url"], "https://sts.googleapis.com/v1/token");
    assert_eq!(document["credential_source"]["format"]["type"], "text");
    assert!(
        document["service_account_impersonation_url"].is_null(),
        "an impersonation URL would replace the subject's own principal with a declared account: {document}"
    );
    let url = document["credential_source"]["url"].as_str().expect("a URL");
    assert!(url.starts_with("http://127.0.0.1:"), "{url}");
}

#[test]
fn a_caller_presenting_both_values_is_served_the_assertion_and_nothing_else() {
    // The happy path, and it is the whole contract: `format: text` means the library takes the
    // WHOLE body as the subject token, so a byte of anything else would reach Google's STS as part
    // of the assertion.
    let held = SubjectSource::bind(&Secret::new("an.assertion.value")).expect("loopback is bindable");
    let (url, secret) = credentials(&document(&held));
    assert_eq!(
        fetch(&url, Some((SECRET_HEADER, &secret))),
        Answered::Served(String::from("an.assertion.value"))
    );
}

#[test]
fn neither_value_alone_is_enough_and_every_refusal_reads_the_same() {
    // **BOTH, and this is the cell the open port's bound rests on.** The listener is on loopback
    // with a kernel-assigned port, so any local process can connect; what it cannot do is present
    // two independent 128-bit values it has never seen. A refusal that distinguished the two halves
    // would tell a caller holding one which to keep guessing, so every failure is the same `404`.
    let held = SubjectSource::bind(&Secret::new("an.assertion.value")).expect("loopback is bindable");
    let (url, secret) = credentials(&document(&held));
    let wrong_path = url
        .rsplit_once('/')
        .map(|(base, _)| format!("{base}/0123456789abcdef0123456789abcdef"))
        .expect("a path");
    for (case, attempt) in [
        ("the nonce alone", fetch(&url, None)),
        (
            "a wrong secret",
            fetch(&url, Some((SECRET_HEADER, "0123456789abcdef0123456789abcdef"))),
        ),
        ("the secret alone", fetch(&wrong_path, Some((SECRET_HEADER, &secret)))),
        ("neither", fetch(&wrong_path, None)),
    ] {
        let Answered::Refused(refused) = attempt else {
            panic!("{case} was served the assertion");
        };
        assert!(refused.contains("404"), "{case}: {refused}");
    }
}

#[test]
fn a_fetch_after_the_connection_was_built_is_still_served() {
    // **THE LIFETIME COUPLING, and it fails mid-query rather than at boot.**
    // `externalaccount::NewTokenProvider` wraps its provider in `auth.NewCachedTokenProvider`, so
    // the driver's fetch is LAZY - it happens after `connect` has returned - and can recur when the
    // credential expires during a long query. A source dropped at the end of `connect` would pass
    // every boot-shaped test and fail the first real question.
    //
    // Two fetches, because *served once* and *served for the job's lifetime* are different
    // contracts and only the second is the driver's.
    let held = SubjectSource::bind(&Secret::new("an.assertion.value")).expect("loopback is bindable");
    let (url, secret) = credentials(&document(&held));
    assert_eq!(
        fetch(&url, Some((SECRET_HEADER, &secret))),
        Answered::Served(String::from("an.assertion.value"))
    );
    assert_eq!(
        fetch(&url, Some((SECRET_HEADER, &secret))),
        Answered::Served(String::from("an.assertion.value")),
        "a second fetch is what a cached provider does when the credential expires mid-query"
    );
}

#[test]
fn the_listener_is_gone_once_the_source_is_dropped() {
    // The other direction of the same coupling: an endpoint serving a subject's assertion after the
    // question it belonged to was answered is an authenticated leak with no owner.
    //
    // **What this cell holds, and what it does not - measured rather than assumed.** It holds that
    // a dropped source STOPS SERVING: replacing `Drop`'s `serving.join()` with a discard leaves
    // this green, so the JOIN is held by `Drop`'s own code and by no cell. What the join buys is
    // that there is no WINDOW - the port is closed before `drop` returns rather than shortly after
    // - and a window is a race, so a deterministic cell for it does not exist. The stop signal is
    // what is asserted here; the absence of a window is read off `Drop`.
    let (url, secret) = {
        let held = SubjectSource::bind(&Secret::new("an.assertion.value")).expect("loopback is bindable");
        let pair = credentials(&document(&held));
        assert_eq!(
            fetch(&pair.0, Some((SECRET_HEADER, &pair.1))),
            Answered::Served(String::from("an.assertion.value")),
            "it served while held"
        );
        pair
    };
    let authority = url
        .strip_prefix("http://")
        .and_then(|rest| rest.split_once('/'))
        .map(|(authority, _)| String::from(authority))
        .expect("the URL carries an authority");
    assert!(
        TcpStream::connect(&authority).is_err(),
        "the subject-token endpoint outlived the source that owned it, with {secret} still valid"
    );
}

#[test]
fn two_requests_never_share_a_nonce_or_a_secret() {
    // One subject's values must not open another subject's source. Both are drawn independently
    // from the operating system, so this is a cell on *neither is derived from the other* as much as
    // on freshness - a shared seed would make one predict the other.
    let first = SubjectSource::bind(&Secret::new("a")).expect("loopback is bindable");
    let second = SubjectSource::bind(&Secret::new("b")).expect("loopback is bindable");
    let (first_url, first_secret) = credentials(&document(&first));
    let (second_url, second_secret) = credentials(&document(&second));
    assert_ne!(first_url, second_url);
    assert_ne!(first_secret, second_secret);
    assert!(
        matches!(
            fetch(&second_url, Some((SECRET_HEADER, &first_secret))),
            Answered::Refused(..)
        ),
        "one subject's secret opened another subject's source"
    );
}

#[test]
fn the_credential_document_is_redacted_under_debug() {
    // **The property the open port's bound rests on**, made mechanical: the document carries both
    // values, so a `tracing` line or a `{:?}` that printed it would hand any local process the
    // assertion. `Secret` has no `Display` and prints `REDACTED` under `Debug`, so neither is
    // reachable by accident.
    let held = SubjectSource::bind(&Secret::new("an.assertion.value")).expect("loopback is bindable");
    let rendered = format!("{:?}", held.document(&pool()));
    assert!(rendered.contains("REDACTED"), "{rendered}");
    for leaked in ["an.assertion.value", "127.0.0.1", "external_account"] {
        assert!(!rendered.contains(leaked), "a debug rendering carried `{leaked}`: {rendered}");
    }
}

#[test]
fn a_declared_pool_value_that_could_reshape_the_document_is_refused_at_composition() {
    // Both values land in a JSON document and a URL, so both are screened - and the screening is at
    // COMPOSITION, so an unusable declaration fails to start rather than failing every question.
    for hostile in ["has space", "has\"quote", "has{brace}", ""] {
        assert!(
            WorkloadPool::parse(hostile, "https://www.googleapis.com/auth/cloud-platform").is_err(),
            "`{hostile}` is not an audience this transport may send"
        );
        assert!(
            WorkloadPool::parse("//iam.googleapis.com/p/1", hostile).is_err(),
            "`{hostile}` is not a scope this transport may send"
        );
    }
    assert!(matches!(
        WorkloadPool::parse("", "https://example.com/auth"),
        Err(UnusablePool::Empty { .. })
    ));
}
