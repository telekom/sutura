//! The handler's tests, driven by the SDK's own client over an in-memory pipe.
//!
//! A file of its own because `server.rs` plus this suite crosses the 1000-line cap `cargo xtask
//! max-lines` enforces, and because the fixtures stay in `crate::testing` either way. The `mod tests`
//! line and the `#[cfg(test)]` on it are in the parent.
use std::sync::Arc;

use rmcp::model::{CallToolRequestParams, ErrorCode};
use rmcp::service::RunningService;
use rmcp::{RoleClient, RoleServer, ServiceError, serve_client, serve_server};
use sutura_app::surface::{LocalService, Surface};
use sutura_app::{Capability, Permitted};

use super::AgentSurface;
use crate::testing;

/// A client and a server joined by an in-memory pipe, with the peer permitted everything.
///
/// **The client is `rmcp`'s own**, which is what "with no client of ours in the loop" means: the
/// bytes on that pipe are the protocol, framed and parsed by the SDK on both sides, and nothing
/// this crate wrote sits between the assertion and the wire.
async fn connected<S>(surface: S) -> RunningService<RoleClient, ()>
where
    S: Surface,
{
    permitting(surface, Permitted::every_capability()).await
}

/// The same, with what the peer may do stated by the caller.
///
/// The seam the scope tests use, and it is the production signature rather than a test-only door:
/// `AgentSurface::new` takes the value because a composition root has to state it, and a test is
/// simply another composition root.
async fn permitting<S>(surface: S, permitted: Permitted) -> RunningService<RoleClient, ()>
where
    S: Surface,
{
    let (client_side, server_side) = tokio::io::duplex(64 * 1024);
    let server = serve_server(AgentSurface::new(Arc::new(surface), permitted), server_side);
    let client = serve_client((), client_side);
    // Both halves of the handshake have to run at once: the server is waiting for `initialize`
    // and the client is waiting for its result, so awaiting either one first deadlocks.
    let (server, client) = tokio::join!(server, client);
    let running: RunningService<RoleServer, _> = server.expect("the server initializes");
    // Detached rather than held: the server's loop has to keep running for the duration of the
    // test, and nothing in a test asserts on the server handle itself.
    drop(tokio::spawn(async move {
        drop(running.waiting().await);
    }));
    client.expect("the client initializes")
}

/// What `certified_service` hands back: the real application over a fake data system, with the
/// audit sink every `LocalService` now requires.
type CertifiedService =
    LocalService<testing::FakeWarehouse, std::sync::Arc<testing::CountingSink>, testing::GrantsTheSharedIdentity>;

/// A service over the real application, so an answer here is an answer the anchor certified.
fn certified_service() -> CertifiedService {
    with_sink().0
}

/// The same service, plus the sink it writes to, for the one test that counts records.
///
/// `LocalService::start` REQUIRES a sink - a deployment that forgot to attach one is not a
/// state that exists - so every service here has one whether a test reads it or not.
fn with_sink() -> (CertifiedService, std::sync::Arc<testing::CountingSink>) {
    let sink = std::sync::Arc::new(testing::CountingSink::default());
    let service = LocalService::start(
        &testing::FixedCatalog,
        testing::fake_warehouse(),
        std::sync::Arc::clone(&sink),
        testing::broker(),
    )
    .expect("the fixture bundle validates");
    (service, sink)
}

fn call(name: &'static str, arguments: &serde_json::Value) -> CallToolRequestParams {
    let object = arguments.as_object().cloned().expect("a fixture is an object");
    CallToolRequestParams::new(name).with_arguments(object)
}

fn ask(arguments: &serde_json::Value) -> CallToolRequestParams {
    call(Capability::AskMetric.id(), arguments)
}

/// The catalog tool, called the way a client with nothing to say calls it.
fn describe() -> CallToolRequestParams {
    call(Capability::DescribeCatalog.id(), &serde_json::json!({}))
}

fn a_certified_question() -> serde_json::Value {
    serde_json::json!({
        "metric": "revenue",
        "grain": "month",
        "range": { "start": "2026-06-01", "end": "2026-07-01" },
    })
}

