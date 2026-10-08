#![forbid(unsafe_code)]
//! The real [`HttpSnapshotReader`] against a real local HTTP server - "ports get fakes, not mocked
//! HTTP" - serving the shapes `docs/what-openmetadata-can-carry.md`'s field-by-field table
//! describes. No mock-HTTP crate: a hand-rolled `TcpListener` loop answering one scripted response
//! per request, in order.
//!
//! **The fake server and the happy-path pages live in `src/test_support.rs`, not here** - the same
//! move `sutura-catalog-datahub`'s suite made, so `sutura-cli`'s owned served-binary suite can build
//! the same fake rather than a second one; see that module's header for why the move is a library
//! concern and not only a test one. What stays HERE is this crate's own test-local scaffolding (a
//! token, a source name, the snapshot-output helpers) and every `#[test]`.
//!
//! `#[cfg(all(test, feature = "http", feature = "fake"))]` on the whole file for three
//! reasons: the `http` feature gates the reader itself, `fake` gates the loopback fake
//! this suite builds against (issue #970's review moved it out of `http` so a shipped binary never
//! carries it), and wrapping the body in `#[cfg(test)] mod tests` is what lets
//! `allow-expect-in-tests`/`allow-panic-in-tests` apply here - the same shape `sutura-catalog-datahub`'s
//! `tests/http_reader.rs` explains.
#![cfg(all(test, feature = "http", feature = "fake"))]

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};

    use sutura_catalog_openmetadata::document::Snapshot;
    use sutura_catalog_openmetadata::http::{
        DEFAULT_MAX_ENTITIES, DEFAULT_PAGE_SIZE, Endpoint, HttpReaderError, HttpSnapshotReader, InvalidEndpoint, PageLimits,
        PagingRefusal, ReadBounds,
    };
    use sutura_catalog_openmetadata::test_support::{FakeServer, Scripted, happy_path_answers, tables_page};
    use sutura_catalog_openmetadata::{OpenMetadataCatalog, OpenMetadataError, SnapshotReader as _};
    use sutura_domain::identity::Secret;
    use sutura_domain::model::SourceName;

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

    fn reader(server: &FakeServer, timeout_seconds: u64, cap: u64) -> HttpSnapshotReader {
        HttpSnapshotReader::new(
            Endpoint::parse(&server.endpoint()).expect("a loopback fake server's own endpoint is a usable one"),
            token(),
            bounds(timeout_seconds, cap),
            // No declared `security.outbound` in the plaintext-loopback cases of this file - the
            // fake answers over `http://`, and the anchors arm of the constructor is exercised by
            // this file's own `tests::tls_anchors` cells.
            None,
        )
    }

    fn http_cause(error: &OpenMetadataError) -> &HttpReaderError {
        let OpenMetadataError::Read(cause) = error else {
            panic!("a reader failure reaches OpenMetadataCatalog as OpenMetadataError::Read: {error}");
        };
        cause
            .downcast_ref::<HttpReaderError>()
            .unwrap_or_else(|| panic!("the boxed cause is this reader's own error type: {cause}"))
    }

    /// **The bearer is sent on every one of the two requests.**
    ///
    /// RED/GREEN mutation: delete `HttpSnapshotReader::bearer`'s call site in `fetch` - the fake
    /// server still answers (it does not itself check the header), but every captured
    /// `authorization` becomes `None` and this assertion goes red.
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
        drop(read.expect("two well-formed pages read"));
        assert_eq!(seen.len(), 2, "one request per entity kind");
        for request in seen {
            assert_eq!(
                request.authorization(),
                Some("Bearer pat-under-test"),
                "every request carries the same bearer"
            );
        }
    }

    /// **The exact request each read makes, byte for byte - not merely that a request happened.**
    /// The round-2 review measured that a suite asserting on `authorization` alone cannot see a
    /// query-parameter regression: deleting `http.rs`'s own `fields_param` construction left every
    /// test in this file green, because none of them read the request line. This cell does.
    ///
    /// RED/GREEN mutation: delete the `fields_param` construction in `HttpSnapshotReader::fetch`
    /// (or its `&fields_param` interpolation into the URL) - the tables request would then carry
    /// no `fields=` at all, and this assertion goes red.
    #[test]
    fn the_tables_and_metrics_requests_carry_their_exact_query_parameters() {
        let server = FakeServer::start(happy_path_answers());
        let read = reader(&server, 10, GENEROUS_CAP).read();
        let seen = server.finish();
        drop(read.expect("two well-formed pages read"));
        assert_eq!(seen.len(), 2, "one request per entity kind");
        assert_eq!(
            seen[0].request_line(),
            "GET /api/v1/tables?limit=1000&fields=columns,tableConstraints HTTP/1.1",
            "the tables request must ask for the two relationship-backed fields TableResource \
             only populates when named"
        );
        assert_eq!(
            seen[1].request_line(),
            "GET /api/v1/metrics?limit=1000 HTTP/1.1",
            "the metrics request needs no `fields=` - MetricResource returns metricType/\
             granularity/metricExpression/measures unconditionally"
        );
    }

    /// **A 401 is a typed refusal, and `OpenMetadata`'s own response text never reaches `Display` or
    /// `Debug`.**
    ///
    /// RED/GREEN mutation: have the `#[error(...)]` on `HttpReaderError::Refused` interpolate
    /// `detail` - this assertion catches the marker text reaching either rendering.
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
        assert_eq!(*entity, "tables");
        assert_eq!(*status, 401);
        assert!(
            !format!("{cause}").contains(MARKER),
            "Display must not carry OpenMetadata's own text"
        );
        assert!(
            !format!("{cause:?}").contains(MARKER),
            "Debug must not carry OpenMetadata's own text"
        );
    }

    /// **A response over the declared cap is refused, not decoded.**
    ///
    /// RED/GREEN mutation: delete the length check in `HttpSnapshotReader::fetch` - the oversized
    /// body (still within `ureq`'s own generous backstop limit) would then reach the JSON decode,
    /// and this assertion goes red.
    #[test]
    fn a_response_over_the_cap_is_refused() {
        const CAP: u64 = 16;
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
                    entity: "tables",
                    cap: CAP
                }
            ),
            "expected TooLarge{{entity: \"tables\", cap: {CAP}}}, got: {}",
            http_cause(&error)
        );
    }

    /// **The decoded snapshot carries the same models, join and metric as the recorded fixture.**
    ///
    /// The HTTP-read page maps into the same two models, the same one-to-many join and the same
    /// reported-not-defined metric the recorded fixture serves. Compared field by field rather than
    /// as whole structs: the wire naturally carries richer column `dataType`/`description`
    /// evidence than the hand-minimal recorded corpus, so whole-struct equality would be a false
    /// claim about the wire and would couple these fixtures to each other byte-for-byte.
    #[test]
    fn the_decoded_snapshot_carries_the_same_models_join_and_metric_as_the_recorded_fixture() {
        let server = FakeServer::start(happy_path_answers());
        let snapshot = reader(&server, 10, GENEROUS_CAP).read().expect("the happy path reads");
        drop(server.finish());

        let recorded = sutura_catalog_openmetadata::fixture::FixtureReader
            .read()
            .expect("the recorded fixture reads");
        // Same tables, in the same order, same columns.
        let model_names = |snap: &Snapshot| {
            snap.tables()
                .iter()
                .map(|t| (t.name().to_owned(), t.columns().to_vec()))
                .collect::<Vec<_>>()
        };
        assert_eq!(model_names(&snapshot), model_names(&recorded));
        // Same relationship, same endpoints and cardinality.
        let relationship = |snap: &Snapshot| {
            let pair = snap.relationships().next().expect("one relationship");
            (
                pair.1.origin_model().to_owned(),
                pair.1.origin_column().to_owned(),
                pair.1.target_model().to_owned(),
                pair.1.target_column().to_owned(),
                pair.1.relationship_type().map(|r| r as u8),
            )
        };
        assert_eq!(relationship(&snapshot), relationship(&recorded));
        // Same metric, by name and aggregation.
        let metric = |snap: &Snapshot| {
            (
                snap.metrics()[0].name().to_owned(),
                snap.metrics()[0].aggregation().to_owned(),
            )
        };
        assert_eq!(metric(&snapshot), metric(&recorded));
    }

    /// **The decoded snapshot certifies the same bundle the recorded fixture does.**
    ///
    /// This is issue #970's own question answered over the wire instead of over a recorded string:
    /// the served pages map into an `OpenMetadataCatalog` whose loaded bundle is the one
    /// `crates/sutura-app/tests/adapters/adapters.rs` measures the recorded corpus against.
    #[test]
    fn the_wire_read_bundle_loads() {
        let server = FakeServer::start(happy_path_answers());
        let mut sources = std::collections::BTreeMap::new();
        drop(sources.insert(String::from("warehouse"), source_name()));
        let bundle = OpenMetadataCatalog::new(source_name(), version(), sources, reader(&server, 10, GENEROUS_CAP))
            .load()
            .expect("the wire-read snapshot assembles into a bundle");
        drop(server.finish());

        assert_eq!(
            bundle.definitions().models().len(),
            2,
            "the wire-read snapshot supplies two models"
        );
    }

    /// **One shared deadline across the (up to) two requests, not one per request.**
    ///
    /// A one-second budget and a first response delayed past it: the second request (`metrics`)
    /// must never be attempted at all, because nothing is left of the shared budget - it is refused
    /// as `DeadlineSpent`, not as a slow-but-independent second timeout.
    ///
    /// RED/GREEN mutation: in `HttpSnapshotReader::fetch`, replace `budget.remaining()` with
    /// `Some(self.bounds.timeout())` - the pre-flight `DeadlineSpent` check can no longer fire, and
    /// this test's variant assertion goes red.
    #[test]
    fn the_shared_deadline_is_honoured_across_requests() {
        let server = FakeServer::start(vec![Scripted::delayed(&tables_page(), Duration::from_millis(1200))]);
        let started = std::time::Instant::now();
        let error = reader(&server, 1, GENEROUS_CAP)
            .read()
            .expect_err("the shared budget is spent");
        let elapsed = started.elapsed();
        assert!(
            matches!(http_cause(&error), HttpReaderError::DeadlineSpent { entity: "metrics", .. }),
            "expected DeadlineSpent naming metrics, got: {}",
            http_cause(&error)
        );
        assert!(
            elapsed < Duration::from_secs(3),
            "a spent budget is refused promptly rather than waited out again: {elapsed:?}"
        );
    }

    /// The page size the paging cells force, so a few tables span several pages.
    fn two_per_page() -> PageLimits {
        PageLimits::parse(2, DEFAULT_MAX_ENTITIES).expect("a page size of two is usable")
    }

    /// One `tables` page of bare tables named `names`, with `after` as the cursor of the next page.
    fn tables_of(names: &[&str], after: Option<&str>, total: u64) -> serde_json::Value {
        let data: Vec<_> = names
            .iter()
            .map(|name| {
                serde_json::json!({
                    "name": name,
                    "fullyQualifiedName": format!("warehouse.default.sales.{name}"),
                    "columns": [{"name": "id", "dataType": "STRING"}],
                })
            })
            .collect();
        serde_json::json!({"data": data, "paging": {"total": total, "after": after}})
    }

    /// One `metrics` page of bare metrics named `names`, with `after` as the cursor of the next page.
    fn metrics_of(names: &[String], after: Option<&str>) -> serde_json::Value {
        let data: Vec<_> = names
            .iter()
            .map(|name| serde_json::json!({"name": name, "metricType": "COUNT"}))
            .collect();
        serde_json::json!({"data": data, "paging": {"after": after}})
    }

    fn reader_of(server: &FakeServer, limits: PageLimits) -> HttpSnapshotReader {
        reader(server, 10, GENEROUS_CAP).with_page_limits(limits)
    }

    fn paging_cause(error: &OpenMetadataError) -> (&'static str, PagingRefusal) {
        match http_cause(error) {
            HttpReaderError::Paging { entity, cause } => (*entity, *cause),
            other => panic!("expected a paging refusal, got: {other}"),
        }
    }

    /// **A list of several pages loads every page, for tables and for metrics.** The first request
    /// carries no cursor, each later one carries the `after` the page before it gave - percent-encoded,
    /// because it is the service's own text - and `limit` is the page size.
    ///
    /// RED/GREEN mutation: drop the `.query("after", ..)` in `fetch` - every later request then repeats
    /// the first page's line, and this assertion goes red.
    #[test]
    fn a_list_of_several_pages_loads_every_page() {
        let metric_names: Vec<String> = ["m1", "m2", "m3"].into_iter().map(String::from).collect();
        let server = FakeServer::start(vec![
            Scripted::ok(&tables_of(&["t1", "t2"], Some("c-1"), 5)),
            Scripted::ok(&tables_of(&["t3", "t4"], Some("c-2&x=1"), 5)),
            Scripted::ok(&tables_of(&["t5"], None, 5)),
            Scripted::ok(&metrics_of(&metric_names[..2], Some("m-1"))),
            Scripted::ok(&metrics_of(&metric_names[2..], None)),
        ]);
        let snapshot = reader_of(&server, two_per_page()).read().expect("every page loads");
        let seen = server.finish();
        let tables: Vec<_> = snapshot.tables().iter().map(|table| table.name().to_owned()).collect();
        assert_eq!(tables, ["t1", "t2", "t3", "t4", "t5"]);
        let metrics: Vec<_> = snapshot.metrics().iter().map(|metric| metric.name().to_owned()).collect();
        assert_eq!(metrics, ["m1", "m2", "m3"]);
        let lines: Vec<_> = seen
            .iter()
            .map(sutura_http_client::test_support::CapturedRequest::request_line)
            .collect();
        assert_eq!(
            lines,
            [
                "GET /api/v1/tables?limit=2&fields=columns,tableConstraints HTTP/1.1",
                "GET /api/v1/tables?limit=2&fields=columns,tableConstraints&after=c-1 HTTP/1.1",
                "GET /api/v1/tables?limit=2&fields=columns,tableConstraints&after=c-2%26x%3D1 HTTP/1.1",
                "GET /api/v1/metrics?limit=2 HTTP/1.1",
                "GET /api/v1/metrics?limit=2&after=m-1 HTTP/1.1",
            ]
        );
    }

    /// **A cursor the service gives twice is refused, not followed again.**
    ///
    /// RED/GREEN mutation: delete `Pager::advance`'s repeated-cursor refusal - the second page's
    /// cursor would be followed and this read would not be refused.
    #[test]
    fn a_repeated_cursor_is_refused() {
        let server = FakeServer::start(vec![
            Scripted::ok(&tables_of(&["t1", "t2"], Some("c-1"), 4)),
            Scripted::ok(&tables_of(&["t3", "t4"], Some("c-1"), 4)),
        ]);
        let error = reader_of(&server, two_per_page())
            .read()
            .expect_err("a repeated cursor is refused");
        assert_eq!(
            server.finish().len(),
            2,
            "the second page was read, its cursor was not followed"
        );
        assert_eq!(paging_cause(&error), ("tables", PagingRefusal::RepeatedCursor));
    }

    /// **A page with no entity that still reports more is refused, not followed.**
    ///
    /// RED/GREEN mutation: delete `Pager::advance`'s no-progress refusal - the empty page's fresh
    /// cursor would be followed and this read would not be refused.
    #[test]
    fn a_page_with_no_progress_is_refused() {
        let server = FakeServer::start(vec![
            Scripted::ok(&tables_of(&["t1", "t2"], Some("c-1"), 3)),
            Scripted::ok(&tables_of(&[], Some("c-2"), 3)),
        ]);
        let error = reader_of(&server, two_per_page())
            .read()
            .expect_err("a page with no progress is refused");
        drop(server.finish());
        assert_eq!(paging_cause(&error), ("tables", PagingRefusal::NoProgress));
    }

    /// **A list above the entity bound is refused, never cut short.** One more metric than the bound
    /// across a hundred and one default-sized pages: the refusal names the bound, and no snapshot of
    /// the first hundred thousand comes back.
    ///
    /// RED/GREEN mutation: raise `DEFAULT_MAX_ENTITIES`, delete `Pager::advance`'s bound check, or
    /// answer `Ok(false)` where it refuses - the read then returns a snapshot (or runs the script out)
    /// and this assertion goes red.
    #[test]
    fn a_list_above_the_entity_bound_is_refused_not_truncated() {
        let over = DEFAULT_MAX_ENTITIES + 1;
        let names: Vec<String> = (0..over).map(|n| format!("m{n}")).collect();
        let mut answers = vec![Scripted::ok(&tables_page())];
        let pages: Vec<_> = names.chunks(DEFAULT_PAGE_SIZE).collect();
        for (n, page) in pages.iter().enumerate() {
            let after = (n + 1 < pages.len()).then(|| format!("m-{n}"));
            answers.push(Scripted::ok(&metrics_of(page, after.as_deref())));
        }
        let server = FakeServer::start(answers);
        let error = reader(&server, 60, GENEROUS_CAP)
            .read()
            .expect_err("a list over the bound is refused, not truncated");
        drop(server.finish());
        assert_eq!(
            paging_cause(&error),
            (
                "metrics",
                PagingRefusal::TooManyEntities {
                    max: DEFAULT_MAX_ENTITIES
                }
            )
        );
    }

    /// **A last page that leaves the list short of the reported total is refused, not read as
    /// complete.**
    ///
    /// RED/GREEN mutation: delete `Pager::advance`'s short-of-total refusal - the two tables would
    /// be read as the whole list and this assertion goes red.
    #[test]
    fn a_last_page_short_of_the_reported_total_is_refused() {
        let server = FakeServer::start(vec![Scripted::ok(&tables_of(&["t1", "t2"], None, 3))]);
        let error = reader_of(&server, two_per_page())
            .read()
            .expect_err("a list shorter than its total is refused");
        drop(server.finish());
        assert_eq!(
            paging_cause(&error),
            ("tables", PagingRefusal::ShortOfTotal { read: 2, total: 3 })
        );
    }

    /// **An `after` that is not text is refused by name, not read as the last page.**
    ///
    /// RED/GREEN mutation: map `report`'s `Some(_)` arm to `None` - the page would be read as the
    /// last and this assertion goes red.
    #[test]
    fn an_after_cursor_that_is_not_text_is_refused() {
        let mut page = tables_of(&["t1"], None, 1);
        page["paging"]["after"] = serde_json::json!(7);
        let server = FakeServer::start(vec![Scripted::ok(&page)]);
        let error = reader_of(&server, two_per_page())
            .read()
            .expect_err("a numeric cursor is refused");
        drop(server.finish());
        assert!(
            matches!(
                http_cause(&error),
                HttpReaderError::UnexpectedShape {
                    entity: "tables",
                    field: "paging.after"
                }
            ),
            "{}",
            http_cause(&error)
        );
    }

    /// **A foreign key whose `referredColumns` entry cannot be split into a table and a column is
    /// refused BY NAME, not read as a dangling join.** `referredColumns` carries a fully qualified
    /// name (`service.database.schema.table.column`, exactly five segments); a bare column name
    /// with no `.` at all is nowhere close, and `split_fqn_tail`'s own unit cells cover the
    /// stricter shapes (wrong segment count, an unterminated quote) this integration cell does not
    /// need to repeat over the wire.
    ///
    /// RED/GREEN mutation: replace `split_fqn_tail`'s `.ok_or(UnexpectedShape { field:
    /// "tableConstraints[].referredColumns[0]" })?` in `harvest_relationship` with
    /// `.unwrap_or_default()` - an unparseable referred column would decode into a relationship
    /// with an empty target, and this assertion goes red.
    #[test]
    fn a_relationship_whose_referred_column_has_no_table_is_refused() {
        let mut page = tables_page();
        page["data"][0]["tableConstraints"][0]["referredColumns"] = serde_json::json!(["customer_id"]);
        let server = FakeServer::start(vec![Scripted::ok(&page)]);
        let error = reader(&server, 10, GENEROUS_CAP)
            .read()
            .expect_err("a referred column with no table segment is refused");
        drop(server.finish());
        assert!(
            matches!(
                http_cause(&error),
                HttpReaderError::UnexpectedShape {
                    entity: "tables",
                    field: "tableConstraints[].referredColumns[0]"
                }
            ),
            "expected UnexpectedShape{{entity: \"tables\", field: \"tableConstraints[].referredColumns[0]\"}}, got: {}",
            http_cause(&error)
        );
    }

    /// **A relationship declaring more than one column per side is refused BY NAME, not silently
    /// narrowed to the first.** The crate's own shape carries one column per side, so a side
    /// declaring more than one is refused rather than reduced to a guess.
    #[test]
    fn a_relationship_with_more_than_one_column_per_side_is_refused() {
        let mut page = tables_page();
        page["data"][0]["tableConstraints"][0]["columns"] = serde_json::json!(["a", "b"]);
        let server = FakeServer::start(vec![Scripted::ok(&page)]);
        let error = reader(&server, 10, GENEROUS_CAP)
            .read()
            .expect_err("a multi-column relationship side is refused");
        drop(server.finish());
        assert!(
            matches!(
                http_cause(&error),
                HttpReaderError::UnexpectedShape {
                    entity: "tables",
                    field: "columns"
                }
            ),
            "expected UnexpectedShape{{entity: \"tables\", field: \"columns\"}}, got: {}",
            http_cause(&error)
        );
    }

    /// A body that is not UTF-8 is refused as `Unreadable`, naming the `tables` entity.
    #[test]
    fn a_body_that_cannot_be_read_is_refused() {
        let server = FakeServer::start(vec![Scripted::raw(200, vec![0xFF, 0xFE, 0xFD])]);
        let error = reader(&server, 10, GENEROUS_CAP)
            .read()
            .expect_err("an unreadable body is refused");
        drop(server.finish());
        let cause = http_cause(&error);
        assert!(
            matches!(cause, HttpReaderError::Unreadable { entity: "tables", .. }),
            "expected Unreadable naming tables, got: {cause}"
        );
    }

    /// A 200 whose body is not JSON is refused as `NotADocument`, naming the `tables` entity.
    #[test]
    fn a_non_json_response_is_refused() {
        let server = FakeServer::start(vec![Scripted::raw(200, b"<html>not json</html>".to_vec())]);
        let error = reader(&server, 10, GENEROUS_CAP)
            .read()
            .expect_err("a non-JSON page is refused");
        drop(server.finish());
        let cause = http_cause(&error);
        assert!(
            matches!(cause, HttpReaderError::NotADocument { entity: "tables", .. }),
            "expected NotADocument naming tables, got: {cause}"
        );
    }

    /// A table constraint whose `relationshipType` is none of the known ones is refused as
    /// `NotTheCanonicalShape`, naming the `tables` entity.
    #[test]
    fn a_relationship_type_that_is_not_a_known_variant_is_refused() {
        let mut page = tables_page();
        page["data"][0]["tableConstraints"][0]["relationshipType"] = serde_json::json!("GARBAGE");
        let server = FakeServer::start(vec![Scripted::ok(&page)]);
        let error = reader(&server, 10, GENEROUS_CAP)
            .read()
            .expect_err("an unknown relationshipType is refused");
        drop(server.finish());
        let cause = http_cause(&error);
        assert!(
            matches!(cause, HttpReaderError::NotTheCanonicalShape { entity: "tables", .. }),
            "expected NotTheCanonicalShape naming tables, got: {cause}"
        );
    }

    /// A userinfo endpoint is refused at PARSE - [`Endpoint::parse`] returns
    /// [`InvalidEndpoint::CredentialsInUrl`] before any `HttpSnapshotReader` can be built, so no
    /// `HttpSnapshotReader` exists to make a call.
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
    /// handshake - "ports get fakes, not mocked HTTP". These cells dial a real `rustls` loopback
    /// server through the reader's OWN `ureq` agent built from a declared bundle. The leaf is a
    /// self-signed IP-SAN cert so server-name verification works against the IP literal this file
    /// dials.
    ///
    /// **RED/GREEN.** `a_declared_bundle_is_trusted_and_the_read_completes` goes red if the
    /// constructor drops its `.tls_config(..)`; `a_declared_bundle_still_refuses_an_issuer_it_does_not_name`
    /// goes red if the fold trusts a foreign CA instead of the declared one; and
    /// `absent_anchors_are_the_compiled_in_default_and_refuse_a_self_signed_peer` goes red if an
    /// absent declaration is treated as "trust the leaf".
    /// The TLS loopback harness itself (`Issued`, `server_config`, `Scratch`, `TlsFakeServer`) now
    /// lives in `sutura_http_client::tls_test_support`, shared with
    /// `sutura-catalog-datahub`'s identical `tls_anchors` module since issue #970's review found
    /// the two byte-for-byte the same (`cargo xtask check-jscpd`). What stays here is this crate's
    /// own reader constructor and every `#[test]`.
    mod tls_anchors {
        use sutura_http_client::tls_test_support::{Issued, Scratch, TlsFakeServer, declared_anchors, issue};

        use super::{GENEROUS_CAP, bounds, http_cause, token};
        use sutura_catalog_openmetadata::SnapshotReader as _;
        use sutura_catalog_openmetadata::http::{Endpoint, HttpReaderError, HttpSnapshotReader};
        use sutura_catalog_openmetadata::test_support::happy_path_answers;

        fn declared(scratch: &Scratch, name: &str, issued: &Issued) -> sutura_tls::LoadedAnchors {
            declared_anchors(scratch, name, issued)
        }

        fn reader(endpoint: &str, anchors: Option<sutura_tls::LoadedAnchors>) -> HttpSnapshotReader {
            HttpSnapshotReader::new(
                Endpoint::parse(endpoint).expect("an https loopback endpoint is usable"),
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
            let server = TlsFakeServer::start(&presented, happy_path_answers());
            let instance = reader(&server.endpoint(), Some(declared(&scratch, "declared-root", &declared_ca)));
            let error = instance
                .read()
                .expect_err("a chain signed by an issuer the declared bundle does not name is refused");
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
            let server = TlsFakeServer::start(&issued, happy_path_answers());
            let instance = reader(&server.endpoint(), None);
            let error = instance
                .read()
                .expect_err("the compiled-in roots refuse a self-signed loopback peer");
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
