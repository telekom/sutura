//! Request logging tests for the assembled HTTP surface.

use super::fixtures::{captured, is_a_correlation_id, json_lines};
use super::*;

#[test]
fn a_request_produces_one_info_span_carrying_the_route_and_a_correlation_id() {
    let app = app(settings(Environment::Development, ""));
    let (status, body, rendered) = captured(
        sutura_config::LogFormat::Bunyan,
        &app,
        request("POST", "/v1/query", None, Body::from(crate::testing::A_QUESTION)),
    );
    assert_eq!(status, StatusCode::OK, "{body}");
    let lines = json_lines(&rendered);
    assert!(!lines.is_empty(), "nothing was logged at all: {rendered}");
    let span_lines: Vec<&serde_json::Value> = lines
        .iter()
        .filter(|line| {
            line["msg"]
                .as_str()
                .is_some_and(|msg| msg.contains("[REQUEST - START]") || msg.contains("[REQUEST - END]"))
        })
        .collect();
    assert!(
        !span_lines.is_empty(),
        "no request span reached the log at the configured level: {rendered}"
    );
    for line in &span_lines {
        assert_eq!(line["level"], 30, "the request span is not at info: {line}");
        assert_eq!(line["route"], "/v1/query", "the span does not name the route: {line}");
        assert_eq!(line["method"], "POST", "the span does not name the method: {line}");
        let correlation = line["correlation"]
            .as_str()
            .unwrap_or_else(|| panic!("the span carries no correlation id: {line}"));
        assert!(
            is_a_correlation_id(correlation),
            "the correlation id is not one this surface would read back: {correlation}"
        );
    }
    for line in &lines {
        assert!(line.get("path").is_none(), "the raw request path reached the log: {line}");
    }

    let (status, _, rendered) = captured(
        sutura_config::LogFormat::Bunyan,
        &app,
        request("GET", "/v1/probing-for-SECRETish-paths", None, Body::empty()),
    );
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        !rendered.contains("probing-for-SECRETish-paths"),
        "a path a caller invented reached the log: {rendered}"
    );
    assert!(
        rendered.contains(r#""route":"unmatched""#),
        "an unmatched request produced no span, or not the constant: {rendered}"
    );
    assert_eq!(crate::router::UNMATCHED_ROUTE, "unmatched");
}

#[test]
fn a_body_that_states_its_own_subject_is_not_a_question() {
    const IMPERSONATED: &str = "victim@example.com";
    let app = app(settings(Environment::Development, ""));
    let claiming = format!(
        r#"{{"metric":"revenue","grain":"month","range":{{"start":"2026-06-01","end":"2026-07-01"}},"subject":"{IMPERSONATED}"}}"#
    );
    let (status, body, rendered) = captured(
        sutura_config::LogFormat::Bunyan,
        &app,
        request("POST", "/v1/query", None, Body::from(claiming)),
    );
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a body naming a subject was accepted: {body}"
    );
    let problem: serde_json::Value = serde_json::from_str(&body).expect("a failure is JSON");
    assert!(
        problem["detail"].as_str().unwrap_or_default().contains("subject"),
        "the refusal does not name the field that was rejected: {body}"
    );
    assert!(
        !rendered.contains(IMPERSONATED),
        "an identifier the caller invented reached the log: {rendered}"
    );

    let (status, body, _) = captured(
        sutura_config::LogFormat::Bunyan,
        &app,
        request("POST", "/v1/query", None, Body::from(crate::testing::A_QUESTION)),
    );
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[test]
fn one_correlation_id_ties_the_span_the_question_and_the_answer_together() {
    let app = app(settings(Environment::Development, ""));
    let mut request = request("POST", "/v1/query", None, Body::from(crate::testing::A_QUESTION));
    request
        .headers_mut()
        .insert(crate::correlation::HEADER, "Ingress-42_abc".parse().expect("a test header"));
    let (status, body, rendered) = captured(sutura_config::LogFormat::Bunyan, &app, request);
    assert_eq!(status, StatusCode::OK, "{body}");
    let lines = json_lines(&rendered);
    for wanted in ["question received", "finished processing request"] {
        let line = lines
            .iter()
            .find(|line| line["msg"].as_str().is_some_and(|msg| msg.contains(wanted)))
            .unwrap_or_else(|| panic!("no `{wanted}` line was written: {rendered}"));
        assert_eq!(
            line["correlation"], "Ingress-42_abc",
            "the `{wanted}` line is not attributable to the request: {line}"
        );
        assert_eq!(line["metric"], "revenue", "{line}");
        assert_eq!(line["grain"], "month", "{line}");
    }
    assert!(
        lines
            .iter()
            .any(|line| line["msg"].as_str().is_some_and(|msg| msg.contains("[REQUEST - END]"))
                && line["correlation"] == "Ingress-42_abc"),
        "the span does not carry the id its events carry: {rendered}"
    );
}

#[test]
fn a_filter_value_never_reaches_the_log() {
    const SENTINEL: &str = "SENTINEL-MUST-NOT-BE-LOGGED";
    let app = app(settings(Environment::Development, ""));
    let question = format!(
        r#"{{"metric":"revenue","grain":"month","range":{{"start":"2026-06-01","end":"2026-07-01"}},"filters":[{{"dimension":"region","value":"{SENTINEL}"}}]}}"#
    );
    for format in [sutura_config::LogFormat::Bunyan, sutura_config::LogFormat::Pretty] {
        let (status, body, rendered) = captured(format, &app, request("POST", "/v1/query", None, Body::from(question.clone())));
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert!(!body.contains(SENTINEL), "the response echoed the value: {body}");
        assert!(
            !rendered.contains(SENTINEL),
            "a filter value reached the {format} log: {rendered}"
        );
        assert!(rendered.contains("finished processing request"), "{rendered}");
    }
}