/// Every tool the shared source declares is advertised, with the schema that was generated for it.
///
/// **Over the wire and not over the function**, which is the whole reason this test exists beside
/// `crate::tool`'s own: the list a client receives is framed and parsed by the SDK, so this is the
/// assertion that the filtering, the naming and the schema survive the transport.
#[tokio::test]
async fn the_whole_tool_set_is_advertised_with_its_generated_schemas() {
    let client = connected(certified_service()).await;
    let tools = client.list_all_tools().await.expect("tools/list answers");
    let advertised: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
    let expected: Vec<&str> = Capability::every().map(Capability::id).collect();
    assert_eq!(advertised, expected, "{advertised:?}");
    for capability in Capability::every() {
        let tool = tools
            .iter()
            .find(|tool| tool.name.as_ref() == capability.id())
            .expect("every capability is advertised");
        // The schema on the wire is the generated one, byte for byte - not a description of it.
        assert_eq!(*tool.input_schema.as_ref(), crate::tool::input_schema(capability));
    }
    drop(client.cancel().await);
}

/// **THE property of this slice, over the wire.** A caller without a scope does not see the tool it
/// lacks.
///
/// `docs/implementation-plan.md` names this test. It asserts the presentation half; the invocation
/// half - which is the control - is the test below it.
#[tokio::test]
async fn the_advertised_tools_differ_by_scope() {
    let everything = connected(certified_service()).await;
    assert_eq!(
        everything.list_all_tools().await.expect("tools/list answers").len(),
        Capability::every().count()
    );
    drop(everything.cancel().await);

    let catalog_only = permitting(
        certified_service(),
        Permitted::granted_by([Capability::DescribeCatalog.scope()]),
    )
    .await;
    let tools = catalog_only.list_all_tools().await.expect("tools/list answers");
    let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
    assert_eq!(names, [Capability::DescribeCatalog.id()], "{names:?}");
    drop(catalog_only.cancel().await);

    // Fail closed: a verified caller whose token named no capability scope sees nothing at all.
    let nobody = permitting(certified_service(), Permitted::granted_by(["openid", "profile"])).await;
    let tools = nobody.list_all_tools().await.expect("tools/list answers");
    assert!(tools.is_empty(), "{tools:?}");
    drop(nobody.cancel().await);
}

/// **The control, as opposed to the presentation above.** A caller that names a tool it was never
/// shown is refused rather than answered.
///
/// This is the test that decides whether the filtering is a control or a courtesy, and it says
/// courtesy: the tool list can be skipped entirely - `call_tool` is reachable without it - so what
/// stops an ungranted call is `Permitted::includes` inside the handler and nothing else.
#[tokio::test]
async fn a_tool_that_was_not_advertised_is_refused_when_it_is_called_anyway() {
    let client = permitting(
        certified_service(),
        Permitted::granted_by([Capability::DescribeCatalog.scope()]),
    )
    .await;
    // Never advertised to this caller, and asked for by name regardless.
    let error = client
        .call_tool(ask(&a_certified_question()))
        .await
        .expect_err("a capability this caller was not granted is not answered");
    let ServiceError::McpError(data) = error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert_eq!(data.code, ErrorCode::METHOD_NOT_FOUND, "{data:?}");
    // And the message names the scope that would grant it, so an operator can act on it.
    assert!(data.message.contains(Capability::AskMetric.scope()), "{}", data.message);

    // The capability it WAS granted still answers, so the refusal above is about the grant rather
    // than about the transport having stopped working.
    let result = client
        .call_tool(describe())
        .await
        .expect("the granted capability is answered");
    assert_ne!(result.is_error, Some(true), "{result:?}");
    drop(client.cancel().await);
}

/// The catalog tool answers with what this deployment measures, and with the provenance an answer
/// carries - so a model can tell the metric it read about is the metric it measured.
#[tokio::test]
async fn the_catalog_tool_lists_the_metrics_this_deployment_measures() {
    let client = connected(certified_service()).await;
    let result = client.call_tool(describe()).await.expect("the catalog tool answers");
    assert_ne!(result.is_error, Some(true), "{result:?}");

    let structured = result
        .structured_content
        .as_ref()
        .expect("the catalog carries structured content");
    let metrics = structured
        .get("metrics")
        .and_then(serde_json::Value::as_array)
        .expect("a listing has metrics");
    assert!(
        metrics
            .iter()
            .any(|metric| metric.get("name").and_then(serde_json::Value::as_str) == Some("revenue")),
        "{structured:?}"
    );
    // The same version and digest an answer carries. Not a separate description of the bundle.
    assert_eq!(
        structured
            .get("provenance")
            .and_then(|provenance| provenance.get("definition_version"))
            .and_then(serde_json::Value::as_str),
        Some("test-1"),
        "{structured:?}"
    );
    // And a client that renders only content blocks still sees the metric.
    let text = result
        .content
        .first()
        .and_then(rmcp::model::ContentBlock::as_text)
        .map(|block| block.text.clone())
        .expect("the catalog carries a text block");
    assert!(text.contains("revenue"), "{text}");
    drop(client.cancel().await);
}

