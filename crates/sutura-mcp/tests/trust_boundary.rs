//! Trust-boundary tests for `sutura-mcp`'s request entry point: the wire-level refusal codes for
//! `ask_metric`'s over-cap counts and `run_sql`'s oversized statement, and parse-error refusals
//! that were reachable through the MCP handler but untested there.
//!
//! Every test drives `AgentSurface` through `rmcp`'s OWN client over an in-memory pipe (the
//! `connected` harness), or - for the `Asking::PerRequest` no-caller case - through a hand-built
//! `RequestContext` over a real `Peer` a throwaway handshake produced. No test here reuses the
//! crate's `#[cfg(test)]` `testing` module: the fakes below are this file's own, so a cell that
//! passed by an accidental shared-state echo rather than by the bound under test cannot.
//!
//! # Conformance pins, not red-on-base
//!
//! Every test here pins behaviour the base tree (post-#1089) already provides, now proven through
//! the real handler rather than only at a lower layer: the count refusals
//! (`too_many_metrics`/`too_many_dimensions`/`too_many_filters`) are #1089's `RefusalReason`s,
//! returned with the same code whether `sutura_domain::question::parse_query` refuses them after
//! `from_value` or `server::bounds::refused_for_over_cap` refuses them first - nothing here measures
//! which of the two actually fired, only that the caller sees the right code. The statement-size
//! bound DOES distinguish the two paths, by message: `server::bounds::oversized_statement` renders
//! "larger than", `RawStatement::parse`'s own `TooLong` renders "not a statement", so that one test
//! also pins which check fires first. The remaining tests pin existing parse-error refusals
//! (`-32602`/`-32600`) that were reachable through the handler but untested there.
//!
//! Each cell is declared with `Claim-Cell` and a killing mutation under `devco/claim-mutations/`:
//! the mutation breaks the production behaviour the cell pins, and the cell's assertion fails.

#![forbid(unsafe_code)]

