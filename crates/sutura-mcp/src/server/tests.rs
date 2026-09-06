//! The handler's tests, driven by the SDK's own client over an in-memory pipe.
//!
//! A file of its own because `server.rs` plus this suite crosses the 1000-line cap `cargo xtask
//! max-lines` enforces, and because the fixtures stay in `crate::testing` either way. The `mod tests`
//! line and the `#[cfg(test)]` on it are in the parent.
use std::sync::Arc;
use std::time::{Duration, Instant};

use rmcp::model::{CallToolRequestParams, ErrorCode};
use rmcp::service::RunningService;
use rmcp::{RoleClient, RoleServer, ServiceError, serve_client, serve_server};
use sutura_app::prompt::CatalogProse;
use sutura_app::surface::{LocalService, Surface};
use sutura_app::{Capability, Permitted};
use sutura_config::{Environment, RequestTimeout, Settings, Sources};
use sutura_runtime::Admission;

use super::AgentSurface;
use crate::testing;

/// What `server.request_timeout_seconds` bounds here, in its own file.
///
/// Split off because this suite plus `server.rs` crosses the 1000-line cap; the module's own header
/// says which group moved and why the admission pair did not.
mod deadline;

/// A client and a server joined by an in-memory pipe, with the peer permitted everything.
///
/// **The client is `rmcp`'s own**, which is what "with no client of ours in the loop" means: the
/// bytes on that pipe are the protocol, framed and parsed by the SDK on both sides, and nothing
/// this crate wrote sits between the assertion and the wire.
async fn connected<S>(surface: S) -> RunningService<RoleClient, ()>
where
    S: Surface,
{
    permitting(surface, Permitted::every_capability(), CatalogProse::Quoted).await
}

/// The same, with what the peer may do stated by the caller.
///
/// The seam the scope tests use, and it is the production signature rather than a test-only door:
/// `AgentSurface::new` takes the value because a composition root has to state it, and a test is
/// simply another composition root.
async fn permitting<S>(surface: S, permitted: Permitted, prose: CatalogProse) -> RunningService<RoleClient, ()>
where
    S: Surface,
{
    served(surface, permitted, prose, admission(""), reply("")).await
}

/// The same again, with both bounds stated as well.
///
/// The seam the bounds' own tests use, and it is the production signature for the same reason
/// [`permitting`] is: `AgentSurface::new` takes an `Admission` because only a composition root can
/// decide there is one bound for the process, and a `RequestTimeout` because only a root can decide
/// how long a peer waits - and a test is another composition root. Every other test here reaches it
/// through [`permitting`] with the settings tree's own defaults.
async fn served<S>(
    surface: S,
    permitted: Permitted,
    prose: CatalogProse,
    admission: Admission,
    reply: RequestTimeout,
) -> RunningService<RoleClient, ()>
where
    S: Surface,
{
    let (client_side, server_side) = tokio::io::duplex(64 * 1024);
    let server = serve_server(
        AgentSurface::new(Arc::new(surface), permitted, prose, admission, reply),
        server_side,
    );
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
        1 << 30,
    )
    .expect("the fixture bundle validates");
    (service, sink)
}

/// The settings tree as an operator inherits it, with this test's own document over the top.
///
/// A settings DOCUMENT and not two parsed numbers, which is what makes the bound below arrive the
/// way a deployment writes it. The same construction `sutura_http`'s harness uses, for the reason it
/// gives: a test that hands a value in cannot tell a read from a constant.
fn settings(overlay: &str) -> Settings {
    Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay)).expect("the test settings load")
}

/// The admission bound those settings describe.
fn admission(overlay: &str) -> Admission {
    Admission::from_settings(settings(overlay).runtime())
}

/// The reply deadline those settings describe.
///
/// Read off the same document as [`admission`], because the two bounds are one deployment's
/// decision and a test that took the first from a document and the second from a literal could not
/// tell either read from a constant.
fn reply(overlay: &str) -> RequestTimeout {
    settings(overlay).server().request_timeout()
}