/// The catalog tool's arguments are PARSED, so a caller cannot believe it narrowed a listing.
///
/// Without the parse this call deserializes cleanly, `metric` is dropped on the floor, and a model
/// that asked about one metric is handed every one of them while believing it filtered.
#[tokio::test]
async fn an_argument_the_catalog_tool_does_not_declare_is_a_named_parse_error() {
    let client = connected(certified_service()).await;
    let error = client
        .call_tool(call(
            Capability::DescribeCatalog.id(),
            &serde_json::json!({ "metric": "revenue" }),
        ))
        .await
        .expect_err("the catalog tool declares no arguments");
    let ServiceError::McpError(data) = error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert_eq!(data.code, ErrorCode::INVALID_PARAMS, "{data:?}");
    assert!(data.message.contains("metric"), "{}", data.message);
    drop(client.cancel().await);
}

/// Reading the catalog writes no audit record, and that is the honest reading of the invariant rather
/// than a gap in it.
///
/// `sutura_domain::audit::CallRecord` records the outcome of a *question* - an answer or a refusal -
/// and a catalog listing is neither. So the assertion is that the count does not move, and the note
/// beside `super::describe` says what widening it would cost.
#[tokio::test]
async fn reading_the_catalog_is_not_an_outcome_and_writes_no_record() {
    let (service, sink) = with_sink();
    let client = connected(service).await;
    drop(client.call_tool(describe()).await.expect("the catalog tool answers"));
    assert_eq!(sink.calls(), 0, "a catalog listing is not a question");
    drop(
        client
            .call_tool(ask(&a_certified_question()))
            .await
            .expect("a question is answered"),
    );
    assert_eq!(sink.calls(), 1, "a question is");
    drop(client.cancel().await);
}

/// #37 made "every outcome recorded before it returns" an invariant, and this transport is a
/// second caller of `Surface::answer` - so it needs its own assertion rather than inheriting
/// the HTTP surface's. A refusal counts too: it is an outcome, not a failure.
#[tokio::test]
async fn every_answered_call_over_this_transport_writes_one_record() {
    let (service, sink) = with_sink();
    let client = connected(service).await;
    assert_eq!(sink.calls(), 0, "nothing asked, nothing recorded");

    drop(
        client
            .call_tool(ask(&a_certified_question()))
            .await
            .expect("a certified question is not a protocol error"),
    );
    assert_eq!(sink.calls(), 1, "an answer is an outcome and is recorded");

    drop(
        client
            .call_tool(ask(&serde_json::json!({
                "metric": "headcount",
                "grain": "month",
                "range": { "start": "2026-06-01", "end": "2026-07-01" },
            })))
            .await
            .expect("a refusal is a result, not a protocol error"),
    );
    assert_eq!(sink.calls(), 2, "a refusal is an outcome too, so it is recorded as well");

    drop(client.cancel().await);
}

#[tokio::test]
async fn a_certified_question_is_answered_over_the_agent_surface() {
    let client = connected(certified_service()).await;
    let result = client
        .call_tool(ask(&a_certified_question()))
        .await
        .expect("a certified question is not a protocol error");

    // Not an error, in either sense MCP has for one.
    assert_ne!(result.is_error, Some(true), "{result:?}");

    // The rows are IN the result. One shape, no handle, no second fetch.
    let structured = result
        .structured_content
        .as_ref()
        .expect("an answer carries structured content");
    assert_eq!(structured.get("outcome").and_then(serde_json::Value::as_str), Some("answer"));
    let rows = structured.get("rows").and_then(serde_json::Value::as_array).expect("rows");
    assert_eq!(rows.len(), 1, "{structured:?}");
    assert!(
        serde_json::to_string(structured)
            .expect("the structured content serializes")
            .contains(&testing::ANCHORED_VALUE.to_string()),
        "{structured:?}"
    );

    // And the provenance, which is what makes the number quotable.
    let provenance = structured.get("provenance").expect("an answer carries provenance");
    assert_eq!(
        provenance.get("definition_version").and_then(serde_json::Value::as_str),
        Some("test-1"),
        "{provenance:?}"
    );

    // A client that renders only content blocks still sees the number.
    let text = result
        .content
        .first()
        .and_then(rmcp::model::ContentBlock::as_text)
        .map(|block| block.text.clone())
        .expect("an answer carries a text block");
    assert!(text.contains(&testing::ANCHORED_VALUE.to_string()), "{text}");
    drop(client.cancel().await);
}

