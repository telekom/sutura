#![forbid(unsafe_code)]
//! The real reader against a loopback fake server: what it sends, and what it refuses.
#![cfg(feature = "http")]

#[cfg(test)]
mod tests {
    use sutura_catalog_foundry::http::{
        API_ROOT, Endpoint, HttpOntologyReader, HttpReaderError, MAX_PAGE_TOKEN_BYTES, OntologyIdentifier, PagingRefusal,
        ReadBounds, TokenSource,
    };
    use sutura_catalog_foundry::{FoundryCatalog, FoundryError, fixture};
    use sutura_domain::identity::Secret;
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};
    use sutura_http_client::delegation::{ExchangeClient, TokenEndpoint};
    use sutura_http_client::test_support::{FakeServer, Scripted};
    use sutura_http_client::token::{ClientCredentials, Scopes};

    /// A canary: a test fails if this text reaches any rendered error.
    const TOKEN: &str = "foundry-token-canary-3e9a";
    const ONTOLOGY: &str = "ri.ontology.main.ontology.00000000-0000-0000-0000-000000000000";

    fn bounds(cap: u64) -> ReadBounds {
        ReadBounds::parse(10, cap).expect("a positive timeout and cap are usable bounds")
    }

    fn catalog_with(server: &FakeServer, token: TokenSource, cap: u64) -> FoundryCatalog<HttpOntologyReader> {
        let reader = HttpOntologyReader::new(
            Endpoint::parse(&server.endpoint()).expect("the fake server's own endpoint is usable"),
            OntologyIdentifier::parse(ONTOLOGY).expect("a resource identifier"),
            token,
            bounds(cap),
            sutura_http_client::fixed(bounds(cap), None),
        );
        let name = SourceName::parse("ontology").expect("a name");
        FoundryCatalog::new(
            name.clone(),
            DefinitionVersion::parse("test").expect("a version"),
            name,
            reader,
        )
    }

    fn catalog(server: &FakeServer, cap: u64) -> FoundryCatalog<HttpOntologyReader> {
        catalog_with(server, TokenSource::fixed(Secret::new(TOKEN)).expect("a bearer value"), cap)
    }

    fn json(text: &str) -> serde_json::Value {
        serde_json::from_str(text).expect("a recorded page is JSON")
    }

    /// The recorded answers, in the order the reader asks: object types, then each one's links.
    fn recorded() -> Vec<Scripted> {
        let mut answers = vec![Scripted::ok(&json(fixture::OBJECT_TYPES))];
        answers.extend(fixture::OUTGOING.iter().map(|(_, page)| Scripted::ok(&json(page))));
        answers
    }

    /// Every link of the cause chain, rendered, with each link's `Debug` beside it.
    fn chain(error: &FoundryError) -> String {
        use std::fmt::Write as _;
        let mut text = format!("{error} {error:?}");
        let mut cause = std::error::Error::source(error);
        while let Some(link) = cause {
            write!(text, " | {link} {link:?}").expect("writing to a String cannot fail");
            cause = link.source();
        }
        text
    }

    fn read_cause(error: &FoundryError) -> &HttpReaderError {
        let FoundryError::Read(cause) = error else {
            panic!("not a read failure: {error:?}");
        };
        cause.downcast_ref().expect("the HTTP reader's own error")
    }

    #[test]
    fn the_reader_asks_the_two_documented_paths_with_the_bearer_and_reads_what_the_fixture_reads() {
        let server = FakeServer::start(recorded());
        let pinned = catalog(&server, 1 << 20).load().expect("the recorded answers load over HTTP");
        let requests = server.finish();
        let root = format!("{API_ROOT}/{ONTOLOGY}/objectTypes");
        let mut expected = vec![format!("GET {root}?pageSize=500 HTTP/1.1")];
        expected.extend(
            fixture::OUTGOING
                .iter()
                .map(|(name, _)| format!("GET {root}/{name}/outgoingLinkTypes?pageSize=500 HTTP/1.1")),
        );
        assert_eq!(
            requests
                .iter()
                .map(sutura_http_client::test_support::CapturedRequest::request_line)
                .collect::<Vec<_>>(),
            expected
        );
        for request in &requests {
            assert_eq!(request.authorization(), Some(format!("Bearer {TOKEN}").as_str()));
        }
        let recorded = fixture::over_fixture_source(
            SourceName::parse("ontology").expect("a name"),
            DefinitionVersion::parse("test").expect("a version"),
        )
        .load()
        .expect("the fixture loads");
        assert_eq!(pinned.digest(), recorded.digest());
    }

    #[test]
    fn the_page_token_is_sent_back_on_the_same_path_and_a_repeated_one_is_refused() {
        let mut page = json(fixture::OBJECT_TYPES);
        let rest = page["data"].as_array_mut().expect("a data array").split_off(2);
        page["nextPageToken"] = "page two/?#".into();
        let mut answers = vec![Scripted::ok(&page), Scripted::ok(&serde_json::json!({ "data": rest }))];
        answers.extend(fixture::OUTGOING.iter().map(|(_, page)| Scripted::ok(&json(page))));
        let server = FakeServer::start(answers);
        catalog(&server, 1 << 20).load().expect("two pages load");
        let requests = server.finish();
        let second = requests[1].request_line();
        assert!(
            second.starts_with(&format!("GET {API_ROOT}/{ONTOLOGY}/objectTypes?pageSize=500&pageToken=page")),
            "{second}"
        );
        assert!(
            !second.contains("two/?#"),
            "the page token reached the request unencoded: {second}"
        );

        let server = FakeServer::start(vec![Scripted::ok(&page), Scripted::ok(&page)]);
        let refused = catalog(&server, 1 << 20).load().expect_err("a repeated token is refused");
        drop(server.finish());
        assert!(matches!(
            read_cause(&refused),
            HttpReaderError::Paging {
                cause: PagingRefusal::RepeatedCursor,
                ..
            }
        ));
    }

    #[test]
    fn a_page_token_over_the_cap_is_not_sent_back() {
        let mut page = json(fixture::OBJECT_TYPES);
        page["nextPageToken"] = "t".repeat(MAX_PAGE_TOKEN_BYTES + 1).into();
        let server = FakeServer::start(vec![Scripted::ok(&page)]);
        let refused = catalog(&server, 1 << 20).load().expect_err("an over-long token is refused");
        assert_eq!(server.finish().len(), 1);
        assert!(matches!(read_cause(&refused), HttpReaderError::PageTokenTooLong { .. }));
    }

    #[test]
    fn an_object_type_name_that_is_no_name_never_reaches_a_path() {
        let mut page = json(fixture::OBJECT_TYPES);
        page["data"][0]["apiName"] = "../../admin".into();
        let server = FakeServer::start(vec![Scripted::ok(&page)]);
        let refused = catalog(&server, 1 << 20).load().expect_err("a path-shaped name is refused");
        assert_eq!(server.finish().len(), 1, "a link path was dialled for a name that is no name");
        assert!(matches!(read_cause(&refused), HttpReaderError::ObjectTypeName(_)));
    }

    #[test]
    fn a_refusal_is_typed_and_carries_neither_the_token_nor_foundrys_text() {
        let server = FakeServer::start(vec![Scripted::status(403, &format!("forbidden for {TOKEN}"))]);
        let refused = catalog(&server, 1 << 20).load().expect_err("a 403 is refused");
        drop(server.finish());
        assert!(matches!(read_cause(&refused), HttpReaderError::Refused { status: 403, .. }));
        assert!(!chain(&refused).contains(TOKEN), "{}", chain(&refused));
    }

    #[test]
    fn an_answer_over_the_cap_is_refused() {
        let server = FakeServer::start(vec![Scripted::ok(&json(fixture::OBJECT_TYPES))]);
        let refused = catalog(&server, 1024).load().expect_err("an answer over the cap is refused");
        drop(server.finish());
        assert!(matches!(read_cause(&refused), HttpReaderError::TooLarge { cap: 1024, .. }));
    }

    #[test]
    fn a_client_credentials_token_is_minted_once_on_the_same_origin_and_presented() {
        let mut answers = vec![Scripted::ok(&serde_json::json!({
            "access_token": TOKEN,
            "token_type": "bearer",
            "expires_in": 3600,
        }))];
        answers.extend(recorded());
        let server = FakeServer::start(answers);
        let origin = Endpoint::parse(&server.endpoint()).expect("the fake server's own endpoint is usable");
        let grant = ClientCredentials::new(
            &origin,
            TokenEndpoint::parse(&format!("{}/multipass/api/oauth2/token", server.endpoint())).expect("a token endpoint"),
            ExchangeClient::new("catalog-reader", Secret::new("client-secret")).expect("a client id"),
            Scopes::parse(["api:ontologies-read"]).expect("a scope"),
            sutura_http_client::fixed(bounds(1 << 20), None),
        )
        .expect("one origin");
        catalog_with(&server, TokenSource::ClientCredentials(grant), 1 << 20)
            .load()
            .expect("the minted token reads the ontology");
        let requests = server.finish();
        assert_eq!(requests[0].request_line(), "POST /multipass/api/oauth2/token HTTP/1.1");
        assert_eq!(
            requests.len(),
            1 + 1 + fixture::OUTGOING.len(),
            "a fresh token was minted again"
        );
        for request in &requests[1..] {
            assert_eq!(request.authorization(), Some(format!("Bearer {TOKEN}").as_str()));
        }
    }

    #[test]
    fn an_ontology_that_is_not_one_path_segment_is_refused() {
        for raw in ["", "..", ".hidden", "a/b", "a?b", "a#b", "a%2Fb", "a b"] {
            assert!(OntologyIdentifier::parse(raw).is_err(), "{raw:?} was accepted");
        }
        let named = OntologyIdentifier::parse("flights-ontology").expect("an API name is one segment");
        assert_eq!(named.as_str(), "flights-ontology");
    }
}
