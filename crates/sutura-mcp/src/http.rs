//! The streamable-HTTP transport, as a plain `tower_service::Service` a composition root mounts.
//!
//! **`#[cfg(feature = "http")]` only** - `telekom/sutura#378` PR3, `docs/adr/0023`. Nothing served
//! links this today: [`crate::http::service`] builds a value a composition root's own router can
//! `axum::Router::nest_service` behind its existing layers, so `sutura-serve`'s leg 1
//! (`sutura_http::inbound`) and `sutura_http::capability::establish_asked` run in front of it
//! exactly as they run in front of every other route on that surface - PR4's job. This crate carries
//! no dependency on `axum` outside its own `#[cfg(test)]` tests: `StreamableHttpService` is a bare
//! `tower_service::Service`, which is what a transport crate composes against rather than an
//! adapter's own router type - see `.agents/skills/sutura/crate-map/SKILL.md`'s "a transport is
//! transport-only."
//!
//! # `Asking::PerRequest`, and why this exists at all
//!
//! [`crate::Asking::PerRequest`] reads a [`sutura_app::Asked`] out of
//! `context.extensions.get::<http::request::Parts>().and_then(|parts| parts.extensions.get::<Asked>())`
//! (`server.rs`). Traced against the pinned SDK's own streamable-HTTP server, that `Parts` value is
//! exactly the request's own `http::request::Parts` - the SAME value
//! `sutura_http::capability::establish_asked` inserts an `Asked` into on the HTTP surface's other
//! routes, before that request ever reaches the nested tower service (`docs/adr/0023`, quoting
//! `streamable_http_server/tower.rs`). [`crate::http::service`] is what makes a REAL one of those reachable at
//! all; until PR4 mounts it behind the real `establish_asked` layer, this module's own tests stand
//! in for that layer with a fake one, inserting a chosen `Asked` the same way.
//!
//! # Session semantics: `legacy_session_mode: false`, and what that decision rests on
//!
//! [`crate::http::config`] pins `StreamableHttpServerConfig::legacy_session_mode` to `false` rather than the
//! SDK's own default (`true`). Read against the pinned transport's own `handle_post`
//! (`transport/streamable_http_server/tower.rs`): with `legacy_session_mode: false`, EVERY request -
//! `initialize` included - is served through its "Stateless mode" branch, which calls this crate's
//! own `service_factory` closure FRESH per request (one new [`crate::AgentSurface`] per call, never
//! reused across requests) and drives exactly one message through
//! `rmcp::transport::OneshotTransport` - never `SessionManager::create_session`,
//! `has_session` or `create_stream`. A `Mcp-Session-Id` header is never looked up under this
//! config, by ANY message type - confirmed by reading the branch rather than assumed from its name.
//!
//! **This is the property the invariant asks for, held by absence rather than by a check: a
//! streamable-HTTP session cannot outlive or cross the verified caller, because under this config
//! there is no session for one request's identity to leak into another's.** Nothing here refuses a
//! caller that PRESENTS a foreign or fabricated `Mcp-Session-Id` - that header is simply inert - and
//! this module's own `tests::a_reused_session_id_carries_no_weight_across_two_different_callers` is
//! the cell that proves it: two requests carrying the SAME fabricated header, two different fake
//! `Asked` values, and each response reflects only its OWN request's identity.
//!
//! **`json_response: true` is pinned alongside it, and it is a second, narrower decision.** Neither
//! `AgentSurface::list_tools` nor `AgentSurface::call_tool` ever emits an intermediate
//! notification or a server-initiated request ahead of its final reply - `sutura_mcp` has never had
//! a use for either - so the SDK's own documented fallback ("if the handler emits a notification or
//! request before the final response, the server falls back to `text/event-stream`") never fires
//! for this deployment's own tool set, and every ordinary call gets back a plain JSON body instead
//! of an SSE stream to parse. A future tool that DID need to stream would need this reconsidered.
//!
//! **What this module does NOT settle, named rather than left implicit:**
//! - **A client negotiating the SDK's OWN advertised `LATEST` protocol version (`2025-11-25`) is
//!   still served correctly under this config, including its very first `tools/list` call with no
//!   prior handshake required.** Read carefully because `handle_post` has TWO `else` arms that are
//!   easy to conflate at a skim: the one reached when `legacy_session_mode: true` and a message
//!   arrives with no `Mcp-Session-Id` header (which DOES require the message to be an
//!   `InitializeRequest` or `DiscoverRequest`, refusing anything else) and the one reached when
//!   `legacy_session_mode: false` (the "Stateless mode" arm, a sibling of the whole
//!   `if use_session {}` statement, not nested inside it) - which serves EVERY message type
//!   one-shot, `initialize` included. [`crate::http::config`]'s pin selects the second; the cells in this
//!   module exercise it directly rather than trust this paragraph.
//! - **`allowed_hosts`/`allowed_origins` are left at the SDK's own defaults**
//!   (`["localhost", "127.0.0.1", "::1"]`, no origin check) - a composition root serving this
//!   outside loopback must override them, or the transport refuses every request with a `Host`
//!   header it does not recognise. PR4's job to state, not this module's.
//! - **The exact SEP-2243 header-validation helpers this module's tests exercise
//!   (`validate_standard_headers`, `validate_request_protocol_version_meta`) were read for their
//!   no-op conditions on a plain, non-`stateless_protocol_metadata_required` request and not
//!   exhaustively traced line by line.** Flagged as the narrowest residual risk in this file; it
//!   is settled by the four cells below compiling and passing against the pinned SDK, not by an
//!   exhaustive manual trace.