#[tokio::test]
#[expect(non_snake_case, reason = "the plan names this test, and the emphasis is the point")]
async fn an_uncertified_question_is_refused_as_a_RESULT_rather_than_an_error() {
    let client = connected(certified_service()).await;
    let result = client
        .call_tool(ask(&serde_json::json!({
            "metric": "headcount",
            "grain": "month",
            "range": { "start": "2026-06-01", "end": "2026-07-01" },
        })))
        .await
        .expect("a refusal is not a protocol error");

    // THE property. A refusal arrives through `Ok`, as a tool result, and `isError` is not set -
    // so nothing a client or a model reads says "retry this".
    assert_ne!(result.is_error, Some(true), "{result:?}");
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
        Some("metric_unknown"),
        "{reason:?}"
    );
    // No rows and no provenance: nothing was measured, so there is nothing to certify.
    assert!(structured.get("rows").is_none(), "{structured:?}");
    assert!(structured.get("provenance").is_none(), "{structured:?}");
    drop(client.cancel().await);
}

#[tokio::test]
#[expect(non_snake_case, reason = "kept in step with the test above, whose name the plan fixes")]
async fn a_question_outside_the_metrics_grains_is_also_a_RESULT() {
    // A second refusal through the same channel, so the assertion above is not a property of
    // one variant. The bundle declares day and month; a week is a governance decision, not a
    // parse failure.
    let client = connected(certified_service()).await;
    let result = client
        .call_tool(ask(&serde_json::json!({
            "metric": "revenue",
            "grain": "week",
            "range": { "start": "2026-06-01", "end": "2026-07-01" },
        })))
        .await
        .expect("a refusal is not a protocol error");
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let structured = result
        .structured_content
        .as_ref()
        .expect("a refusal carries structured content");
    assert_eq!(
        structured
            .get("reason")
            .and_then(|reason| reason.get("code"))
            .and_then(serde_json::Value::as_str),
        Some("grain_not_supported"),
        "{structured:?}"
    );
    drop(client.cancel().await);
}

#[tokio::test]
async fn a_query_field_the_domain_does_not_declare_is_a_named_parse_error() {
    // The `deny_unknown_fields` guarantee, asserted THROUGH the transport rather than assumed to
    // survive it. Without it this call deserializes cleanly, `sql` is dropped on the floor, and a
    // model that believes it sent SQL is answered as though it had asked the modelled question.
    let client = connected(certified_service()).await;
    let error = client
        .call_tool(ask(&serde_json::json!({
            "metric": "revenue",
            "grain": "month",
            "range": { "start": "2026-06-01", "end": "2026-07-01" },
            "sql": "select * from orders",
        })))
        .await
        .expect_err("`sql` is not a field of a question");

    let ServiceError::McpError(data) = error else {
        panic!("expected a protocol error, got {error:?}");
    };
    // Named: the JSON-RPC code for bad parameters, and the field in the message.
    assert_eq!(data.code, ErrorCode::INVALID_PARAMS, "{data:?}");
    assert!(data.message.contains("sql"), "{}", data.message);
    drop(client.cancel().await);
}

#[tokio::test]
async fn a_service_that_cannot_answer_is_an_error_and_carries_no_detail_from_the_cause() {
    // The third channel, and the reason it is a third: a data system that is down is neither an
    // answer nor a governance decision. It comes back as `isError`, and the driver's own
    // complaint - which names a table, a column or a path - does not travel with it.
    let client = connected(testing::FailingSurface::new()).await;
    let result = client
        .call_tool(ask(&a_certified_question()))
        .await
        .expect("a failure is still a tool result");
    assert_eq!(result.is_error, Some(true), "{result:?}");
    let text = result
        .content
        .first()
        .and_then(rmcp::model::ContentBlock::as_text)
        .map(|block| block.text.clone())
        .expect("a failure carries a text block");
    assert!(!text.contains("connection refused"), "{text}");
    assert!(text.contains("data system"), "{text}");
    drop(client.cancel().await);
}

#[tokio::test]
async fn a_tool_this_server_does_not_have_is_not_found_rather_than_answered() {
    let client = connected(certified_service()).await;
    let error = client
        .call_tool(call("run_sql", &a_certified_question()))
        .await
        .expect_err("there is one tool");
    let ServiceError::McpError(data) = error else {
        panic!("expected a protocol error, got {error:?}");
    };
    assert_eq!(data.code, ErrorCode::METHOD_NOT_FOUND, "{data:?}");
    drop(client.cancel().await);
}
