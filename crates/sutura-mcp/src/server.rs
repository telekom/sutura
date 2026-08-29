//! The handler: `tools/list`, `tools/call`, and the three channels a caller has to be able to tell
//! apart.
//!
//! # Three outcomes, three channels, and the middle one is the property that matters
//!
//! | What happened | How it comes back | Why |
//! | --- | --- | --- |
//! | The question was answered | a tool result, `isError` absent, `outcome: "answer"` | rows inline, provenance beside them |
//! | The question was **refused** | a tool result, `isError` absent, `outcome: "refusal"` | it is a governance *result*, and nothing about it went wrong |
//! | The arguments were not a question | a JSON-RPC error, `-32602` | a parse failure, named, before the service is reached |
//! | The service could not answer | a tool result with `isError: true`, and no detail | something went wrong, and the detail is a path or a table |
//!
//! **A refusal is not an error and must not look like one.** `sutura_app::surface::Surface::answer`
//! is where a transport inherits that, and its own doc comment says why: a caller must not be able to
//! mistake "you may not ask that" for a hiccup and retry until something works. An `isError: true`
//! refusal would be exactly that mistake, in the one place a model is most likely to act on it - a
//! model told a call errored retries, and a model told the answer is "no, because the catalog defines
//! no such metric" asks something else.
//!
//! # Why a malformed argument is a JSON-RPC error rather than `isError`
//!
//! Because it is not a tool *execution* failure - the tool never ran. `-32602` is the name JSON-RPC
//! already has for it, `MalformedQuestion` names the field, and it is the one channel a caller cannot
//! read as either an answer or a refusal.
//!
//! **The limit, stated with the claim:** rmcp's own documentation notes that clients typically render
//! a protocol error opaquely, so a model may see less than the message carries. `Ok(isError: true)`
//! was the alternative and was rejected: it would put "your arguments were wrong" in the same channel
//! as "the data system is down", and the first is fixable by the caller while the second is not.
//!
//! # Why the port call leaves the async worker
//!
//! `Surface::answer` is synchronous, and the engine behind it drives its own runtime - entering a
//! runtime from within a runtime panics. So the call goes to the blocking pool through
//! `sutura_runtime::spawn_carrying_span`, which is the one call `clippy.toml` permits for this,
//! because a bare `spawn_blocking` loses the request's span on a pool thread.
//!
//! # What this slice does NOT do, on purpose
//!
//! * **No row cap of its own.** The answer cap is `max_rows + 1` and a result that hits it is
//!   refused rather than truncated; whether an *agent* surface wants a lower advisory cap - and what
//!   number - is a real question nobody has measured, so the bound is left exactly where it is.
//! * **No `get_tool`.** Implementing it would have rmcp validate arguments against the advertised
//!   schema before this handler sees them, which is defence in depth and also a second enforcement
//!   point whose message is not ours. One gate, and it is `crate::wire::AskArgs`'s own
//!   `deny_unknown_fields`.
//! * **No filtering by who is asking.** There is nothing verified to filter on: the deployment token
//!   on the HTTP surface authenticates a deployment, and `docs/adr/0014` decides leg 1 without
//!   building it. A filter over an unverified claim looks like a control and is not one.

use std::sync::Arc;

use rmcp::model::{
    CallToolRequestMethod, CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData, ServerHandler};
use sutura_app::surface::{Surface, SurfaceFailure, cause_chain};
use sutura_domain::query::Query;

use crate::tool;
use crate::wire::{AskArgs, MalformedQuestion, OutcomeContent};

/// What a cooperative client is told about this server, beyond its tools.
///
/// **Advisory, and it is not where correctness lives.** A gateway of the shape this product runs
/// behind ignores everything but tools, so a client that never reads this must still be answered
/// correctly - and is, because every rule is on the other side of the port.
const INSTRUCTIONS: &str = "Ask this server for numbers rather than for data. \
     One tool answers one governed question about one certified metric; a question outside what the \
     catalog declares comes back as a refusal naming the reason, which is an answer and not a fault. \
     Every answer carries the definition version and digest that produced it - quote them when you \
     report the number.";