use std::sync::Arc;

use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
use sutura_app::prompt::CatalogProse;
use sutura_app::surface::Surface;
use sutura_config::RequestTimeout;
use sutura_runtime::Admission;

use crate::{AgentSurface, Asking};

/// This deployment's fixed transport configuration - see the module documentation for
/// `legacy_session_mode` and `json_response`.
///
/// A function rather than a `const`: `StreamableHttpServerConfig` is `#[non_exhaustive]` - a
/// struct-expression literal cannot name its fields at all - and its `Default` builds a fresh
/// `CancellationToken`, so the two pins below can only be applied through the SDK's own builder.
///
/// **The limit the two pins carry, stated rather than assumed contractually:** only
/// `legacy_session_mode` and `json_response` are set here; the other eight fields are inherited
/// from the SDK's `Default` through the builder and are not pinned - a future field with an unsafe
/// default would arrive silently, and `allowed_hosts` stays loopback-only, so a composition root
/// serving outside loopback must override it (the transport refuses every unrecognised `Host`, see
/// the module documentation). And [`service`] still constructs a [`LocalSessionManager`]; that
/// manager is kept idle by `legacy_session_mode: false` alone. Nothing here binds a session to a
/// caller, and the SDK's own `create_session` takes no identity argument regardless - the caller is
/// re-resolved per request out of each request's `Asked`, never out of a session.
#[must_use]
pub fn config() -> StreamableHttpServerConfig {
    StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
}