/// Polls `ready` until it holds. `false` if it never did.
///
/// The alternative is a sleep long enough for the slowest machine, which is either a flake or dead
/// time in every run. What is asserted is the condition, so the failure message is the caller's.
async fn eventually(ready: impl Fn() -> bool) -> bool {
    for _ in 0_u16..1_000 {
        if ready() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

/// The text block of a tool result, which is the half a plain client renders.
fn text_of(result: &rmcp::model::CallToolResult) -> String {
    result
        .content
        .first()
        .and_then(rmcp::model::ContentBlock::as_text)
        .map(|block| block.text.clone())
        .expect("every result in this suite carries a text block")
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

/// The same question as a domain [`Query`], through this crate's own translation.
///
/// [`super::question`] rather than `Query::new`, so the value under test arrives the way a call's
/// does - the arguments object, parsed - and a test cannot construct a question the wire could not
/// carry.
///
/// [`Query`]: sutura_domain::query::Query
fn a_query() -> sutura_domain::query::Query {
    super::question(ask(&a_certified_question())).expect("the fixture question parses")
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
        CatalogProse::Quoted,
    )
    .await;
    let tools = catalog_only.list_all_tools().await.expect("tools/list answers");
    let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
    assert_eq!(names, [Capability::DescribeCatalog.id()], "{names:?}");
    drop(catalog_only.cancel().await);

    // Fail closed: a verified caller whose token named no capability scope sees nothing at all.
    let nobody = permitting(
        certified_service(),
        Permitted::granted_by(["openid", "profile"]),
        CatalogProse::Quoted,
    )
    .await;
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
        CatalogProse::Quoted,
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

/// `prompt.catalog_prose: omitted` is honoured by the TOOL, end to end - on BOTH halves of the reply.
///
/// The catalog tool answers with a text block a plain client renders and structured content a
/// program reads, so an operator who does not trust their catalog authors must be able to drop the
/// prose from this surface the way they drop it from the prompt - from all of it. With the setting
/// omitted the metric and dimension descriptions are absent from both halves and a notice says so;
/// with the default they arrive in both.
///
/// **The structured half is why this test exists in this shape.** Its first version read
/// `content.first()` and nothing else, so it passed while `structured_content` beside it carried
/// every description a deployment had asked to withhold - `#266`'s `H1`, and the same shape
/// `docs/adr/0022`'s amendment describes: a test that reads a different field from the one carrying
/// the leak looks like coverage and is none. Asserted through the SDK's own client, because the
/// defect was the WIRING and a test over the wire type alone passes with a handler that still
/// builds its content without the setting.
#[tokio::test]
async fn catalog_prose_omitted_omits_it_from_the_tool_answers() {
    let omitted = permitting(certified_service(), Permitted::every_capability(), CatalogProse::Omitted).await;
    let result = omitted.call_tool(describe()).await.expect("the catalog tool answers");
    let structured = result
        .structured_content
        .as_ref()
        .expect("the catalog carries structured content");
    // The descriptions exist in the bundle, and reach neither half of the reply.
    let flat = structured.to_string();
    assert!(!flat.contains("Revenue, in minor units."), "{flat}");
    assert!(!flat.contains("Sales region."), "{flat}");
    assert!(!flat.contains("description"), "{flat}");
    // The omission is a fact the client reads, not one it infers from a missing field.
    assert_eq!(structured["catalog_prose"], "omitted", "{flat}");
    // And what a caller needs in order to ask a valid question is untouched.
    assert_eq!(structured["metrics"][0]["name"], "revenue", "{flat}");
    assert_eq!(
        structured["metrics"][0]["dimensions"][0]["allowed_values"][0], "north",
        "{flat}"
    );
    let text = result
        .content
        .first()
        .and_then(rmcp::model::ContentBlock::as_text)
        .map(|block| block.text.clone())
        .expect("the catalog carries a text block");
    assert!(!text.contains("Revenue, in minor units."), "{text}");
    assert!(!text.contains("Sales region."), "{text}");
    // The omission is stated, not silent.
    assert!(text.contains("NOT included"), "{text}");
    drop(omitted.cancel().await);

    // And the default carries them in both halves, so the two settings provably differ and the
    // omission is a control rather than an outage.
    let quoted = permitting(certified_service(), Permitted::every_capability(), CatalogProse::Quoted).await;
    let result = quoted.call_tool(describe()).await.expect("the catalog tool answers");
    let structured = result
        .structured_content
        .as_ref()
        .expect("the catalog carries structured content");
    assert_eq!(
        structured["metrics"][0]["description"], "Revenue, in minor units.",
        "{structured}"
    );
    assert_eq!(structured["catalog_prose"], "quoted", "{structured}");
    let text = result
        .content
        .first()
        .and_then(rmcp::model::ContentBlock::as_text)
        .map(|block| block.text.clone())
        .expect("the catalog carries a text block");
    assert!(text.contains("> Revenue, in minor units."), "{text}");
    drop(quoted.cancel().await);
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

/// **F7, as a regression.** The configured number of questions execute at once, and the next one is
/// shed rather than admitted.
///
/// The finding's own shape: nine questions asked of a server whose bound is the repository default,
/// and the measurement is how many were inside `Surface::answer` at the same time. Before the bound
/// existed the answer was nine - a temporary probe over this same fake and this same SDK printed
/// `requested=9 simultaneously_inside_Surface_answer=9` - because the handler spawned a blocking
/// task per call with nothing counting them.
///
/// **The bound is read off the settings and not written here**, so this asserts the CONFIGURED
/// number rather than a number that happens to agree with one. Only the admission window is
/// overlaid, and only so the shed call does not spend the default five seconds proving it waited.
///
/// The `n` questions are in flight and confirmed inside the port before the `n + 1`th asks: without
/// that the test races the spawn, and a green run would mean the ninth arrived after a slot came
/// free.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_configured_bound_is_what_executes_at_once_and_the_next_question_is_shed() {
    let settings = settings("runtime:\n  admission_timeout_seconds: 1\n");
    let bound = settings.runtime().max_concurrent_queries().count();
    let window = settings.runtime().admission_timeout().duration();
    let admission = Admission::from_settings(settings.runtime());
    let (surface, holding) = testing::surface_that_can_be_held();
    let client = Arc::new(
        served(
            surface,
            Permitted::every_capability(),
            CatalogProse::Quoted,
            admission.clone(),
            settings.server().request_timeout(),
        )
        .await,
    );
    holding.arm();

    let mut inflight = Vec::new();
    for _ in 0..bound {
        let client = Arc::clone(&client);
        inflight.push(tokio::spawn(
            async move { client.call_tool(ask(&a_certified_question())).await },
        ));
    }
    assert!(
        eventually(|| holding.inside() == bound).await,
        "only {} of {bound} questions reached the port",
        holding.inside()
    );
    assert_eq!(admission.free(), 0, "the bound was not taken by the questions inside it");

    // The `n + 1`th, with every slot held.
    let began = Instant::now();
    let shed = client
        .call_tool(ask(&a_certified_question()))
        .await
        .expect("a shed call is a tool result and not a protocol error");
    let waited = began.elapsed();

    // Released before the assertions, so a failing one does not leave the blocking pool holding the
    // runtime open for the fake's own cap.
    holding.release();
    for held in inflight {
        let answered = held
            .await
            .expect("the calling task ran")
            .expect("a held question is answered once it is released");
        assert_ne!(answered.is_error, Some(true), "{answered:?}");
    }

    assert_eq!(
        holding.peak(),
        bound,
        "{} questions were inside the port at once against a bound of {bound}",
        holding.peak()
    );
    // Not a refusal and not a protocol error: the third channel, with the one thing an agent can act
    // on. A shed question was never judged, so nothing about it is a governance result.
    assert_eq!(shed.is_error, Some(true), "{shed:?}");
    let text = text_of(&shed);
    assert!(text.contains("ask again shortly"), "{text}");
    assert!(shed.structured_content.is_none(), "{shed:?}");
    // Bounded by the admission window, which is what stops the queue in front of the bound from
    // being a second unbounded thing.
    assert!(waited >= window, "shed before its window elapsed: {waited:?}");
    assert!(waited < window * 10, "the wait was not bounded by the window: {waited:?}");
}

/// **The other half, and the one a permit released at the wrong moment would fail.** A caller that
/// goes away does not hand back the slot the work it started is still holding.
///
/// `tokio` documents that a started blocking task cannot be aborted, so a question already on the
/// pool runs to completion whatever the peer is told. If the permit belonged to the future waiting
/// for that answer, the future being dropped would hand a slot back while the question it started
/// was still running - a bound on callers wearing the shape of a bound on work, which is worse than
/// no bound because it reads as a control. So the permit is moved into the closure, and this is the
/// assertion that says so.
///
/// **It drives `super::answer` and not the SDK's dispatch, and that is a limit rather than a
/// shortcut.** `rmcp` 3.1.4 answers a `notifications/cancelled` by cancelling a token this handler
/// does not read, and spawns each request as a detached task - so neither a cancelled call nor a
/// closed session drops the future that is waiting for an answer. Dropping one is therefore
/// something a test has to do itself: aborting the task is what a request timeout does to the same
/// future on the HTTP surface, where a `tower` layer drops it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_caller_that_goes_away_does_not_hand_back_the_slot_its_worker_still_holds() {
    let admission = admission("runtime:\n  max_concurrent_queries: 1\n  admission_timeout_seconds: 1\n");
    let (surface, holding) = testing::surface_that_can_be_held();
    let service = Arc::new(surface);
    holding.arm();

    let call = tokio::spawn({
        let service = Arc::clone(&service);
        let admission = admission.clone();
        async move { super::answer(&service, &admission, reply(""), a_query()).await }
    });
    assert!(
        eventually(|| holding.inside() == 1).await,
        "the question never reached the port"
    );
    assert_eq!(admission.free(), 0, "the slot was never taken");

    // The caller gives up. Awaiting the handle is the synchronisation point: it returns once the
    // task has actually been dropped, so anything the future owned has been released by here.
    call.abort();
    let gone = call.await.expect_err("an aborted call does not answer");
    assert!(gone.is_cancelled(), "{gone:?}");

    // THE assertion. The worker is still inside the port and the slot is still gone.
    assert_eq!(holding.inside(), 1, "the worker left the port when its caller did");
    assert_eq!(
        admission.free(),
        0,
        "the abandoned call handed its slot back while the work it started was still running"
    );
    // And the consequence, which is the whole point of holding it: nothing else may start on top of
    // work that is still running.
    let shed = super::answer(&service, &admission, reply(""), a_query()).await;
    assert_eq!(shed.is_error, Some(true), "a second question ran on top of the first");

    // Handed back when the WORK finishes, and not before.
    holding.release();
    assert!(
        eventually(|| admission.free() == 1).await,
        "the slot never came back after the work finished"
    );
    assert_eq!(holding.inside(), 0);
}
