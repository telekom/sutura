#![forbid(unsafe_code)]
//! The real reader against a loopback fake server: what it sends, and what it refuses.
#![cfg(feature = "http")]

#[cfg(test)]
mod tests {
    use sutura_catalog_cube::http::{Endpoint, HttpMetaReader, HttpReaderError, META_PATH, ReadBounds};
    use sutura_catalog_cube::{CubeCatalog, CubeError, fixture};
    use sutura_domain::identity::Secret;
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};
    use sutura_http_client::test_support::{FakeServer, Scripted};

    /// A canary: a test fails if this text reaches any rendered error.
    const TOKEN: &str = "cube-token-canary-6c1f";

    fn catalog(server: &FakeServer, cap: u64) -> CubeCatalog<HttpMetaReader> {
        let reader = HttpMetaReader::new(
            Endpoint::parse(&server.endpoint()).expect("the fake server's own endpoint is usable"),
            Secret::new(String::from(TOKEN)),
            ReadBounds::parse(10, cap).expect("a positive timeout and cap are usable bounds"),
            None,
        );
        let name = SourceName::parse("metrics").expect("a name");
        CubeCatalog::new(
            name.clone(),
            DefinitionVersion::parse("test").expect("a version"),
            name,
            reader,
        )
    }

    /// Every link of the cause chain, rendered.
    fn chain(error: &CubeError) -> String {
        let mut text = error.to_string();
        let mut cause = std::error::Error::source(error);
        while let Some(link) = cause {
            text.push_str(" | ");
            text.push_str(&link.to_string());
            cause = link.source();
        }
        text
    }

    #[test]
    fn the_reader_sends_the_deployment_token_as_a_bearer_to_the_metadata_path() {
        let answer: serde_json::Value = serde_json::from_str(fixture::META).expect("the recorded answer is JSON");
        let server = FakeServer::start(vec![Scripted::ok(&answer)]);
        let pinned = catalog(&server, 1 << 20).load().expect("the recorded answer loads over HTTP");
        let requests = server.finish();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].request_line(), format!("GET {META_PATH} HTTP/1.1"));
        assert_eq!(requests[0].authorization(), Some(format!("Bearer {TOKEN}").as_str()));
        assert_eq!(pinned.definitions().models().len(), 5);
    }

    #[test]
    fn a_refusal_is_typed_and_carries_neither_the_token_nor_cubes_text() {
        let server = FakeServer::start(vec![Scripted::status(
            403,
            &format!(r#"{{"error":"Invalid token {TOKEN}"}}"#),
        )]);
        let refused = catalog(&server, 1 << 20).load().expect_err("a 403 is a refusal");
        drop(server.finish());
        let CubeError::Read(cause) = &refused else {
            panic!("a refused read is a read error: {refused:?}")
        };
        assert!(
            matches!(
                cause.downcast_ref::<HttpReaderError>(),
                Some(HttpReaderError::Refused { status: 403, .. })
            ),
            "{cause:?}"
        );
        let rendered = chain(&refused);
        assert!(!rendered.contains(TOKEN), "{rendered}");
        assert!(!rendered.contains("Invalid token"), "{rendered}");
    }

    #[test]
    fn an_answer_over_the_cap_is_refused_rather_than_read() {
        let answer: serde_json::Value = serde_json::from_str(fixture::META).expect("the recorded answer is JSON");
        let server = FakeServer::start(vec![Scripted::ok(&answer)]);
        let refused = catalog(&server, 4096)
            .load()
            .expect_err("the recorded answer is larger than 4096 bytes");
        drop(server.finish());
        let CubeError::Read(cause) = &refused else {
            panic!("an oversized answer is a read error: {refused:?}")
        };
        assert!(
            matches!(
                cause.downcast_ref::<HttpReaderError>(),
                Some(HttpReaderError::TooLarge { cap: 4096 })
            ),
            "{cause:?}"
        );
    }
}
