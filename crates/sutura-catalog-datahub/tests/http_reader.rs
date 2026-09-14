//! The real [`HttpAspectReader`] against a real local HTTP server - "ports get fakes, not mocked
//! HTTP" - serving the shapes `docs/adr/0016`'s "Field by field" table and `tests/provisioned.rs`'s
//! own measured `metric` envelope describe. No mock-HTTP crate: a hand-rolled `TcpListener` loop
//! answering one scripted response per request, in order.
//!
//! `#[cfg(all(test, feature = "http"))]` on the whole file for two reasons: the `http` feature gates
//! the reader itself, and wrapping the body in `#[cfg(test)] mod tests` is what lets
//! `allow-expect-in-tests`/`allow-panic-in-tests` apply here - the same shape
//! `tests/provisioned.rs`'s own header explains.
#![cfg(all(test, feature = "http"))]

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::Duration;

    use sutura_catalog_datahub::http::{Endpoint, HttpAspectReader, HttpReaderError, InvalidEndpoint, ReadBounds};
    use sutura_catalog_datahub::{AspectReader as _, DataHubCatalog, DataHubError};
    use sutura_domain::identity::Secret;
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};

    /// A generous default so a test that is not exercising the cap does not have to think about it.
    const GENEROUS_CAP: u64 = 1 << 20;

    /// One scripted answer: a status, a body, and how long to wait before sending it.
    struct Scripted {
        status: u16,
        body: Vec<u8>,
        delay: Duration,
    }

    impl Scripted {
        fn ok(body: &serde_json::Value) -> Self {
            Self {
                status: 200,
                body: body.to_string().into_bytes(),
                delay: Duration::ZERO,
            }
        }

        fn status(status: u16, body: &str) -> Self {
            Self {
                status,
                body: body.as_bytes().to_vec(),
                delay: Duration::ZERO,
            }
        }

        fn delayed(body: &serde_json::Value, delay: Duration) -> Self {
            Self {
                status: 200,
                body: body.to_string().into_bytes(),
                delay,
            }
        }
    }

    /// Every request this fake server has answered, in order: `authorization` header or `None`.
    ///
    /// A named alias rather than the type spelled at its one use, since a nested
    /// `Option<JoinHandle<Vec<Option<String>>>>` is over `clippy::type_complexity`'s threshold.
    type CapturedAuthorizations = Vec<Option<String>>;

    /// A real local HTTP/1.1 server answering one [`Scripted`] response per connection, in order,
    /// then closing. Captures each request's `authorization` header so a test can assert the bearer
    /// was sent.
    struct FakeServer {
        addr: std::net::SocketAddr,
        handle: Option<thread::JoinHandle<CapturedAuthorizations>>,
    }

    impl FakeServer {
        fn start(answers: Vec<Scripted>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
            let addr = listener.local_addr().expect("a bound listener has a local address");
            // **`answers.into_iter()` bounds the loop, not `listener.incoming()`.** The obvious
            // shape - `for stream in listener.incoming() { let Some(answer) = answers.next() else
            // { break }; ... }` - blocks INSIDE `incoming()`'s next call waiting for a connection
            // that will never arrive once the scripted answers are exhausted and the client has
            // stopped asking: the break is only reached AFTER a connection is accepted, not
            // before. Looping over `answers` and calling `accept()` exactly that many times is
            // what lets this thread exit - and `FakeServer::finish`'s `.join()` return - once the
            // last scripted answer has been served, with no further accept ever attempted.
            let handle = thread::spawn(move || {
                let mut authorizations = Vec::new();
                for answer in answers {
                    let Ok((mut stream, _)) = listener.accept() else { break };
                    authorizations.push(read_authorization(&mut stream));
                    if !answer.delay.is_zero() {
                        thread::sleep(answer.delay);
                    }
                    write_response(&mut stream, answer.status, &answer.body);
                }
                authorizations
            });
            Self {
                addr,
                handle: Some(handle),
            }
        }

        fn endpoint(&self) -> String {
            format!("http://{}", self.addr)
        }

        /// Joins the server thread and returns every request's `authorization` header, in order.
        ///
        /// Only called by a test that knows exactly how many connections it will make - a test that
        /// deliberately stops short (the deadline test) drops the server instead, and the abandoned
        /// thread exits with the process.
        fn finish(mut self) -> CapturedAuthorizations {
            self.handle
                .take()
                .expect("a server is finished at most once")
                .join()
                .expect("the fake server thread did not panic")
        }
    }

    /// Reads one HTTP request up to its blank line and returns its `authorization` header, if any.
    /// This reader is never asked to read a GET body, so it does not look for one.
    fn read_authorization(stream: &mut TcpStream) -> Option<String> {
        let mut buf = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            let read = stream.read(&mut chunk).ok()?;
            if read == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..read]);
            if buf.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        String::from_utf8_lossy(&buf)
            .lines()
            .find(|line| line.to_ascii_lowercase().starts_with("authorization:"))
            .map(|line| line.split_once(':').map_or("", |(_, value)| value).trim().to_owned())
    }

    fn write_response(stream: &mut TcpStream, status: u16, body: &[u8]) {
        let head = format!(
            "HTTP/1.1 {status} x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        drop(stream.write_all(head.as_bytes()));
        drop(stream.write_all(body));
        drop(stream.flush());
    }

    fn token() -> Secret {
        Secret::new(String::from("pat-under-test"))
    }

    /// The `sutura` field's independence from the deployment's own property name is the whole point
    /// of `docs/adr/0016` decision 7 - see `tests/provisioned.rs`'s `DEPLOYMENT_PROPERTY`. Reused
    /// here for the same reason: a fake registering the adapter's own field name would pass equally
    /// whether the name were the deployment's choice or a constant this crate requires.
    const DEPLOYMENT_PROPERTY: &str = "deployment_metric_document";

    fn bounds(timeout_seconds: u64, cap: u64) -> ReadBounds {
        ReadBounds::parse(timeout_seconds, cap).expect("a positive timeout and cap are usable bounds")
    }

    fn source_name() -> SourceName {
        SourceName::parse("local").expect("a test source name is a name")
    }

    fn version() -> DefinitionVersion {
        DefinitionVersion::parse("test").expect("a test version is a version")
    }

    /// One `dataset` page, over the two models the certified fixture metric needs: `orders`
    /// (carrying every column the metric's measure, time column and required filter name) and
    /// `customers` (carrying the dimension's column). Model name and table are the same string,
    /// because `HttpAspectReader::read_datasets`'s own doc names that as a real limit rather than
    /// hiding it - see `crates/sutura-catalog-datahub/src/http.rs`.
    fn dataset_page() -> serde_json::Value {
        serde_json::json!({
            "entities": [
                {
                    "urn": "urn:li:dataset:(urn:li:dataPlatform:bigquery,orders,PROD)",
                    "schemaMetadata": { "value": { "fields": [
                        {"fieldPath": "order_id"},
                        {"fieldPath": "customer_id"},
                        {"fieldPath": "amount_cents"},
                        {"fieldPath": "order_date"},
                        {"fieldPath": "status"},
                    ] } },
                    "datasetProperties": { "value": { "description": "Net revenue orders, in minor units." } },
                },
                {
                    "urn": "urn:li:dataset:(urn:li:dataPlatform:bigquery,customers,PROD)",
                    "schemaMetadata": { "value": { "fields": [
                        {"fieldPath": "customer_id"},
                        {"fieldPath": "segment"},
                    ] } },
                    "datasetProperties": { "value": { "description": "The customer dimension." } },
                },
            ]
        })
    }

    /// One `semanticModel` page, over the one relationship the certified fixture metric's dimension
    /// reaches `customers` through.
    fn relationship_page() -> serde_json::Value {
        serde_json::json!({
            "entities": [
                {
                    "semanticModelRelationship": { "value": {
                        "name": "orders_to_customer",
                        "from": "urn:li:dataset:(urn:li:dataPlatform:bigquery,orders,PROD)",
                        "fromColumns": ["customer_id"],
                        "to": "urn:li:dataset:(urn:li:dataPlatform:bigquery,customers,PROD)",
                        "toColumns": ["customer_id"],
                        "cardinality": "N_ONE",
                    } },
                },
            ]
        })
    }

    /// One `metric` page carrying the recorded fixture's OWN certified metric, read through the
    /// crate's public port rather than restated here - the same reason
    /// `tests/provisioned.rs::a_document_served_by_a_real_datahub_decodes_into_a_certified_metric`
    /// does it that way: the two cannot drift by construction.
    fn metric_page() -> serde_json::Value {
        let recorded = sutura_catalog_datahub::fixture::FixtureReader
            .read()
            .expect("the recorded fixture reads");
        let certified = recorded
            .metrics()
            .iter()
            .find(|metric| metric.sutura().is_some())
            .expect("the fixture carries one certified metric");
        let property = certified.sutura().expect("the metric just found carries the property");
        serde_json::json!({
            "entities": [
                {
                    "urn": "urn:li:metric:(urn:li:dataPlatform:bigquery,orders,revenue)",
                    "metricInfo": { "value": {
                        "name": certified.name(),
                        "expression": { "dialects": [{ "dialect": certified.dialect(), "expression": certified.expression() }] },
                    } },
                    "structuredProperties": { "value": { "properties": [
                        { "propertyUrn": format!("urn:li:structuredProperty:{DEPLOYMENT_PROPERTY}"), "values": [{ "string": property.string_value() }] },
                    ] } },
                },
            ]
        })
    }

    fn happy_path_answers() -> Vec<Scripted> {
        vec![
            Scripted::ok(&dataset_page()),
            Scripted::ok(&relationship_page()),
            Scripted::ok(&metric_page()),
        ]
    }

    fn reader(server: &FakeServer, timeout_seconds: u64, cap: u64) -> HttpAspectReader {
        HttpAspectReader::new(
            Endpoint::parse(&server.endpoint()).expect("a loopback fake server's own endpoint is a usable one"),
            String::from(DEPLOYMENT_PROPERTY),
            token(),
            bounds(timeout_seconds, cap),
        )
    }

    fn http_cause(error: &DataHubError) -> &HttpReaderError {
        let DataHubError::Read { cause } = error else {
            panic!("a reader failure reaches DataHubCatalog as DataHubError::Read: {error}");
        };
        cause
            .downcast_ref::<HttpReaderError>()
            .unwrap_or_else(|| panic!("the boxed cause is this reader's own error type: {cause}"))
    }

    /// **The bearer is sent on every one of the three requests.**
    ///
    /// RED/GREEN mutation: delete `HttpAspectReader::bearer`'s call site (the `.header("authorization", ..)`
    /// line in `fetch`) - the fake server still answers (it does not itself check the header), but
    /// every captured `authorization` becomes `None` and this assertion goes red.
    #[test]
    fn the_bearer_is_sent_on_every_request() {
        let server = FakeServer::start(happy_path_answers());
        let read = reader(&server, 10, GENEROUS_CAP).read();
        let seen = server.finish();
        drop(read.expect("three well-formed pages read"));
        assert_eq!(seen.len(), 3, "one request per entity type");
        for authorization in seen {
            assert_eq!(
                authorization.as_deref(),
                Some("Bearer pat-under-test"),
                "every request carries the same bearer"
            );
        }
    }

    /// **A 401 is a typed refusal, and `DataHub`'s own response text never reaches `Display` or `Debug`.**
    ///
    /// RED/GREEN mutation: have the `#[error(...)]` on `HttpReaderError::Refused` interpolate `detail`
    /// (it would first need `EndpointMessage` to grow a `Display` impl, which is itself the guard this
    /// test backs up) - this assertion catches the marker text reaching either rendering.
    #[test]
    fn a_401_is_a_typed_refusal_naming_no_endpoint_text() {
        const MARKER: &str = "do-not-leak-this-marker";
        let server = FakeServer::start(vec![Scripted::status(401, &format!("{{\"error\":\"{MARKER}\"}}"))]);
        let error = reader(&server, 10, GENEROUS_CAP)
            .read()
            .expect_err("a 401 is refused, not read");
        drop(server.finish());
        let cause = http_cause(&error);
        let HttpReaderError::Refused { entity, status, .. } = cause else {
            panic!("a 401 maps to Refused, got: {cause}");
        };
        assert_eq!(*entity, "dataset");
        assert_eq!(*status, 401);
        assert!(
            !format!("{cause}").contains(MARKER),
            "Display must not carry DataHub's own text"
        );
        assert!(
            !format!("{cause:?}").contains(MARKER),
            "Debug must not carry DataHub's own text"
        );
    }

    /// **A response over the declared cap is refused, not decoded.**
    ///
    /// RED/GREEN mutation: delete the `if text.len() as u64 > cap { return Err(TooLarge ..) }` check
    /// in `HttpAspectReader::fetch` - the oversized body (still within `ureq`'s own generous backstop
    /// limit) would then reach `serde_json::from_str` and this assertion goes red.
    #[test]
    fn a_response_over_the_cap_is_refused() {
        const CAP: u64 = 16;
        // One byte past the declared cap - not JSON, and deliberately so: the length check in
        // `HttpAspectReader::fetch` runs BEFORE the JSON decode, so this proves the refusal fires on
        // size alone. Small enough to stay well inside `fetch`'s own backstop limit (`cap` plus a
        // margin), which is measured to matter: a body landing exactly ON a limit set to `cap + 1`
        // failed the read outright on this `ureq` version, which is why `fetch` no longer relies on
        // that library's own boundary behaviour for the precise refusal - this test's explicit
        // length-check assertion is what is under test, not `ureq`'s.
        let oversized = Scripted {
            status: 200,
            body: vec![b'x'; usize::try_from(CAP + 1).expect("a small test constant fits in usize")],
            delay: Duration::ZERO,
        };
        let server = FakeServer::start(vec![oversized]);
        let error = reader(&server, 10, CAP).read().expect_err("an oversized page is refused");
        drop(server.finish());
        assert!(
            matches!(
                http_cause(&error),
                HttpReaderError::TooLarge {
                    entity: "dataset",
                    cap: CAP
                }
            ),
            "expected TooLarge{{entity: \"dataset\", cap: {CAP}}}, got: {}",
            http_cause(&error)
        );
    }

    /// **The decoded snapshot certifies the same `Measure` the recorded fixture does.**
    ///
    /// This is issue #202's own question, answered over the wire instead of over a recorded string:
    /// the metric page's shape is the one `tests/provisioned.rs` measured live, and the dataset/
    /// relationship pages are this test's own (`docs/adr/0016`-schema, unmeasured - see
    /// `crates/sutura-catalog-datahub/src/http.rs`'s module header).
    ///
    /// RED/GREEN mutation: swap the JSON-pointer path `harvest_metric` reads the scalar from
    /// (`values[0]["string"]`) for the wrong index or key - the certified `Measure` then either goes
    /// missing (`UnknownMeasureColumn`/decode failure) or, if the fake carried a second bogus value,
    /// decodes the wrong document - either way this equality goes red.
    #[test]
    fn the_decoded_metric_equals_the_fixtures_certified_measure() {
        let server = FakeServer::start(happy_path_answers());
        let mut sources = std::collections::BTreeMap::new();
        drop(sources.insert(String::from("bigquery"), source_name()));
        let bundle = DataHubCatalog::new(source_name(), version(), sources, reader(&server, 10, GENEROUS_CAP))
            .load()
            .expect("the wire-read snapshot assembles into a bundle");
        drop(server.finish());

        let recorded = sutura_catalog_datahub::fixture::FixtureReader
            .read()
            .expect("the recorded fixture reads");
        let certified = recorded
            .metrics()
            .iter()
            .find(|metric| metric.sutura().is_some())
            .expect("the fixture carries one certified metric");
        let content = certified
            .sutura()
            .expect("the metric just found carries the property")
            .assemble()
            .expect("the recorded document decodes");

        let name = sutura_domain::model::MetricName::parse(certified.name()).expect("the fixture metric is named");
        let metric = bundle
            .definitions()
            .metric(&name)
            .expect("the wire-read metric is certified rather than a promotion candidate");
        assert_eq!(
            metric.measure(),
            Some(content.measure()),
            "the certified measure read over HTTP is the one the recorded fixture carries"
        );
    }

    /// **One shared deadline across the (up to) three requests, not one per request.**
    ///
    /// A one-second budget and a first response delayed past it: the second request
    /// (`semanticModel`) must never be attempted at all, because nothing is left of the shared
    /// budget - it is refused as `DeadlineSpent`, not as a slow-but-independent second timeout.
    /// Asserted by NAME rather than by wall clock alone, so a mutation that widens each request's
    /// own timeout back to the full budget (rather than what is left of it) is caught even though
    /// both shapes eventually fail: a widened per-request timeout would instead reach a connection
    /// refusal once the fake server's single scripted answer is exhausted.
    ///
    /// RED/GREEN mutation: in `HttpAspectReader::fetch`, replace `budget.remaining()` with
    /// `Some(self.bounds.timeout())` - the pre-flight `DeadlineSpent` check can no longer fire, and
    /// this test's variant assertion goes red (the second request is attempted and refused some
    /// other way instead).
    #[test]
    fn the_shared_deadline_is_honoured_across_requests() {
        let server = FakeServer::start(vec![Scripted::delayed(&dataset_page(), Duration::from_millis(1200))]);
        let started = std::time::Instant::now();
        let error = reader(&server, 1, GENEROUS_CAP)
            .read()
            .expect_err("the shared budget is spent");
        let elapsed = started.elapsed();
        // Deliberately not joined: the fake server is still waiting on a second connection that
        // this reader must never make, so a `.finish()` here would hang the test on the very
        // behaviour it is proving does not happen.
        assert!(
            matches!(
                http_cause(&error),
                HttpReaderError::DeadlineSpent {
                    entity: "semanticModel",
                    ..
                }
            ),
            "expected DeadlineSpent naming semanticModel, got: {}",
            http_cause(&error)
        );
        assert!(
            elapsed < Duration::from_secs(3),
            "a spent budget is refused promptly rather than waited out again: {elapsed:?}"
        );
    }

    /// **A page that signals more results than the one page this reader reads is refused, not
    /// silently truncated.**
    ///
    /// RED/GREEN mutation: delete the `total`/`scrollId` check in
    /// `HttpAspectReader::page_signals_more` - a page reporting a `total` above what it returned
    /// would then be read as complete, and this assertion goes red.
    #[test]
    fn a_page_reporting_more_results_than_it_returned_is_refused() {
        let mut truncated = dataset_page();
        truncated["total"] = serde_json::json!(2);
        // Only ONE of the two entities the "total" claims, on a page carrying two.
        let entities = truncated["entities"].as_array_mut().expect("the dataset page is an array");
        drop(entities.pop());
        assert_eq!(entities.len(), 1);
        let server = FakeServer::start(vec![Scripted::ok(&truncated)]);
        let error = reader(&server, 10, GENEROUS_CAP)
            .read()
            .expect_err("a truncated page is refused");
        drop(server.finish());
        assert!(
            matches!(http_cause(&error), HttpReaderError::MorePages { entity: "dataset" }),
            "expected MorePages{{entity: \"dataset\"}}, got: {}",
            http_cause(&error)
        );
    }

    /// **A relationship declaring more than one column per side is refused BY NAME, not silently
    /// narrowed to the first.** `docs/adr/0016`'s "Field by field" table names `DataHub` as WIDER
    /// than this adapter here; `one_column` is supposed to hold the line, and this cell is the one
    /// that actually asks it to - a review found the earlier suite proved only the one-column happy
    /// path, so deleting `one_column`'s `columns.len() == 1` filter survived every existing test.
    ///
    /// RED/GREEN mutation: delete the `.filter(|columns| columns.len() == 1)` in `one_column` - the
    /// first of the two columns would be taken silently and this assertion goes red.
    #[test]
    fn a_relationship_with_more_than_one_column_per_side_is_refused() {
        let mut multi_column = relationship_page();
        multi_column["entities"][0]["semanticModelRelationship"]["value"]["fromColumns"] = serde_json::json!(["a", "b"]);
        // The dataset page is first in read order and must be well-formed so the read REACHES the
        // relationship page this cell is actually about.
        let server = FakeServer::start(vec![Scripted::ok(&dataset_page()), Scripted::ok(&multi_column)]);
        let error = reader(&server, 10, GENEROUS_CAP)
            .read()
            .expect_err("a multi-column relationship side is refused");
        drop(server.finish());
        assert!(
            matches!(
                http_cause(&error),
                HttpReaderError::UnexpectedShape {
                    entity: "semanticModel",
                    field: "fromColumns"
                }
            ),
            "expected UnexpectedShape{{entity: \"semanticModel\", field: \"fromColumns\"}}, got: {}",
            http_cause(&error)
        );
    }

    /// **A dataset entity with no `schemaMetadata` aspect at all is refused BY NAME, not read as
    /// zero columns.** The same review that found the multi-column gap above found this one too: no
    /// existing cell constructed a wrong-shaped page, so a `.unwrap_or_default()` in place of the
    /// `schemaMetadata.value.fields` refusal would have survived silently.
    ///
    /// RED/GREEN mutation: replace the `.ok_or(UnexpectedShape { field: "schemaMetadata.value.fields" })?`
    /// in `harvest_dataset` with `.unwrap_or_default()` - a dataset missing the aspect would decode
    /// with zero columns instead of refusing, and this assertion goes red.
    #[test]
    fn a_dataset_with_no_schema_metadata_aspect_is_refused() {
        let mut missing_schema = dataset_page();
        // `as_object_mut` rather than replacing the whole entity, so everything else about the
        // entity - its `urn`, its `datasetProperties` - stays exactly as the happy path's, and only
        // the aspect under test is absent.
        drop(
            missing_schema["entities"][0]
                .as_object_mut()
                .expect("a dataset entity is an object")
                .remove("schemaMetadata"),
        );
        let server = FakeServer::start(vec![Scripted::ok(&missing_schema)]);
        let error = reader(&server, 10, GENEROUS_CAP)
            .read()
            .expect_err("a dataset with no schemaMetadata aspect is refused");
        drop(server.finish());
        assert!(
            matches!(
                http_cause(&error),
                HttpReaderError::UnexpectedShape {
                    entity: "dataset",
                    field: "schemaMetadata.value.fields"
                }
            ),
            "expected UnexpectedShape{{entity: \"dataset\", field: \"schemaMetadata.value.fields\"}}, got: {}",
            http_cause(&error)
        );
    }

    /// **A `scrollId` alone - no `total` field at all - is refused, not just a `total` above the
    /// returned count.** `page_signals_more` checks both tells; a review found the existing cell
    /// exercises only the `total` arm (the PR's own "paging truncation" mutation hardcoded the
    /// WHOLE function to `false`, which cannot tell the two arms apart), so this cell isolates the
    /// `scrollId` arm on its own.
    ///
    /// RED/GREEN mutation: delete the `scrollId` check in `page_signals_more` (keep the `total`
    /// arm) - this page carries no `total` at all, so only the `scrollId` arm can catch it, and
    /// deleting it alone goes red.
    #[test]
    fn a_page_carrying_only_a_scroll_id_is_refused() {
        let mut scrolling = dataset_page();
        scrolling["scrollId"] = serde_json::json!("opaque-scroll-token");
        let server = FakeServer::start(vec![Scripted::ok(&scrolling)]);
        let error = reader(&server, 10, GENEROUS_CAP)
            .read()
            .expect_err("a page carrying a scrollId and no total is refused");
        drop(server.finish());
        assert!(
            matches!(http_cause(&error), HttpReaderError::MorePages { entity: "dataset" }),
            "expected MorePages{{entity: \"dataset\"}}, got: {}",
            http_cause(&error)
        );
    }

    /// A userinfo endpoint is refused at PARSE - [`Endpoint::parse`] returns
    /// [`InvalidEndpoint::CredentialsInUrl`] before any `HttpAspectReader` can be built, so no
    /// `HttpAspectReader` exists to make a call. This is a parse-refusal cell, not a dial probe:
    /// nothing here opens a socket, and no listener stands by to observe a connection that is
    /// never made (the round-2 review's dial probe that motivated the `CredentialsInUrl` refusal
    /// is held by the round-2 unit cell `a_userinfo_prefix_naming_a_loopback_ip_...` instead).
    #[test]
    fn a_userinfo_endpoint_is_a_parse_refusal_rather_than_a_dial_probe() {
        let malicious = String::from("http://[::1]:1@127.0.0.1:9002");
        let error = Endpoint::parse(&malicious).expect_err("a userinfo prefix is refused before any host is dialled");
        assert_eq!(error, InvalidEndpoint::CredentialsInUrl { given: malicious });
    }
}
