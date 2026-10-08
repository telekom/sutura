#![forbid(unsafe_code)]
//! The real [`HttpAspectReader`] against a real local HTTP server - "ports get fakes, not mocked
//! HTTP" - serving the shapes `docs/adr/0016`'s "Field by field" table and `tests/provisioned.rs`'s
//! own measured `metric` envelope describe. No mock-HTTP crate: a hand-rolled `TcpListener` loop
//! answering one scripted response per request, in order.
//!
//! **The fake server and the happy-path pages live in `src/test_support.rs`, not here** - issue
//! #202's second PR moved them so `sutura-serve`'s own served-binary suite could build the same fake
//! rather than a second one; see that module's header for why the move is a library concern and not
//! only a test one. What stays HERE is this crate's own test-local scaffolding (a token, a source
//! name, the reader constructor, the cause-downcast helper) and every `#[test]`.
//!
//! `#[cfg(all(test, feature = "http", feature = "fake"))]` on the whole file for three
//! reasons: the `http` feature gates the reader itself, `fake` gates the loopback fake
//! this suite builds against (issue #970's review moved it out of `http` so a shipped binary
//! never carries it), and wrapping the body in `#[cfg(test)] mod tests` is what lets
//! `allow-expect-in-tests`/`allow-panic-in-tests` apply here - the same shape
//! `tests/provisioned.rs`'s own header explains.
#![cfg(all(test, feature = "http", feature = "fake"))]

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sutura_catalog_datahub::http::{
        DEFAULT_MAX_ENTITIES, DEFAULT_PAGE_SIZE, Endpoint, HttpAspectReader, HttpReaderError, InvalidEndpoint, PageLimits,
        PagingRefusal, ReadBounds,
    };
    use sutura_catalog_datahub::test_support::{
        DEPLOYMENT_PROPERTY, FakeServer, Scripted, dataset_page, happy_path_answers, metric_page, relationship_page,
    };
    use sutura_catalog_datahub::{AspectReader as _, DataHubCatalog, DataHubError};
    use sutura_domain::identity::Secret;
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};

    /// A generous default so a test that is not exercising the cap does not have to think about it.
    const GENEROUS_CAP: u64 = 1 << 20;

    fn token() -> Secret {
        Secret::new(String::from("pat-under-test"))
    }

    fn bounds(timeout_seconds: u64, cap: u64) -> ReadBounds {
        ReadBounds::parse(timeout_seconds, cap).expect("a positive timeout and cap are usable bounds")
    }

    fn source_name() -> SourceName {
        SourceName::parse("local").expect("a test source name is a name")
    }

    fn version() -> DefinitionVersion {
        DefinitionVersion::parse("test").expect("a test version is a version")
    }

    fn reader(server: &FakeServer, timeout_seconds: u64, cap: u64) -> HttpAspectReader {
        HttpAspectReader::new(
            Endpoint::parse(&server.endpoint()).expect("a loopback fake server's own endpoint is a usable one"),
            String::from(DEPLOYMENT_PROPERTY),
            token(),
            bounds(timeout_seconds, cap),
            // No declared `security.outbound` in the plaintext-loopback cases of this file - the
            // fake answers over `http://`, and the anchors arm of the constructor is exercised by
            // this file's own `tests::tls_anchors` cells.
            None,
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
    /// **A proxy the environment names never carries the plaintext loopback read.**
    #[test]
    fn an_environment_proxy_never_carries_the_plaintext_loopback_read() {
        sutura_dev::env_proxy::dialled_directly(
            module_path!(),
            "an_environment_proxy_never_carries_the_plaintext_loopback_read",
            || {
                let server = FakeServer::start(happy_path_answers());
                let read = reader(&server, 2, GENEROUS_CAP).read();
                drop(read.expect("the read reached the loopback endpoint directly"));
                drop(server.finish());
            },
        );
    }

    #[test]
    fn the_bearer_is_sent_on_every_request() {
        let server = FakeServer::start(happy_path_answers());
        let read = reader(&server, 10, GENEROUS_CAP).read();
        let seen = server.finish();
        drop(read.expect("three well-formed pages read"));
        assert_eq!(seen.len(), 3, "one request per entity type");
        for request in seen {
            assert_eq!(
                request.authorization(),
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
        let oversized = Scripted::raw(
            200,
            vec![b'x'; usize::try_from(CAP + 1).expect("a small test constant fits in usize")],
        );
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

    /// **A body that is not UTF-8 is refused as unreadable, not as unreached.**
    #[test]
    fn a_response_that_cannot_be_read_as_text_is_refused() {
        let server = FakeServer::start(vec![Scripted::raw(200, vec![0xFF, 0xFE])]);
        let error = reader(&server, 10, GENEROUS_CAP).read().err();
        drop(server.finish());
        let cause = error.as_ref().map(http_cause);
        assert!(
            matches!(cause, Some(HttpReaderError::Unreadable { entity: "dataset", .. })),
            "{cause:?}"
        );
    }

    /// **A 200 whose body is text but not JSON is refused as not a document.**
    #[test]
    fn a_successful_response_that_is_not_json_is_refused() {
        let server = FakeServer::start(vec![Scripted::raw(200, b"not-json-at-all".to_vec())]);
        let error = reader(&server, 10, GENEROUS_CAP).read().err();
        drop(server.finish());
        let cause = error.as_ref().map(http_cause);
        assert!(
            matches!(cause, Some(HttpReaderError::NotADocument { entity: "dataset", .. })),
            "{cause:?}"
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

    /// **A schema field's `nativeDataType`/`description`/`isPartOfKey` map into the model's own
    /// column - over the REAL reader, not the fixture.** A review found this had no test: every
    /// existing `dataset_page()`-based cell served fields with `fieldPath` alone, so a mutation
    /// dropping `harvest_dataset`'s new column-metadata block entirely stayed green.
    ///
    /// RED/GREEN mutation: in `harvest_dataset`, replace the `for field in fields` loop's body with
    /// nothing (never populate `column_metadata`/`primary_key`) - this test's three assertions go
    /// red one at a time as each is removed.
    #[test]
    fn a_schema_fields_type_description_and_key_flag_map_into_the_model_over_http() {
        let mut dataset = dataset_page();
        dataset["entities"][0]["schemaMetadata"]["value"]["fields"][0] = serde_json::json!({
            "fieldPath": "order_id",
            "nativeDataType": "BIGINT",
            "description": "The order's own identifier.",
            "isPartOfKey": true,
        });
        let server = FakeServer::start(vec![
            Scripted::ok(&dataset),
            Scripted::ok(&relationship_page()),
            Scripted::ok(&metric_page()),
        ]);
        let mut sources = std::collections::BTreeMap::new();
        drop(sources.insert(String::from("bigquery"), source_name()));
        let bundle = DataHubCatalog::new(source_name(), version(), sources, reader(&server, 10, GENEROUS_CAP))
            .load()
            .expect("a schema field carrying the new keys still loads");
        drop(server.finish());

        let orders = bundle
            .definitions()
            .models()
            .get(&sutura_domain::model::ModelName::parse("orders").expect("a fixture model is a model"))
            .expect("orders is a model");
        let order_id = sutura_domain::model::ColumnName::parse("order_id").expect("a fixture column is a column");
        let column = orders.column(&order_id).expect("order_id is declared");
        assert_eq!(
            column.data_type().map(sutura_domain::catalog::ColumnType::as_str),
            Some("BIGINT")
        );
        assert_eq!(column.description(), "The order's own identifier.");
        assert_eq!(orders.primary_key(), &std::collections::BTreeSet::from([order_id]));
    }

    /// **The finding this PR was built to close: the real, nested `nativeDataType` a review measured
    /// against a live-shaped page must not refuse the whole load.** At base, `harvest_dataset` read
    /// only `fieldPath`, so this page loaded; before `ColumnType` had its own bound (review, over
    /// the shared 64-character dimension bound), this exact 70-character type refused the WHOLE
    /// catalog with "a dimension value may be at most 64 characters". `MAX_COLUMN_TYPE_CHARS` is
    /// generous enough that this legal type is carried, not dropped.
    #[test]
    fn a_real_nested_native_data_type_over_http_is_carried_not_refused() {
        let long_type = "STRUCT<street STRING, city STRING, postal_code STRING, country STRING>";
        assert!(
            long_type.len() <= sutura_domain::catalog::MAX_COLUMN_TYPE_CHARS,
            "the reviewed example must fit inside the bound, or this proves nothing about it"
        );
        let mut dataset = dataset_page();
        dataset["entities"][0]["schemaMetadata"]["value"]["fields"][2] = serde_json::json!({
            "fieldPath": "amount_cents",
            "nativeDataType": long_type,
        });
        let server = FakeServer::start(vec![
            Scripted::ok(&dataset),
            Scripted::ok(&relationship_page()),
            Scripted::ok(&metric_page()),
        ]);
        let mut sources = std::collections::BTreeMap::new();
        drop(sources.insert(String::from("bigquery"), source_name()));
        let bundle = DataHubCatalog::new(source_name(), version(), sources, reader(&server, 10, GENEROUS_CAP))
            .load()
            .expect("the reviewed real-world nested type must not refuse the whole catalog");
        drop(server.finish());
        let orders = bundle
            .definitions()
            .models()
            .get(&sutura_domain::model::ModelName::parse("orders").expect("a fixture model is a model"))
            .expect("orders is a model");
        let amount_cents = sutura_domain::model::ColumnName::parse("amount_cents").expect("a fixture column is a column");
        assert_eq!(
            orders
                .column(&amount_cents)
                .expect("amount_cents is declared")
                .data_type()
                .map(sutura_domain::catalog::ColumnType::as_str),
            Some(long_type)
        );
    }

    /// **A type past even the generous bound is dropped, not fatal - the escape hatch behind the
    /// bound.** Nothing observed in review is this long; this is the pathological case the bound
    /// itself exists to bound, proved over the real HTTP reader rather than only at the domain
    /// layer.
    #[test]
    fn a_native_data_type_past_even_the_generous_bound_is_dropped_rather_than_refusing_the_load() {
        let long_type = format!("STRUCT<{}last STRING>", "field STRING, ".repeat(80));
        assert!(long_type.len() > sutura_domain::catalog::MAX_COLUMN_TYPE_CHARS);
        let mut dataset = dataset_page();
        dataset["entities"][0]["schemaMetadata"]["value"]["fields"][2] = serde_json::json!({
            "fieldPath": "amount_cents",
            "nativeDataType": long_type,
        });
        let server = FakeServer::start(vec![
            Scripted::ok(&dataset),
            Scripted::ok(&relationship_page()),
            Scripted::ok(&metric_page()),
        ]);
        let mut sources = std::collections::BTreeMap::new();
        drop(sources.insert(String::from("bigquery"), source_name()));
        let bundle = DataHubCatalog::new(source_name(), version(), sources, reader(&server, 10, GENEROUS_CAP))
            .load()
            .expect("a type past even the generous bound must not refuse the whole catalog");
        drop(server.finish());
        let orders = bundle
            .definitions()
            .models()
            .get(&sutura_domain::model::ModelName::parse("orders").expect("a fixture model is a model"))
            .expect("orders is a model");
        let amount_cents = sutura_domain::model::ColumnName::parse("amount_cents").expect("a fixture column is a column");
        assert_eq!(
            orders.column(&amount_cents).expect("amount_cents is declared").data_type(),
            None
        );
    }

    /// **A dataset whose platform the composition root declares no source for is refused BY NAME at
    /// load, not run silently.** This catalog maps only `bigquery`; the served dataset entity's URN
    /// names `postgres`, so `DataHubCatalog::load` must refuse with `UnknownPlatform { platform:
    /// "postgres", .. }` rather than guess where that model reads from (`docs/adr/0016` decision 5).
    /// The reader serves every page - the refusal happens in the catalog's own source mapping, not
    /// in the wire read, which is why all three pages stay on the happy path.
    ///
    /// RED/GREEN mutation: have `split_dataset_urn` return `"bigquery"` for any non-empty platform
    /// segment - the `postgres` entity would then harvest as a `bigquery` dataset, the catalog would
    /// map a source for it, and the load would SUCCEED where this cell demands a refusal.
    #[test]
    fn a_postgres_platform_dataset_is_refused_with_unknown_platform() {
        let mut dataset = dataset_page();
        // Rewrite entity 0's own URN so the platform this cell refuses is what it names, keeping
        // every other aspect exactly the happy path's.
        dataset["entities"][0]["urn"] = serde_json::json!("urn:li:dataset:(urn:li:dataPlatform:postgres,orders,PROD)");
        let server = FakeServer::start(vec![
            Scripted::ok(&dataset),
            Scripted::ok(&relationship_page()),
            Scripted::ok(&metric_page()),
        ]);
        let mut sources = std::collections::BTreeMap::new();
        drop(sources.insert(String::from("bigquery"), source_name()));
        let outcome = DataHubCatalog::new(source_name(), version(), sources, reader(&server, 10, GENEROUS_CAP))
            .load()
            .err();
        drop(server.finish());
        assert!(
            matches!(outcome, Some(DataHubError::UnknownPlatform { ref platform, .. }) if platform == "postgres"),
            "expected UnknownPlatform naming postgres, got: {outcome:?}"
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

    /// The page size the paging cells force, so a few datasets span several pages.
    fn two_per_page() -> PageLimits {
        PageLimits::parse(2, DEFAULT_MAX_ENTITIES).expect("a page size of two is usable")
    }

    fn reader_of(server: &FakeServer, limits: PageLimits) -> HttpAspectReader {
        reader(server, 10, GENEROUS_CAP).with_page_limits(limits)
    }

    /// One `dataset` page of bare datasets named `names`, `scroll_id` naming the next page.
    fn datasets_of(names: &[&str], scroll_id: Option<&str>, total: Option<u64>) -> serde_json::Value {
        let entities: Vec<_> = names
            .iter()
            .map(|name| {
                serde_json::json!({
                    "urn": format!("urn:li:dataset:(urn:li:dataPlatform:bigquery,{name},PROD)"),
                    "schemaMetadata": { "value": { "fields": [{"fieldPath": "id"}] } },
                })
            })
            .collect();
        let mut page = serde_json::json!({ "entities": entities, "scrollId": scroll_id });
        if let Some(total) = total {
            page["total"] = serde_json::json!(total);
        }
        page
    }

    /// One `metric` page of bare metrics named `names`, `scroll_id` naming the next page.
    fn metrics_of(names: &[String], scroll_id: Option<&str>) -> serde_json::Value {
        let entities: Vec<_> = names
            .iter()
            .map(|name| {
                serde_json::json!({
                    "metricInfo": { "value": {
                        "name": name,
                        "expression": { "dialects": [{ "dialect": "ANSI_SQL", "expression": "count(*)" }] },
                    } },
                })
            })
            .collect();
        serde_json::json!({ "entities": entities, "scrollId": scroll_id })
    }

    fn paging_cause(error: &DataHubError) -> (&'static str, PagingRefusal) {
        match http_cause(error) {
            HttpReaderError::Paging { entity, cause } => (*entity, *cause),
            other => panic!("expected a paging refusal, got: {other}"),
        }
    }

    /// **A list of several pages loads every page, for datasets and for metrics.** The first request
    /// carries no `scrollId`, each later one carries the one the page before it gave - percent-encoded,
    /// because it is the service's own text - and `count` is the page size.
    ///
    /// RED/GREEN mutation: drop the `.query("scrollId", ..)` in `fetch` - every later request then
    /// repeats the first page's line, and this assertion goes red.
    #[test]
    fn a_list_of_several_pages_loads_every_page() {
        let metric_names: Vec<String> = ["m1", "m2", "m3"].into_iter().map(String::from).collect();
        let server = FakeServer::start(vec![
            Scripted::ok(&datasets_of(&["d1", "d2"], Some("s-1"), None)),
            Scripted::ok(&datasets_of(&["d3", "d4"], Some("s-2&x=1"), None)),
            Scripted::ok(&datasets_of(&["d5"], None, None)),
            Scripted::ok(&relationship_page()),
            Scripted::ok(&metrics_of(&metric_names[..2], Some("m-1"))),
            Scripted::ok(&metrics_of(&metric_names[2..], None)),
        ]);
        let snapshot = reader_of(&server, two_per_page()).read().expect("every page loads");
        let seen = server.finish();
        let datasets: Vec<_> = snapshot.datasets().iter().map(|dataset| dataset.name().to_owned()).collect();
        assert_eq!(datasets, ["d1", "d2", "d3", "d4", "d5"]);
        let metrics: Vec<_> = snapshot.metrics().iter().map(|metric| metric.name().to_owned()).collect();
        assert_eq!(metrics, ["m1", "m2", "m3"]);
        let lines: Vec<_> = seen
            .iter()
            .map(sutura_http_client::test_support::CapturedRequest::request_line)
            .collect();
        assert_eq!(
            lines,
            [
                "GET /openapi/v3/entity/dataset?aspects=schemaMetadata&aspects=datasetProperties&count=2 HTTP/1.1",
                "GET /openapi/v3/entity/dataset?aspects=schemaMetadata&aspects=datasetProperties&count=2&scrollId=s-1 HTTP/1.1",
                "GET /openapi/v3/entity/dataset?aspects=schemaMetadata&aspects=datasetProperties&count=2&scrollId=s-2%26x%3D1 HTTP/1.1",
                "GET /openapi/v3/entity/semanticModel?aspects=semanticModelInfo&count=2 HTTP/1.1",
                "GET /openapi/v3/entity/metric?aspects=metricInfo&aspects=structuredProperties&count=2 HTTP/1.1",
                "GET /openapi/v3/entity/metric?aspects=metricInfo&aspects=structuredProperties&count=2&scrollId=m-1 HTTP/1.1",
            ]
        );
    }

    /// **A `scrollId` the service gives twice is refused, not followed again.**
    ///
    /// RED/GREEN mutation: delete `Pager::advance`'s repeated-cursor refusal - the second page's
    /// scroll id would be followed and this read would not be refused.
    #[test]
    fn a_repeated_scroll_id_is_refused() {
        let server = FakeServer::start(vec![
            Scripted::ok(&datasets_of(&["d1", "d2"], Some("s-1"), None)),
            Scripted::ok(&datasets_of(&["d3", "d4"], Some("s-1"), None)),
        ]);
        let error = reader_of(&server, two_per_page())
            .read()
            .expect_err("a repeated scroll id is refused");
        assert_eq!(
            server.finish().len(),
            2,
            "the second page was read, its scroll id was not followed"
        );
        assert_eq!(paging_cause(&error), ("dataset", PagingRefusal::RepeatedCursor));
    }

    /// **A page with no entity that still reports more is refused, not followed.**
    ///
    /// RED/GREEN mutation: delete `Pager::advance`'s no-progress refusal - the empty page's fresh
    /// scroll id would be followed and this read would not be refused.
    #[test]
    fn a_page_with_no_progress_is_refused() {
        let server = FakeServer::start(vec![
            Scripted::ok(&datasets_of(&["d1", "d2"], Some("s-1"), None)),
            Scripted::ok(&datasets_of(&[], Some("s-2"), None)),
        ]);
        let error = reader_of(&server, two_per_page())
            .read()
            .expect_err("a page with no progress is refused");
        drop(server.finish());
        assert_eq!(paging_cause(&error), ("dataset", PagingRefusal::NoProgress));
    }

    /// **A list above the entity bound is refused, never cut short.** One more metric than the bound
    /// across a hundred and one default-sized pages: the refusal names the bound, and no snapshot of
    /// the first hundred thousand comes back.
    ///
    /// The two literals are the bound the readers ship with, never read from the constant, so moving
    /// the constant in either direction is a red cell.
    ///
    /// RED/GREEN mutation: raise or lower `DEFAULT_MAX_ENTITIES`, delete `Pager::advance`'s bound
    /// check, or answer `Ok(false)` where it refuses - the read then returns a snapshot (or runs the script out)
    /// and this assertion goes red.
    #[test]
    fn a_list_above_the_entity_bound_is_refused_not_truncated() {
        let over = 100_001;
        let names: Vec<String> = (0..over).map(|n| format!("m{n}")).collect();
        let mut answers = vec![Scripted::ok(&dataset_page()), Scripted::ok(&relationship_page())];
        let pages: Vec<_> = names.chunks(DEFAULT_PAGE_SIZE).collect();
        for (n, page) in pages.iter().enumerate() {
            let scroll_id = (n + 1 < pages.len()).then(|| format!("m-{n}"));
            answers.push(Scripted::ok(&metrics_of(page, scroll_id.as_deref())));
        }
        let server = FakeServer::start(answers);
        let error = reader(&server, 60, GENEROUS_CAP)
            .read()
            .expect_err("a list over the bound is refused, not truncated");
        drop(server.finish());
        assert_eq!(
            paging_cause(&error),
            ("metric", PagingRefusal::TooManyEntities { max: 100_000 })
        );
    }

    /// **A last page that leaves the list short of a reported `total` is refused, not read as
    /// complete.**
    ///
    /// RED/GREEN mutation: delete `Pager::advance`'s short-of-total refusal - the two datasets would
    /// be read as the whole list and this assertion goes red.
    #[test]
    fn a_last_page_short_of_the_reported_total_is_refused() {
        let server = FakeServer::start(vec![Scripted::ok(&datasets_of(&["d1", "d2"], None, Some(3)))]);
        let error = reader_of(&server, two_per_page())
            .read()
            .expect_err("a list shorter than its total is refused");
        drop(server.finish());
        assert_eq!(
            paging_cause(&error),
            ("dataset", PagingRefusal::ShortOfTotal { read: 2, total: 3 })
        );
    }

    /// **A `scrollId` that is not text is refused by name, not read as the last page.**
    ///
    /// RED/GREEN mutation: map `report`'s `Some(_)` arm to `None` - the page would be read as the
    /// last and this assertion goes red.
    #[test]
    fn a_scroll_id_that_is_not_text_is_refused() {
        let mut page = datasets_of(&["d1"], None, None);
        page["scrollId"] = serde_json::json!(7);
        let server = FakeServer::start(vec![Scripted::ok(&page)]);
        let error = reader_of(&server, two_per_page())
            .read()
            .expect_err("a numeric scroll id is refused");
        drop(server.finish());
        assert!(
            matches!(
                http_cause(&error),
                HttpReaderError::UnexpectedShape {
                    entity: "dataset",
                    field: "scrollId"
                }
            ),
            "{}",
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
        multi_column["entities"][0]["semanticModelInfo"]["value"]["relationships"][0]["fromColumns"] =
            serde_json::json!(["a", "b"]);
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
        assert!(
            matches!(&error, InvalidEndpoint::CredentialsInUrl { given } if given.to_string() == "http://127.0.0.1:9002"),
            "{error:?}"
        );
    }

    /// `security.outbound.transport_anchors` (`github.com/telekom/sutura#125`) over a REAL TLS
    /// handshake - "ports get fakes, not mocked HTTP". The `FakeServer` above serves plaintext
    /// loopback, which cannot exercise the anchors fold / verification at all; these three cells
    /// dial a real `rustls` loopback server through the reader's OWN `ureq` agent built from a
    /// declared bundle, and assert the SHAPE of a real handshake outcome (read completes, or is
    /// refused). The leaf is a self-signed IP-SAN cert so server-name verification works against the
    /// IP literal this file dials - see `tls_anchors::issue`.
    ///
    /// **RED/GREEN.** `a_declared_bundle_is_trusted_and_the_read_completes` goes red if the
    /// constructor drops its `.tls_config(..)` (the compiled-in roots refuse the self-signed peer);
    /// `a_declared_bundle_still_refuses_an_issuer_it_does_not_name` goes red if the fold trusts a
    /// foreign CA instead of the declared one; `absent_anchors_are_the_compiled_in_default_and_refuse_a_self_signed_peer`
    /// goes red if an absent declaration is treated as "trust the leaf".
    /// The TLS loopback harness itself (`Issued`, `server_config`, `Scratch`, `TlsFakeServer`) now
    /// lives in `sutura_http_client::tls_test_support`, shared with
    /// `sutura-catalog-openmetadata`'s identical `tls_anchors` module since issue #970's review
    /// found the two byte-for-byte the same (`cargo xtask check-jscpd`). What stays here is this
    /// crate's own reader constructor and every `#[test]`.
    mod tls_anchors {
        use sutura_http_client::tls_test_support::{Issued, Scratch, TlsFakeServer, declared_anchors, issue};

        use super::{DEPLOYMENT_PROPERTY, GENEROUS_CAP, bounds, http_cause, token};
        use sutura_catalog_datahub::AspectReader as _;
        use sutura_catalog_datahub::http::{Endpoint, HttpAspectReader, HttpReaderError};
        use sutura_catalog_datahub::test_support::happy_path_answers;

        fn declared(scratch: &Scratch, name: &str, issued: &Issued) -> sutura_tls::LoadedAnchors {
            declared_anchors(scratch, name, issued)
        }

        fn reader(endpoint: &str, anchors: Option<sutura_tls::LoadedAnchors>) -> HttpAspectReader {
            HttpAspectReader::new(
                Endpoint::parse(endpoint).expect("an https loopback endpoint is usable"),
                String::from(DEPLOYMENT_PROPERTY),
                token(),
                bounds(10, GENEROUS_CAP),
                anchors,
            )
        }

        #[test]
        fn a_declared_bundle_is_trusted_and_the_read_completes() {
            let scratch = Scratch::new("trusted");
            let issued = issue();
            let server = TlsFakeServer::start(&issued, happy_path_answers());
            let instance = reader(&server.endpoint(), Some(declared(&scratch, "root", &issued)));
            instance
                .read()
                .expect("a peer signed by the declared bundle is trusted and its pages read");
        }

        #[test]
        fn a_declared_bundle_still_refuses_an_issuer_it_does_not_name() {
            let scratch = Scratch::new("foreign-issuer");
            let presented = issue();
            let declared_ca = issue();
            // The full happy-path corpus, not an empty body: an empty response fails to decode as
            // JSON regardless of whether the handshake was trusted, so it cannot tell "refused at
            // the handshake" apart from "trusted, then failed to parse" - a verification bug that
            // let the wrong issuer through would go unnoticed. Real pages make the read SUCCEED if
            // the handshake wrongly trusts this peer, so only a genuine refusal turns this red.
            let server = TlsFakeServer::start(&presented, happy_path_answers());
            let instance = reader(&server.endpoint(), Some(declared(&scratch, "declared-root", &declared_ca)));
            let error = instance
                .read()
                .expect_err("a chain signed by an issuer the declared bundle does not name is refused");
            // A bare `expect_err` would also pass on a malformed body read over a TRUSTED
            // connection (a JSON-decode error is still an `Err`), which proves nothing about
            // TRUST. Assert the refusal is the handshake's own `Unreachable { cause: Io(..) }`
            // shape and that the io error names the certificate, the way `sutura-exec-bigquery`'s
            // `wire/tests/tls.rs` cells hold their own trust refusals.
            let cause = http_cause(&error);
            let HttpReaderError::Unreachable { cause: io_cause, .. } = cause else {
                panic!("a TLS trust refusal reaches this reader as Unreachable, got: {cause}");
            };
            assert!(
                matches!(io_cause.as_ref(), ureq::Error::Io(_)),
                "a trust refusal is the handshake's own io-layer error, not a decode error: {io_cause}"
            );
            assert!(
                io_cause.to_string().contains("certificate"),
                "the io error must name the certificate refusal, got: {io_cause}"
            );
        }

        #[test]
        fn absent_anchors_are_the_compiled_in_default_and_refuse_a_self_signed_peer() {
            let issued = issue();
            // See the sibling cell above for why this is the full corpus rather than an empty body.
            let server = TlsFakeServer::start(&issued, happy_path_answers());
            let instance = reader(&server.endpoint(), None);
            let error = instance
                .read()
                .expect_err("the compiled-in roots refuse a self-signed loopback peer");
            // Same variant assertion as the sibling cell above: only a trust refusal, never a
            // decode error, may satisfy this cell.
            let cause = http_cause(&error);
            let HttpReaderError::Unreachable { cause: io_cause, .. } = cause else {
                panic!("a TLS trust refusal reaches this reader as Unreachable, got: {cause}");
            };
            assert!(
                matches!(io_cause.as_ref(), ureq::Error::Io(_)),
                "a trust refusal is the handshake's own io-layer error, not a decode error: {io_cause}"
            );
            assert!(
                io_cause.to_string().contains("certificate"),
                "the io error must name the certificate refusal, got: {io_cause}"
            );
        }
    }
}