/// The agent-facing surface over one [`Surface`].
///
/// Holds the service behind an `Arc` because a tool call is answered on the blocking pool, so the
/// port has to outlive the future that started the call.
pub struct AgentSurface<S> {
    service: Arc<S>,
}

impl<S> AgentSurface<S> {
    /// Wraps a service.
    ///
    /// Takes the `Arc` rather than making one, so a composition root serving two transports shares
    /// one bundle and one data system rather than opening a second of each.
    #[must_use]
    pub const fn new(service: Arc<S>) -> Self {
        Self { service }
    }
}

impl<S> core::fmt::Debug for AgentSurface<S> {
    /// Hand-written because a `Surface` implementation need not be `Debug` - and because printing a
    /// bundle into a log is a page of definitions for no benefit.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AgentSurface").finish_non_exhaustive()
    }
}

impl<S> ServerHandler for AgentSurface<S>
where
    S: Surface,
{
    fn get_info(&self) -> ServerInfo {
        // `Implementation::new` and not `from_build_env`: that helper reads the build environment of
        // the crate it is compiled into, which is the SDK, so a server using it introduces itself as
        // the SDK. `env!` here expands in this crate.
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    /// One tool, so no cursor and no page. `with_all_items` is what says that, rather than an empty
    /// `next_cursor` a reader has to interpret.
    ///
    /// Not an `async fn`, because there is nothing to await: building the tool is a schema
    /// derivation. The trait declares a future, so this returns a ready one.
    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult::with_all_items(vec![tool::ask_metric()])))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if request.name != tool::ASK_METRIC {
            return Err(ErrorData::method_not_found::<CallToolRequestMethod>());
        }
        let query = question(request)?;
        Ok(CallToolResponse::Complete(answer(&self.service, query).await))
    }
}

/// The arguments, parsed into a domain question.
///
/// **This is the whole translation an adapter is allowed to do**: read the wire shape, parse each
/// field into the newtype that establishes its invariant, hand over a `Query`. Nothing here decides
/// what may be asked.
fn question(request: CallToolRequestParams) -> Result<Query, ErrorData> {
    // `Value::Object` rather than the map directly, because `from_value` is what applies
    // `deny_unknown_fields` - and an absent `arguments` is an empty object, so a call with no
    // arguments fails on the missing required fields rather than on a different message.
    let arguments = serde_json::Value::Object(request.arguments.unwrap_or_default());
    let args: AskArgs = serde_json::from_value(arguments).map_err(|cause| invalid(&MalformedQuestion::NotAnObject { cause }))?;
    Query::try_from(args).map_err(|error| invalid(&error))
}

/// A malformed question, as a JSON-RPC error naming what was wrong.
///
/// The chain is walked into the message because `Display` on a `thiserror` enum prints the outermost
/// sentence only, and here the inner one is the half that names the field or the character set. That
/// is safe for exactly the reason `MalformedQuestion` is careful about: no variant carries the
/// caller's own text except where it has already failed an identifier parse.
fn invalid(error: &MalformedQuestion) -> ErrorData {
    let mut message = error.to_string();
    for cause in cause_chain(error) {
        message.push_str(": ");
        message.push_str(&cause);
    }
    ErrorData::invalid_params(message, None)
}

/// One question through the port, on the blocking pool, as a tool result.
async fn answer<S>(service: &Arc<S>, query: Query) -> CallToolResult
where
    S: Surface,
{
    let service = Arc::clone(service);
    match sutura_runtime::spawn_carrying_span(move || service.answer(&crate::principal::established(), &query)).await {
        // A refusal and an answer take the same branch, which is the point: both are `Ok`, both are
        // a tool result, and only `outcome` inside the payload tells them apart.
        Ok(Ok(ref outcome)) => produced(outcome),
        Ok(Err(failure)) => could_not_answer(&failure),
        // The blocking task did not finish: it panicked, or the runtime is shutting down. Reported
        // like a failure, because from a caller's side it is the same fact.
        Err(error) => {
            tracing::error!(error = %error, "the blocking task answering a tool call did not finish");
            failed("this deployment could not answer")
        }
    }
}