// `cfg(test)` because clippy only honours `allow-expect-in-tests` for code inside a
// `#[cfg(test)]` item, and `tests_outside_test_module` wants the `#[test]`/`#[tokio::test]`
// functions there too - the same wrapper `crates/sutura-mcp/tests/shared_question_conversion.rs`
// carries for the identical reason. An integration test target is compiled with `--test`, so
// the gate is true here and nothing below is conditional in practice.
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use rmcp::model::{CallToolRequestParams, ErrorCode, NumberOrString};
    use rmcp::service::{Peer, RequestContext, RoleClient, RoleServer, RunningService, ServiceError};
    use rmcp::{ServerHandler, serve_client, serve_server};
    use sutura_app::prompt::{CatalogProse, Tool};
    use sutura_app::surface::LocalService;
    use sutura_app::{Capability, Permitted, Warehouses};
    use sutura_config::{Environment, RequestTimeout, Settings, Sources};
    use sutura_domain::audit::AuditSink;
    use sutura_domain::calendar::{Date, TimeRange};
    use sutura_domain::capabilities::MetadataCapabilities;
    use sutura_domain::catalog::{
        Anchor, AnchorValue, Audience, Definitions, Description, Dimension, DimensionValue, Metric, Model,
    };
    use sutura_domain::identity::{
        CredentialBroker, CredentialsDoNotCoverThePlan, Expiry, LegCredentials, Minted, Presented,
        RequestContext as PrincipalContext, SourceSet,
    };
    use sutura_domain::knowledge::Knowledge;
    use sutura_domain::measure::{AggregatedColumn, Measure, Term};
    use sutura_domain::model::Aggregate;
    use sutura_domain::model::{ColumnName, DimensionName, Grain, MetricName, ModelName, SourceName, TableName};
    use sutura_domain::pinned::{
        CatalogKind, Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions, SemanticCatalog,
    };
    use sutura_domain::plan::{Executable, RefusingCombiner};
    use sutura_domain::source::{AcknowledgementReason, ImpersonationCapability, SharedIdentityDeclared, SourcePosture};
    use sutura_domain::warehouse::deadline::Deadline;
    use sutura_domain::warehouse::{AnchorRows, PreFlight, ResultBatches, RowSet, Value, Warehouse};
    use sutura_runtime::Admission;

    use sutura_mcp::{AgentSurface, Asking};

    // ----------------------------------------------------------- fakes for the ports ---

    /// The number the anchor certifies and the fake warehouse reproduces.
    const ANCHORED_VALUE: i64 = 197_122;

    #[derive(Debug, thiserror::Error)]
    #[error("a fixture cannot fail")]
    struct Unreachable;

    fn source() -> SourceName {
        SourceName::parse("local").expect("a test source is a source")
    }

    fn description(raw: &str) -> Description {
        Description::parse(raw).expect("a test description is a description")
    }

    fn column(raw: &str) -> ColumnName {
        ColumnName::parse(raw).expect("a test column is a column")
    }

    fn june() -> TimeRange {
        TimeRange::new(
            Date::parse("2026-06-01").expect("a test date is a date"),
            Date::parse("2026-07-01").expect("a test date is a date"),
        )
        .expect("June is a range")
    }

    /// One model, one anchored metric, one filterable dimension - the smallest bundle that validates.
    fn bundle() -> PinnedDefinitions {
        let model = Model::new(
            ModelName::parse("orders").expect("a test model is a model"),
            source(),
            TableName::parse("orders").expect("a test table is a table"),
            BTreeSet::from([column("amount_cents"), column("order_date"), column("region")]),
            description("Orders, one row per order."),
        );
        let region = Dimension::new(
            DimensionName::parse("region").expect("a test dimension is a dimension"),
            column("region"),
            None,
            Some(BTreeSet::from([
                DimensionValue::parse("north").expect("a test value is a value"),
                DimensionValue::parse("south").expect("a test value is a value"),
            ])),
            description("Sales region."),
        );
        let revenue = Metric::new(
            MetricName::parse("revenue").expect("a test metric is a metric"),
            ModelName::parse("orders").expect("a test model is a model"),
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
            Vec::new(),
            column("order_date"),
            BTreeSet::from([Grain::Day, Grain::Month]),
            vec![region],
            Some(Anchor::new(
                june(),
                AnchorValue::parse(ANCHORED_VALUE.to_string()).expect("a test anchor value is a value"),
            )),
            description("Revenue, in minor units."),
            Audience::Open,
        )
        .expect("one dimension cannot duplicate another");
        let definitions = Definitions::assemble(vec![model], vec![], vec![revenue]).expect("the test bundle is consistent");
        PinnedDefinitions::pin(
            DefinitionVersion::parse("test-1").expect("a test version is a version"),
            definitions.clone(),
            Knowledge::none(),
            ContributionManifest::single(
                source(),
                Contribution::of(MetadataCapabilities::produced(&definitions, &Knowledge::none())),
            ),
        )
        .expect("the test definitions hash")
    }

    /// A golden catalog that loads the fixture bundle.
    struct FixedCatalog;

    impl SemanticCatalog for FixedCatalog {
        type Error = Unreachable;

        const KIND: CatalogKind = CatalogKind::Golden;

        fn capabilities() -> MetadataCapabilities {
            MetadataCapabilities::everything()
        }

        fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
            Ok(bundle())
        }
    }

    fn canned(rows: &RowSet) -> ResultBatches {
        sutura_domain::warehouse::arrow::of_row_set(rows).expect("a row set's width invariant is the only failure this has")
    }

    fn declared_shared() -> SharedIdentityDeclared {
        SharedIdentityDeclared::of(
            AcknowledgementReason::parse("a transport-layer fake over no data system, in this process")
                .expect("a fixture reason is a reason"),
        )
    }

    fn shared_posture() -> SourcePosture {
        SourcePosture::SharedServiceUser {
            declared: declared_shared(),
        }
    }

    /// A data system that answers every statement with the anchored value, so the bundle validates.
    #[derive(Debug)]
    struct FakeWarehouse {
        src: SourceName,
        posture: SourcePosture,
        result: RowSet,
    }

    impl Warehouse for FakeWarehouse {
        type Error = Unreachable;

        const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

        fn source(&self) -> &SourceName {
            &self.src
        }

        fn posture(&self) -> &SourcePosture {
            &self.posture
        }

        fn dry_run(
            &self,
            _executable: Executable<'_>,
            _presented: &Presented,
            _deadline: Deadline,
        ) -> Result<PreFlight, Self::Error> {
            Ok(PreFlight::NotAsked)
        }

        fn execute(
            &self,
            _executable: Executable<'_>,
            _presented: &Presented,
            _deadline: Deadline,
        ) -> Result<ResultBatches, Self::Error> {
            Ok(canned(&self.result))
        }

        fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
            Ok(AnchorRows::of(self.result.clone()))
        }
    }

    fn fake_warehouse() -> Warehouses<FakeWarehouse> {
        Warehouses::of(FakeWarehouse {
            src: source(),
            posture: shared_posture(),
            result: RowSet::new(vec![String::from("revenue")], vec![vec![Value::Integer(ANCHORED_VALUE)]])
                .expect("a one-cell result is a result set"),
        })
    }

    #[derive(Debug, thiserror::Error)]
    #[error("the fixture broker minted a set that does not cover the plan")]
    struct FixtureBrokerDefect {
        #[source]
        cause: CredentialsDoNotCoverThePlan,
    }

    /// A broker that grants the shared posture for whatever it is asked about.
    struct GrantsTheSharedIdentity;

    impl CredentialBroker for GrantsTheSharedIdentity {
        type Error = FixtureBrokerDefect;

        fn mint(&self, context: &PrincipalContext, sources: &SourceSet) -> Result<Minted, Self::Error> {
            let mut presented = BTreeMap::new();
            for name in sources.iter() {
                drop(presented.insert(
                    name.clone(),
                    Presented::SharedServiceUser {
                        declared: declared_shared(),
                    },
                ));
            }
            LegCredentials::minted(context.chain().subject().clone(), Expiry::NothingExpires, sources, presented)
                .map(|credentials| Minted::Granted { credentials })
                .map_err(|cause| FixtureBrokerDefect { cause })
        }
    }

    /// An audit sink that counts records, so a no-caller refusal can be shown to write none.
    #[derive(Default)]
    struct CountingSink {
        calls: std::sync::atomic::AtomicUsize,
    }

    impl CountingSink {
        fn calls(&self) -> usize {
            self.calls.load(std::sync::atomic::Ordering::Relaxed)
        }
    }

    impl AuditSink for CountingSink {
        fn record(&self, _record: &sutura_domain::audit::CallRecord<'_>) {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    type CertifiedService = LocalService<FakeWarehouse, Arc<CountingSink>, GrantsTheSharedIdentity, RefusingCombiner>;

    fn certified_service() -> (CertifiedService, Arc<CountingSink>) {
        let sink = Arc::new(CountingSink::default());
        let service = LocalService::start(
            &FixedCatalog,
            fake_warehouse(),
            Arc::clone(&sink),
            GrantsTheSharedIdentity,
            RefusingCombiner,
            1 << 30,
        )
        .expect("the fixture bundle validates");
        (service, sink)
    }

    // --------------------------------------------------------------- the harness ---

    fn settings(overlay: &str) -> Settings {
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay)).expect("the test settings load")
    }

    fn admission() -> Admission {
        Admission::from_settings(settings("").runtime())
    }

    fn reply() -> RequestTimeout {
        settings("").server().request_timeout()
    }

    /// A fixture for [`AgentSurface`]'s `tools` field: every certified operation, so a call this file
    /// makes is never refused for naming a tool the surface does not carry.
    fn tools() -> Arc<[Tool]> {
        Arc::from(Tool::ALL)
    }

    /// A client and a server joined by an in-memory pipe, the peer permitted everything.
    async fn connected(service: CertifiedService) -> RunningService<RoleClient, ()> {
        let (client_side, server_side) = tokio::io::duplex(64 * 1024);
        let server = serve_server(
            AgentSurface::new(
                Arc::new(service),
                Asking::TheProcessOwner {
                    permitted: Permitted::every_capability(),
                },
                CatalogProse::Quoted,
                admission(),
                reply(),
                tools(),
                None,
            ),
            server_side,
        );
        let client = serve_client((), client_side);
        let (server, client) = tokio::join!(server, client);
        let running = server.expect("the throwaway handshake's server initializes");
        drop(tokio::spawn(async move {
            drop(running.waiting().await);
        }));
        client.expect("the throwaway handshake's client initializes")
    }

    fn call(name: &'static str, arguments: &serde_json::Value) -> CallToolRequestParams {
        let object = arguments.as_object().cloned().unwrap_or_default();
        CallToolRequestParams::new(name).with_arguments(object)
    }

    fn ask(arguments: &serde_json::Value) -> CallToolRequestParams {
        call(Capability::AskMetric.id(), arguments)
    }

    fn raw(arguments: &serde_json::Value) -> CallToolRequestParams {
        call(Capability::RunSql.id(), arguments)
    }

    fn describe() -> CallToolRequestParams {
        call(Capability::DescribeCatalog.id(), &serde_json::json!({}))
    }

    /// Pulls the JSON-RPC error code out of a failed `call_tool`.
    fn error_code(err: &ServiceError) -> ErrorCode {
        let ServiceError::McpError(data) = err else {
            panic!("expected a protocol error, got {err:?}");
        };
        data.code
    }

    /// Pulls the JSON-RPC error message out of a failed `call_tool`.
    fn error_message(err: &ServiceError) -> &str {
        let ServiceError::McpError(data) = err else {
            panic!("expected a protocol error, got {err:?}");
        };
        &data.message
    }

    /// Asserts a tool result is a refusal with the given code, through `Ok` (not a protocol error).
    ///
    /// `#[track_caller]` so a failing assertion here reports the CALLING test's own line, not this
    /// helper's - `just causality`'s claim-mutation check reads that site to tell a cell's own
    /// assertion from a panic elsewhere, and a shared helper with no `#[track_caller]` reports itself.
    #[track_caller]
    fn assert_refusal(result: &rmcp::model::CallToolResult, expected_code: &str) {
        assert_ne!(result.is_error, Some(true), "a refusal is not an error: {result:?}");
        let structured = result
            .structured_content
            .as_ref()
            .expect("a refusal carries structured content");
        assert_eq!(
            structured.get("outcome").and_then(serde_json::Value::as_str),
            Some("refusal"),
            "{structured:?}"
        );
        let reason = structured.get("reason").expect("a refusal carries a reason");
        assert_eq!(
            reason.get("code").and_then(serde_json::Value::as_str),
            Some(expected_code),
            "{reason:?}"
        );
    }

    // ------------------------------------------------------- UNBOUNDED: over-cap ---

    /// Over-cap `metrics` is refused as `too_many_metrics` over the wire.
    ///
    /// **Not renamed** despite the name: it claims "before allocation", but this pins only that the
    /// caller sees the right code, not the ordering. Whichever of
    /// `server::bounds::refused_for_over_cap` or the downstream `parse_query` actually fires,
    /// `outcome: "refusal"` and `code: "too_many_metrics"` are the same either way (confirmed by
    /// review 5335886314: deleting the bound, and moving `from_value` ahead of it, both leave this
    /// green). This fn's `Claim-Cell:` trailer is on an already-pushed, non-tip commit and the
    /// bijection is range-wide, so renaming would orphan it; the corrected claim lives here instead.
    /// The killing mutation still holds this narrower claim: it changes `RefusalReason::code()` for
    /// `TooManyMetrics` to a wrong string, so the assertion on the code fails.
    #[tokio::test]
    async fn too_many_metrics_is_refused_before_allocation() {
        let client = connected(certified_service().0).await;
        let mut metrics = Vec::new();
        for n in 0..=sutura_domain::query::MAX_METRICS {
            metrics.push(format!("m{n}"));
        }
        let result = client
            .call_tool(ask(&serde_json::json!({
                "metrics": metrics,
                "grain": "month",
                "range": { "start": "2026-06-01", "end": "2026-07-01" },
            })))
            .await
            .expect("over-cap metrics is a refusal, not a protocol error");
        assert_refusal(&result, "too_many_metrics");
        drop(client.cancel().await);
    }

    /// Over-cap `dimensions` is refused as `too_many_dimensions` over the wire. Not renamed, same
    /// reason and correction as `too_many_metrics_is_refused_before_allocation` above; the killing
    /// mutation changes `RefusalReason::code()` for `TooManyDimensions`.
    #[tokio::test]
    async fn too_many_dimensions_is_refused_before_allocation() {
        let client = connected(certified_service().0).await;
        let mut dimensions = Vec::new();
        for n in 0..=sutura_domain::query::MAX_DIMENSIONS {
            dimensions.push(format!("d{n}"));
        }
        let result = client
            .call_tool(ask(&serde_json::json!({
                "metrics": ["revenue"],
                "grain": "month",
                "range": { "start": "2026-06-01", "end": "2026-07-01" },
                "dimensions": dimensions,
            })))
            .await
            .expect("over-cap dimensions is a refusal, not a protocol error");
        assert_refusal(&result, "too_many_dimensions");
        drop(client.cancel().await);
    }

    /// Over-cap `filters` is refused as `too_many_filters` over the wire. The filter count bound is
    /// the one #1089 added to the domain (`MAX_FILTERS = 16`). Not renamed, same reason and
    /// correction as `too_many_metrics_is_refused_before_allocation` above; the killing mutation
    /// changes `RefusalReason::code()` for `TooManyFilters`.
    #[tokio::test]
    async fn too_many_filters_is_refused_before_allocation() {
        let client = connected(certified_service().0).await;
        let mut filters = Vec::new();
        for _ in 0..=sutura_domain::query::MAX_FILTERS {
            filters.push(serde_json::json!({"op":"eq","dimension":"region","value":"north"}));
        }
        let result = client
            .call_tool(ask(&serde_json::json!({
                "metrics": ["revenue"],
                "grain": "month",
                "range": { "start": "2026-06-01", "end": "2026-07-01" },
                "filters": filters,
            })))
            .await
            .expect("over-cap filters is a refusal, not a protocol error");
        assert_refusal(&result, "too_many_filters");
        drop(client.cancel().await);
    }

    /// An oversized `statement` is a `-32602` parse error at the wire, before the `String` is
    /// allocated. Conformance pin: the base tree's `RawStatement::parse` returns `TooLong` after the
    /// allocation; the pre-allocation bound in `server::bounds` fires the same `-32602` before. The
    /// message distinguishes the two paths: `bounds` renders "larger than" (from
    /// `MalformedStatement::StatementTooLarge`), `RawStatement::parse` renders "not a statement" (from
    /// `MalformedStatement::Statement`). The test asserts the "larger than" message, so the killing
    /// mutation - removing the pre-allocation check in `server::bounds` - lets `RawStatement::parse`
    /// fire instead, changing the message and failing the assertion.
    #[tokio::test]
    async fn an_oversized_statement_is_a_named_parse_error() {
        let client = connected(certified_service().0).await;
        let too_long = "a".repeat(sutura_domain::raw::MAX_RAW_STATEMENT_BYTES + 1);
        let error = client
            .call_tool(raw(&serde_json::json!({ "statement": too_long })))
            .await
            .expect_err("an oversized statement is a parse error");
        assert_eq!(error_code(&error), ErrorCode::INVALID_PARAMS);
        assert!(error_message(&error).contains("larger than"), "{}", error_message(&error));
        drop(client.cancel().await);
    }

    // --------------------------------------------------- UNTESTED: run_sql refusals ---

    /// An empty `statement` is refused as `-32602` through the handler.
    #[tokio::test]
    async fn an_empty_statement_is_a_named_parse_error_over_the_wire() {
        let client = connected(certified_service().0).await;
        let error = client
            .call_tool(raw(&serde_json::json!({ "statement": "   " })))
            .await
            .expect_err("an empty statement is a parse error");
        assert_eq!(error_code(&error), ErrorCode::INVALID_PARAMS);
        drop(client.cancel().await);
    }

    /// A `statement` carrying an embedded NUL is refused as `-32602` through the handler.
    #[tokio::test]
    async fn a_statement_with_an_embedded_nul_is_a_named_parse_error_over_the_wire() {
        let client = connected(certified_service().0).await;
        let error = client
            .call_tool(raw(&serde_json::json!({ "statement": "select 1\0" })))
            .await
            .expect_err("an embedded NUL is a parse error");
        assert_eq!(error_code(&error), ErrorCode::INVALID_PARAMS);
        drop(client.cancel().await);
    }

    // ------------------------------------------- UNTESTED: ask_metric parse errors ---

    /// An over-cap `In` filter value set is `-32602` through the handler. The per-filter value bound
    /// (`MAX_VALUES_PER_DIMENSION`) is already enforced in `sutura_domain::question`; this pins it
    /// through the MCP handler.
    #[tokio::test]
    async fn too_many_filter_values_is_a_named_parse_error_over_the_wire() {
        let client = connected(certified_service().0).await;
        let mut values = Vec::new();
        for n in 0..=sutura_domain::catalog::MAX_VALUES_PER_DIMENSION {
            values.push(format!("v{n}"));
        }
        let error = client
            .call_tool(ask(&serde_json::json!({
                "metrics": ["revenue"],
                "grain": "month",
                "range": { "start": "2026-06-01", "end": "2026-07-01" },
                "filters": [{"op":"in","dimension":"region","values": values}],
            })))
            .await
            .expect_err("too many filter values is a parse error");
        assert_eq!(error_code(&error), ErrorCode::INVALID_PARAMS);
        drop(client.cancel().await);
    }

    /// `top.n = 0` is `-32602` through the handler.
    #[tokio::test]
    async fn a_zero_top_n_is_a_named_parse_error_over_the_wire() {
        let client = connected(certified_service().0).await;
        let error = client
            .call_tool(ask(&serde_json::json!({
                "metrics": ["revenue"],
                "grain": "month",
                "range": { "start": "2026-06-01", "end": "2026-07-01" },
                "top": {"n": 0, "by": "metric", "direction": "desc"},
            })))
            .await
            .expect_err("a zero top.n is a parse error");
        assert_eq!(error_code(&error), ErrorCode::INVALID_PARAMS);
        drop(client.cancel().await);
    }

    /// An invalid `top.by` is `-32602` through the handler.
    #[tokio::test]
    async fn an_invalid_top_by_is_a_named_parse_error_over_the_wire() {
        let client = connected(certified_service().0).await;
        let error = client
            .call_tool(ask(&serde_json::json!({
                "metrics": ["revenue"],
                "grain": "month",
                "range": { "start": "2026-06-01", "end": "2026-07-01" },
                "top": {"n": 5, "by": "volume", "direction": "desc"},
            })))
            .await
            .expect_err("an invalid top.by is a parse error");
        assert_eq!(error_code(&error), ErrorCode::INVALID_PARAMS);
        drop(client.cancel().await);
    }

    /// An invalid `top.direction` is `-32602` through the handler.
    #[tokio::test]
    async fn an_invalid_top_direction_is_a_named_parse_error_over_the_wire() {
        let client = connected(certified_service().0).await;
        let error = client
            .call_tool(ask(&serde_json::json!({
                "metrics": ["revenue"],
                "grain": "month",
                "range": { "start": "2026-06-01", "end": "2026-07-01" },
                "top": {"n": 5, "by": "metric", "direction": "up"},
            })))
            .await
            .expect_err("an invalid top.direction is a parse error");
        assert_eq!(error_code(&error), ErrorCode::INVALID_PARAMS);
        drop(client.cancel().await);
    }

    /// `range.last.count = 0` is `-32602` through the handler.
    #[tokio::test]
    async fn a_zero_last_count_is_a_named_parse_error_over_the_wire() {
        let client = connected(certified_service().0).await;
        let error = client
            .call_tool(ask(&serde_json::json!({
                "metrics": ["revenue"],
                "grain": "month",
                "range": { "last": {"count": 0, "unit": "month"} },
            })))
            .await
            .expect_err("a zero last.count is a parse error");
        assert_eq!(error_code(&error), ErrorCode::INVALID_PARAMS);
        drop(client.cancel().await);
    }

    /// An invalid `range.last.unit` is `-32602` through the handler.
    #[tokio::test]
    async fn an_invalid_last_unit_is_a_named_parse_error_over_the_wire() {
        let client = connected(certified_service().0).await;
        let error = client
            .call_tool(ask(&serde_json::json!({
                "metrics": ["revenue"],
                "grain": "month",
                "range": { "last": {"count": 1, "unit": "decade"} },
            })))
            .await
            .expect_err("an invalid last.unit is a parse error");
        assert_eq!(error_code(&error), ErrorCode::INVALID_PARAMS);
        drop(client.cancel().await);
    }

    // ------------------------------------- UNTESTED: no caller for describe_catalog ---

    /// A real `Peer<RoleServer>`, the only way to hold one - see `asking.rs` in the crate for why
    /// `Peer::new` is not an option.
    async fn a_peer() -> Peer<RoleServer> {
        struct Nobody;
        impl ServerHandler for Nobody {}
        let (client_side, server_side) = tokio::io::duplex(4096);
        let server = serve_server(Nobody, server_side);
        let client = serve_client((), client_side);
        let (server, client) = tokio::join!(server, client);
        let running = server.expect("the throwaway handshake's server initializes");
        let peer = running.peer().clone();
        drop(tokio::spawn(async move {
            drop(running.waiting().await);
        }));
        drop(client.expect("the throwaway handshake's client initializes").cancel().await);
        peer
    }

    /// `describe_catalog` under `Asking::PerRequest` with no established caller is refused as
    /// `-32600`, never answered as the deployment - the same refusal `ask_metric`/`tools/list` already
    /// have, asserted through the catalog door specifically.
    #[tokio::test]
    async fn describe_catalog_with_no_established_caller_is_refused_over_the_wire() {
        let (service, sink) = certified_service();
        let surface = AgentSurface::new(
            Arc::new(service),
            Asking::PerRequest,
            CatalogProse::Quoted,
            admission(),
            reply(),
            tools(),
            None,
        );
        let context = {
            let ctx = RequestContext::<RoleServer>::new(NumberOrString::Number(1), a_peer().await);
            // No `Asked` inserted: this is a request whose transport established nobody.
            ctx
        };
        let error = surface
            .call_tool(describe(), context)
            .await
            .expect_err("no established caller must be refused, not answered as the deployment");
        assert_eq!(error.code, ErrorCode::INVALID_REQUEST, "{error:?}");
        assert_eq!(
            sink.calls(),
            0,
            "a call refused for having no established caller writes no audit record"
        );
    }
}
