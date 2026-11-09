#![forbid(unsafe_code)]
//! The real reader against a loopback fake server: what it sends, and what it refuses.
#![cfg(feature = "http")]

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::{Value, json};
    use sutura_catalog_openapi::http::{Endpoint, HttpAnswerReader, HttpReaderError, ReadBounds};
    use sutura_catalog_openapi::{Contract, OpenApiCatalog, OpenApiError, fixture};
    use sutura_domain::identity::Secret;
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};
    use sutura_http_client::test_support::{CapturedRequest, FakeServer, Scripted};

    /// A canary: a test fails if this text reaches any rendered error.
    const TOKEN: &str = "openapi-token-canary-41d7";

    fn catalog(server: &FakeServer, timeout_seconds: u64, cap: u64) -> OpenApiCatalog<HttpAnswerReader> {
        let reader = HttpAnswerReader::new(
            Endpoint::parse(&server.endpoint()).expect("the fake server's own endpoint is usable"),
            Secret::new(String::from(TOKEN)),
            ReadBounds::parse(timeout_seconds, cap).expect("a positive timeout and cap are usable bounds"),
            None,
        );
        let name = SourceName::parse("registry").expect("a name");
        OpenApiCatalog::new(
            name.clone(),
            DefinitionVersion::parse("test").expect("a version"),
            name,
            Contract::parse(fixture::DOCUMENT, fixture::MAPPING).expect("the example contract checks"),
            reader,
        )
    }

    /// The recorded answers, in the order one load asks for them.
    fn in_order() -> Vec<(&'static str, Value)> {
        let mut recorded: serde_json::Map<String, Value> =
            serde_json::from_str(fixture::ANSWERS).expect("the recorded answers are JSON");
        [
            "/v1/workspaces/demo/datasets?pageSize=2",
            "/v1/workspaces/demo/datasets?pageSize=2&pageToken=page-2",
            "/v1/workspaces/demo/datasets/region/links",
            "/v1/workspaces/demo/datasets/customer/links",
            "/v1/workspaces/demo/datasets/subscription/links",
        ]
        .into_iter()
        .map(|target| (target, recorded.remove(target).expect("the target is recorded")))
        .collect()
    }

    /// Every link of the cause chain, rendered.
    fn chain(error: &OpenApiError) -> String {
        let mut text = error.to_string();
        let mut cause = std::error::Error::source(error);
        while let Some(link) = cause {
            text.push_str(" | ");
            text.push_str(&link.to_string());
            cause = link.source();
        }
        text
    }

    fn read_cause(refused: &OpenApiError) -> &HttpReaderError {
        let OpenApiError::Read { cause, .. } = refused else {
            panic!("a refused read is a read error: {refused:?}")
        };
        cause
            .downcast_ref::<HttpReaderError>()
            .unwrap_or_else(|| panic!("the cause is the reader's: {cause:?}"))
    }

    #[test]
    fn each_page_and_each_models_links_are_asked_for_with_the_deployment_bearer() {
        let answers = in_order();
        let server = FakeServer::start(answers.iter().map(|(_, answer)| Scripted::ok(answer)).collect());
        let pinned = catalog(&server, 10, 1 << 20)
            .load()
            .expect("the recorded answers load over HTTP");
        let requests = server.finish();
        let lines: Vec<&str> = requests.iter().map(CapturedRequest::request_line).collect();
        let expected: Vec<String> = answers.iter().map(|(target, _)| format!("GET {target} HTTP/1.1")).collect();
        assert_eq!(lines, expected);
        for request in &requests {
            assert_eq!(request.authorization(), Some(format!("Bearer {TOKEN}").as_str()));
        }
        assert_eq!(pinned.definitions().models().len(), 3);
    }

    #[test]
    fn a_refusal_is_typed_and_carries_neither_the_token_nor_the_apis_text() {
        let server = FakeServer::start(vec![Scripted::status(
            401,
            &format!(r#"{{"error":"invalid token {TOKEN}"}}"#),
        )]);
        let refused = catalog(&server, 10, 1 << 20).load().expect_err("a 401 is a refusal");
        drop(server.finish());
        assert!(
            matches!(read_cause(&refused), HttpReaderError::Refused { status: 401, .. }),
            "{refused:?}"
        );
        let rendered = chain(&refused);
        assert!(!rendered.contains(TOKEN), "{rendered}");
        assert!(!rendered.contains("invalid token"), "{rendered}");
    }

    #[test]
    fn a_redirect_is_refused_and_never_followed() {
        let server = FakeServer::start(vec![Scripted::status(302, "")]);
        let refused = catalog(&server, 10, 1 << 20).load().expect_err("a redirect is not an answer");
        assert_eq!(server.finish().len(), 1, "exactly one request was sent");
        assert!(matches!(refused, OpenApiError::Read { .. }), "{refused:?}");
    }

    #[test]
    fn an_answer_over_the_cap_is_refused_rather_than_read() {
        let big = json!({ "items": [], "padding": "x".repeat(8192) });
        let server = FakeServer::start(vec![Scripted::ok(&big)]);
        let refused = catalog(&server, 10, 4096)
            .load()
            .expect_err("the answer is larger than 4096 bytes");
        drop(server.finish());
        assert!(
            matches!(read_cause(&refused), HttpReaderError::TooLarge { cap: 4096 }),
            "{refused:?}"
        );
    }

    #[test]
    fn a_final_response_body_past_the_deadline_is_refused() {
        let server = FakeServer::start(vec![Scripted::delayed(
            &json!({ "items": [] }),
            Duration::from_millis(1200),
        )]);
        let refused = catalog(&server, 1, 1 << 20)
            .load()
            .expect_err("the final response body arrives after the one-second deadline");
        assert!(matches!(
            read_cause(&refused),
            HttpReaderError::DeadlineSpent { budget_seconds: 1 }
        ));
        assert_eq!(server.finish().len(), 1, "the delayed response was the only request");
    }

    #[test]
    fn one_deadline_covers_every_request_of_a_load() {
        let answers = in_order();
        let scripted: Vec<Scripted> = answers
            .iter()
            .take(2)
            .map(|(_, answer)| Scripted::delayed(answer, Duration::from_millis(1100)))
            .collect();
        let server = FakeServer::start(scripted);
        let refused = catalog(&server, 2, 1 << 20)
            .load()
            .expect_err("two answers of 1.1 s each spend a 2 s deadline before the third request");
        assert_eq!(server.finish().len(), 2, "the third request was never sent");
        assert!(
            matches!(read_cause(&refused), HttpReaderError::DeadlineSpent { budget_seconds: 2 }),
            "{refused:?}"
        );
    }
}