/// An answer or a refusal, as one result shape.
fn produced(outcome: &sutura_domain::query::ToolOutcome) -> CallToolResult {
    let content = OutcomeContent::from(outcome);
    let mut result = CallToolResult::success(vec![ContentBlock::text(content.as_text())]);
    // `ok()` rather than a propagated error: `OutcomeContent` is strings, numbers and vectors, so
    // serializing it cannot fail, and there is no `unwrap` in this workspace to say so. A client
    // that got no structured content still has the text block.
    result.structured_content = serde_json::to_value(&content).ok();
    result
}

/// Something went wrong, said with no detail taken from the cause.
///
/// The text of a `SurfaceFailure`'s chain is a path, a table or a column, and this result reaches a
/// model's context. The cause goes to the log instead, which is the one place text is the point -
/// the same judgement `sutura_http::problem`'s internal failure already makes.
fn could_not_answer(failure: &SurfaceFailure) -> CallToolResult {
    tracing::error!(error = %failure, causes = ?cause_chain(failure), "the agent surface could not answer");
    failed(match *failure {
        SurfaceFailure::Compile { .. } => "this deployment could not compile the question against its own bundle",
        SurfaceFailure::Warehouse { .. } => "the data system did not answer",
    })
}

/// A tool result that says the call failed, with a fixed sentence and nothing derived from a cause.
fn failed(detail: &str) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(String::from(detail))])
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rmcp::model::{CallToolRequestParams, ErrorCode};
    use rmcp::service::RunningService;
    use rmcp::{RoleClient, RoleServer, ServiceError, serve_client, serve_server};
    use sutura_app::surface::{LocalService, Surface};

    use super::AgentSurface;
    use crate::testing;

    /// A client and a server joined by an in-memory pipe.
    ///
    /// **The client is `rmcp`'s own**, which is what "with no client of ours in the loop" means: the
    /// bytes on that pipe are the protocol, framed and parsed by the SDK on both sides, and nothing
    /// this crate wrote sits between the assertion and the wire.
    async fn connected<S>(surface: S) -> RunningService<RoleClient, ()>
    where
        S: Surface,
    {
        let (client_side, server_side) = tokio::io::duplex(64 * 1024);
        let server = serve_server(AgentSurface::new(Arc::new(surface)), server_side);
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
    type CertifiedService = LocalService<testing::FakeWarehouse, std::sync::Arc<testing::CountingSink>>;

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
        let service =
            LocalService::start(&testing::FixedCatalog, testing::fake_warehouse(), std::sync::Arc::clone(&sink))
                .expect("the fixture bundle validates");
        (service, sink)
    }

    fn call(name: &'static str, arguments: &serde_json::Value) -> CallToolRequestParams {
        let object = arguments.as_object().cloned().expect("a fixture is an object");
        CallToolRequestParams::new(name).with_arguments(object)
    }

    fn ask(arguments: &serde_json::Value) -> CallToolRequestParams {
        call(crate::tool::ASK_METRIC, arguments)
    }

    fn a_certified_question() -> serde_json::Value {
        serde_json::json!({
            "metric": "revenue",
            "grain": "month",
            "range": { "start": "2026-06-01", "end": "2026-07-01" },
        })
    }

    #[tokio::test]
    async fn the_one_tool_is_advertised_with_its_generated_schema() {
        let client = connected(certified_service()).await;
        let tools = client.list_all_tools().await.expect("tools/list answers");
        assert_eq!(tools.len(), 1, "{tools:?}");
        let tool = tools.first().expect("one tool");
        assert_eq!(tool.name, crate::tool::ASK_METRIC);
        // The schema on the wire is the generated one, byte for byte - not a description of it.
        assert_eq!(*tool.input_schema.as_ref(), crate::tool::input_schema());
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
}