/// Builds the streamable-HTTP transport over one [`Surface`], as a plain `tower_service::Service`
/// for a composition root to `nest_service` behind its own layers.
///
/// **Always [`Asking::PerRequest`], and that is not a parameter.** This constructor exists
/// specifically for a transport reachable over a network; `Asking::TheProcessOwner` is
/// [`crate::serve_stdio`]'s answer for a pipe, and giving this function a choice would make "which
/// one" a call-site decision a composition root could get backwards. See [`crate::Asking`] for why
/// the two are not interchangeable.
///
/// `service_factory` is called by the SDK ONCE PER REQUEST under [`config`]'s stateless mode (see
/// the module documentation) - never once per process and never once per session - so each call
/// clones the shared `service`/`admission` handles rather than allocating a second data-system
/// connection or a second permit set.
#[must_use]
pub fn service<S>(
    surface: Arc<S>,
    prose: CatalogProse,
    admission: Admission,
    reply: RequestTimeout,
) -> StreamableHttpService<AgentSurface<S>, LocalSessionManager>
where
    S: Surface + Send + Sync + 'static,
{
    StreamableHttpService::new(
        move || {
            Ok(AgentSurface::new(
                Arc::clone(&surface),
                Asking::PerRequest,
                prose,
                admission.clone(),
                reply,
            ))
        },
        Arc::new(LocalSessionManager::default()),
        config(),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::Router;
    use axum::body::{Body, to_bytes};
    use axum::extract::{Request, State};
    use axum::http::header;
    use axum::middleware::{Next, from_fn_with_state};
    use axum::response::Response;
    use serde_json::{Value, json};
    use sutura_app::prompt::CatalogProse;
    use sutura_app::{Asked, Capability, Permitted};
    use sutura_config::{Environment, RequestTimeout, Settings, Sources};
    use sutura_domain::identity::{PrincipalChain, RequestContext, Subject, SubjectId};
    use sutura_runtime::Admission;
    use tower::ServiceExt as _;

    use crate::testing;

    /// One name for the type [`super::service`] returns, so the two router-building helpers below
    /// spell it once rather than twice.
    type Transport<S> = rmcp::transport::StreamableHttpService<
        crate::AgentSurface<S>,
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager,
    >;

    /// The settings tree as an operator inherits it - the same construction `server/tests.rs` uses,
    /// duplicated here rather than shared: this module and `server/tests` are siblings, not a
    /// parent/child pair, and neither exposes its fixtures to the other on purpose (`testing.rs`'s
    /// own module documentation gives the same reason for its duplication against `sutura_http`).
    fn settings() -> Settings {
        Settings::load(&Sources::defaults(Environment::Development)).expect("the default settings load")
    }

    fn admission() -> Admission {
        Admission::from_settings(settings().runtime())
    }

    fn reply() -> RequestTimeout {
        settings().server().request_timeout()
    }

    /// A caller `Asked` naming a distinct subject, so two fixtures in one test are provably two
    /// different callers rather than the same value written twice - the same shape
    /// `server/tests/asking.rs::subject_asked` uses for the in-process cells this module's tests
    /// extend over real HTTP bytes.
    fn subject_asked(id: &str, permitted: Permitted) -> Asked {
        let subject = Subject::Verified {
            id: SubjectId::parse(id).expect("a test subject id is a subject id"),
        };
        Asked::established(RequestContext::of(PrincipalChain::of(subject)), permitted)
    }

    /// Stands in for `sutura_http::capability::establish_asked` on this crate's own hermetic
    /// router: inserts ONE fixed `Asked` into every request's extensions, ahead of the mount - the
    /// exact insertion point the real layer uses on the HTTP surface's other routes.
    async fn insert_asked(State(asked): State<Arc<Asked>>, mut request: Request, next: Next) -> Response {
        drop(request.extensions_mut().insert(Asked::clone(&asked)));
        next.run(request).await
    }

    /// One caller's whole router: the mount, behind a fake `establish_asked` naming `asked`.
    ///
    /// Takes the transport by value and clones it internally rather than requiring the caller to -
    /// `StreamableHttpService::clone()` shares the same `Arc`-held session manager and factory, so
    /// two routers built from clones of ONE transport are still one mounted transport for every
    /// property this module's tests care about, the same way two `axum::Router`s built over one
    /// `ServiceState::clone()` are one service in `sutura_http`'s own tests.
    fn router<S>(transport: Transport<S>, asked: Asked) -> Router
    where
        S: sutura_app::surface::Surface + Send + Sync + 'static,
    {
        Router::new()
            .nest_service("/mcp", transport)
            .layer(from_fn_with_state(Arc::new(asked), insert_asked))
    }

    /// The same, with no identity layer at all - the shape a deployment whose `establish_asked`
    /// layer was skipped or mis-ordered would produce.
    fn router_with_no_established_caller<S>(transport: Transport<S>) -> Router
    where
        S: sutura_app::surface::Surface + Send + Sync + 'static,
    {
        Router::new().nest_service("/mcp", transport)
    }

    fn initialize(id: i64) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "sutura-mcp-http-test", "version": "0.0.0"}
            }
        })
    }

    fn tools_list(id: i64) -> Value {
        json!({"jsonrpc": "2.0", "id": id, "method": "tools/list", "params": {}})
    }

    /// One POST, no fabricated `Mcp-Session-Id` - the ordinary shape.
    async fn post(app: Router, body: Value) -> Value {
        post_with(app, body, None).await
    }

    /// The same, returning the HTTP status alongside the parsed body - used where the status is
    /// part of the assertion (the no-caller refusal is a JSON-RPC error the pinned SDK serves as
    /// HTTP 200, not a 5xx, on both the `json_response` path, `tower.rs:2003`, and the negotiated
    /// path, `tower.rs:637`).
    async fn post_with_status(app: Router, body: Value, session_id: Option<&str>) -> (axum::http::StatusCode, Value) {
        let mut builder = axum::http::Request::builder()
            .method("POST")
            .uri("/mcp")
            .header(header::HOST, "localhost")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(session_id) = session_id {
            builder = builder.header("mcp-session-id", session_id);
        }
        let request = builder
            .body(Body::from(
                serde_json::to_vec(&body).expect("a test JSON-RPC body serializes"),
            ))
            .expect("a well-formed test request builds");
        let response = app.oneshot(request).await.expect("a tower service's Error is Infallible");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the hermetic response body reads to completion");
        let value = serde_json::from_slice(&bytes).unwrap_or_else(|cause| panic!("the response body is JSON: {cause}"));
        (status, value)
    }

    /// The same, with a caller-chosen `Mcp-Session-Id` header - used only by
    /// [`a_reused_session_id_carries_no_weight_across_two_different_callers`] to fabricate reuse.
    async fn post_with(app: Router, body: Value, session_id: Option<&str>) -> Value {
        post_with_status(app, body, session_id).await.1
    }

    fn tool_names(response: &Value) -> Vec<String> {
        response
            .get("result")
            .and_then(|result| result.get("tools"))
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("a tools/list result carries a `tools` array: {response}"))
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(String::from))
            .collect()
    }

    /// The composition claim itself: `crate::http::service` mounts inside a plain `axum::Router`
    /// via `nest_service`, and one MOUNTED instance answers `tools/list` per the REQUEST's own
    /// `Asked` - not a value fixed when the router was built.
    #[tokio::test]
    async fn mounting_the_transport_over_axum_answers_tools_list_per_request() {
        let transport = super::service(
            Arc::new(testing::FailingSurface::new()),
            CatalogProse::Quoted,
            admission(),
            reply(),
        );

        let catalog_only = subject_asked(
            "catalog-only-caller",
            Permitted::granted_by([Capability::DescribeCatalog.scope()]),
        );
        let app_a = router(transport.clone(), catalog_only);
        drop(post(app_a.clone(), initialize(1)).await);
        let tools_a = post(app_a, tools_list(2)).await;
        let names_a = tool_names(&tools_a);
        assert_eq!(names_a, vec![String::from("describe_catalog")], "{tools_a}");

        let everything = subject_asked("every-capability-caller", Permitted::every_capability());
        let app_b = router(transport, everything);
        drop(post(app_b.clone(), initialize(1)).await);
        let tools_b = post(app_b, tools_list(2)).await;
        let names_b = tool_names(&tools_b);
        assert!(
            names_b.len() > names_a.len(),
            "a caller with every capability must see more tools than one scoped to `describe_catalog` alone: {tools_b}"
        );
    }

    /// A tool call with nothing ahead of the mount establishing who is asking is refused over the
    /// real HTTP wire, not just in-process - `server/tests/asking.rs`'s
    /// `a_tool_call_with_no_established_caller_is_refused_and_never_answered_as_the_deployment`
    /// proves the same substitution in-process; this cell is the new thing PR3 adds, because a
    /// code-, status- or shape-mapping regression between `ErrorData` and the wire is invisible to
    /// that test. It asserts the JSON-RPC error CODE (`INVALID_REQUEST` = `-32600`) and the HTTP
    /// status the pinned SDK maps it to (`200`, on both the `json_response` path, `tower.rs:2003`,
    /// and the negotiated path, `tower.rs:637`) - so a mis-set code or an accidental 5xx reddens
    /// this cell, not just a wrong message string.
    #[tokio::test]
    async fn a_request_with_no_established_caller_is_refused_over_http() {
        let transport = super::service(
            Arc::new(testing::FailingSurface::new()),
            CatalogProse::Quoted,
            admission(),
            reply(),
        );
        let app = router_with_no_established_caller(transport);
        drop(post(app.clone(), initialize(1)).await);
        let (status, refused) = post_with_status(app, tools_list(2), None).await;
        let error = refused
            .get("error")
            .and_then(Value::as_object)
            .unwrap_or_else(|| panic!("no established caller must answer a JSON-RPC error object: {refused}"));
        let code = error
            .get("code")
            .and_then(Value::as_i64)
            .unwrap_or_else(|| panic!("the refusal carries a numeric JSON-RPC error code: {refused}"));
        assert_eq!(
            code, -32600,
            "`no_established_caller` is `INVALID_REQUEST` (-32600), got: {refused}"
        );
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("the refusal names the reason in `message`: {refused}"));
        assert!(
            message.contains("could not establish who is asking"),
            "expected `no_established_caller`'s own message, got: {refused}"
        );
        // `INVALID_REQUEST` is not in the SDK's BAD_REQUEST/NOT_FOUND set, so `jsonrpc_http_status`
        // falls to `_ => StatusCode::OK` (`tower.rs:637`); the `json_response` stateless arm writes
        // the same 200 explicitly (`tower.rs:2003`). It is a JSON-RPC refusal, not a 5xx.
        assert_eq!(
            status,
            axum::http::StatusCode::OK,
            "the pinned SDK serves this refusal as HTTP 200, got: {status} {refused}"
        );
    }

    /// **The session-semantics cell the invariant is built to survive.** Two requests, the SAME
    /// fabricated `Mcp-Session-Id` header, two different callers - one mounted transport must
    /// answer each strictly as itself. Under `crate::http::config`'s `legacy_session_mode: false`
    /// the header is never looked up at all (see the module documentation), so this also proves
    /// that claim rather than only the identity one: neither response depends on the OTHER request
    /// having run at all, which a session-keyed lookup would not survive.
    #[tokio::test]
    async fn a_reused_session_id_carries_no_weight_across_two_different_callers() {
        let transport = super::service(
            Arc::new(testing::FailingSurface::new()),
            CatalogProse::Quoted,
            admission(),
            reply(),
        );
        let foreign_session_id = "a-session-id-this-request-never-opened";

        let catalog_only = subject_asked(
            "caller-a-on-a-reused-session-id",
            Permitted::granted_by([Capability::DescribeCatalog.scope()]),
        );
        let app_a = router(transport.clone(), catalog_only);
        let tools_a = post_with(app_a, tools_list(1), Some(foreign_session_id)).await;
        let names_a = tool_names(&tools_a);
        assert_eq!(names_a, vec![String::from("describe_catalog")], "{tools_a}");

        let everything = subject_asked("caller-b-on-the-same-reused-session-id", Permitted::every_capability());
        let app_b = router(transport, everything);
        let tools_b = post_with(app_b, tools_list(1), Some(foreign_session_id)).await;
        let names_b = tool_names(&tools_b);
        assert!(
            names_b.len() > names_a.len(),
            "a caller presenting the SAME Mcp-Session-Id header as an earlier, different caller \
             must still answer strictly as itself, never inheriting the first caller's grant: {tools_b}"
        );
    }

    /// The literal this deployment fixes, read back directly rather than left to the behavioural
    /// cells above to catch by inference. Flipping `legacy_session_mode` back to the SDK's own
    /// default (`true`) WOULD also redden the two cells above - our fixture's `tools/list` request
    /// carries no `Mcp-Session-Id` from a prior `initialize`, so it would fall into the session-based
    /// branch's "no session id at all" arm and be refused outright as a malformed exchange, not
    /// answered - but that failure shape (a protocol-level refusal, not a scope mismatch) is the
    /// wrong signal for THIS property; asserting the field directly names the actual thing that
    /// changed.
    ///
    /// **Only these two fields are contractual**: `StreamableHttpServerConfig` has ten fields, the
    /// other eight are inherited from the SDK's `Default` through the builder ([`config`] applies
    /// both pins through the builder precisely because the struct is `#[non_exhaustive]`), and this
    /// cell asserts exactly what this module decides - `legacy_session_mode` (stateless sessions)
    /// and `json_response` - and nothing it merely inherits.
    #[test]
    fn the_streamable_http_config_pins_stateless_sessions() {
        let config = super::config();
        assert!(!config.legacy_session_mode, "{config:?}");
        assert!(config.json_response, "{config:?}");
    }
}
