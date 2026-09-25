//! `Asking::PerRequest`: the refusal, and that two calls on one connection are two different
//! askers.
//!
//! # Why these cells cannot reuse `super::served`/`permitting`/`connected`
//!
//! Every other test in this suite drives [`AgentSurface`] through a REAL `rmcp` client over an
//! in-memory pipe, which is the right shape for `Asking::TheProcessOwner` - the identity is fixed
//! for the whole connection, so one client is one caller. `Asking::PerRequest` is the opposite
//! claim: the SAME surface, on the SAME connection, must answer two different callers differently.
//! Nothing about the SDK's own client lets a test vary `rmcp::service::RequestContext::extensions`
//! per call - that value is produced by the transport, not by a caller's request - so these cells
//! call [`ServerHandler::call_tool`]/[`ServerHandler::list_tools`] directly, as the plain async
//! trait methods they are, with a hand-built `RequestContext`.
//!
//! # What "hand-built" means, precisely
//!
//! `rmcp::service::RequestContext`'s fields are `pub` and `RequestContext::new(id, peer)` is `pub`,
//! but its `peer: rmcp::service::Peer<RoleServer>` field cannot be built from nothing: `Peer::new` is
//! `pub(crate)` in the pinned SDK (`rmcp`'s `src/service.rs`), so nothing outside the `rmcp`
//! crate can mint one directly. [`a_peer`] gets a real one the only way available - a throwaway
//! handshake over an in-memory pipe, against a handler ([`Nobody`]) that does nothing - and every
//! test below reuses the SAME peer across its several hand-built contexts, the way `rmcp`'s own
//! session-mode transport would reuse one `Peer` across many requests on one connection.
//!
//! What a real HTTP transport injects into `context.extensions` is an `http::request::Parts`
//! (`docs/adr/0023`, quoting the vendored `streamable_http_server/tower.rs`), and
//! `sutura_http::capability::establish_asked` inserts a `sutura_app::Asked` into THAT value's own
//! extensions - so [`parts_carrying`] builds the same shape, not a shortcut past it.

use std::sync::Arc;

use rmcp::model::{ErrorCode, NumberOrString};
use rmcp::service::{Peer, RequestContext, RoleServer};
use rmcp::{ServerHandler, serve_client, serve_server};
use sutura_app::prompt::CatalogProse;
use sutura_app::{Asked, Capability, Permitted};
use sutura_domain::identity::{PrincipalChain, RequestContext as PrincipalContext, Subject};

use crate::testing;
use crate::{AgentSurface, Asking};

/// A handler that answers nothing and refuses nothing - the minimum `ServerHandler` impl, every
/// method left at its SDK-provided default. Exists only so [`a_peer`] has something to hand a
/// throwaway `serve_server` call; nothing in this file drives a request through it.
struct Nobody;

impl ServerHandler for Nobody {}

/// A real `Peer<RoleServer>`, the only way to hold one - see the module documentation for why
/// `Peer::new` is not an option. What is driven through it below never touches the peer itself:
/// `call_tool` and `list_tools` read only `context.ct` and `context.extensions`, neither of which
/// this throwaway handshake's own traffic shapes - so a minimal handshake over a small in-memory
/// pipe is enough.
async fn a_peer() -> Peer<RoleServer> {
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

/// An `http::request::Parts` carrying one `Asked` in its OWN extensions - the exact shape
/// `sutura_http::capability::establish_asked` produces on the HTTP transport today, and the shape
/// `AgentSurface::asked` reads under `Asking::PerRequest`.
fn parts_carrying(asked: Asked) -> http::request::Parts {
    let (mut parts, ()) = http::Request::builder()
        .body(())
        .expect("a bodyless request builds")
        .into_parts();
    drop(parts.extensions.insert(asked));
    parts
}

/// A hand-built `RequestContext<RoleServer>` over a shared `peer` - `asked` is the caller this
/// request carries, or `None` for a request whose transport established nobody at all.
fn hand_built_context(id: i64, peer: Peer<RoleServer>, asked: Option<Asked>) -> RequestContext<RoleServer> {
    let mut context = RequestContext::new(NumberOrString::Number(id), peer);
    if let Some(asked) = asked {
        drop(context.extensions.insert(parts_carrying(asked)));
    }
    context
}

/// A caller `Asked` naming a distinct subject, so two fixtures in one test are provably two
/// different callers rather than the same value written twice.
fn subject_asked(id: &str, permitted: Permitted) -> Asked {
    let subject = Subject::verified(id).expect("a test subject id is a subject id");
    Asked::established(PrincipalContext::of(PrincipalChain::of(subject)), permitted)
}

/// The same as [`subject_asked`], but with a deployment-mapped audience grant on the caller's own
/// context - which is what `docs/adr/0028`'s `scoped_for` reads to decide what this caller may see.
fn audience_asked(
    id: &str,
    permitted: Permitted,
    audiences: std::collections::BTreeSet<sutura_domain::model::AudienceId>,
) -> Asked {
    let subject = Subject::verified(id).expect("a test subject id is a subject id");
    let context =
        PrincipalContext::of(PrincipalChain::of(subject)).granting(sutura_domain::catalog::GrantedAudiences::of(audiences));
    Asked::established(context, permitted)
}

/// **The most important mutation in the whole issue - #378's own plan names it that.** A tool call
/// under `Asking::PerRequest` with no `Asked` in the request's own extensions must be refused, and
/// refused as a distinct, typed outcome - never silently answered as `Subject::TheDeploymentItself`
/// the way `Asking::TheProcessOwner` (or a mistaken `unwrap_or_else(principal::established)`) would.
///
/// The audit assertion is what makes the mutation this guards against visible: substituting
/// `crate::principal::established()` for the refusal in `AgentSurface::asked`'s `PerRequest` arm
/// would still return SOME `CallToolResponse` rather than panicking, so the error-code assertion
/// alone is the one a careless substitution could dodge by returning a different error. The record
/// count cannot be dodged the same way: a call answered as the deployment reaches `Surface::answer`,
/// which writes a record through `sink` - a call refused before that point writes none.
#[tokio::test]
async fn a_tool_call_with_no_established_caller_is_refused_and_never_answered_as_the_deployment() {
    let (service, sink) = super::with_sink();
    let surface = AgentSurface::new(
        Arc::new(service),
        Asking::PerRequest,
        CatalogProse::Quoted,
        super::admission(""),
        super::reply(""),
        testing::instructions(),
        None,
    );
    let context = hand_built_context(1, a_peer().await, None);
    let error = surface
        .call_tool(super::ask(&super::a_certified_question()), context)
        .await
        .expect_err("no established caller must be refused, not answered");
    assert_eq!(error.code, ErrorCode::INVALID_REQUEST, "{error:?}");
    assert_eq!(
        sink.calls(),
        0,
        "a call refused for having no established caller must write no audit record at all - in \
         particular never one naming Subject::TheDeploymentItself"
    );

    // The presentation half answers the refusal the same way the call does: `tools/list` must not
    // show an absent caller the deployment's whole tool set just because nobody established them.
    let listed = surface
        .list_tools(None, hand_built_context(2, a_peer().await, None))
        .await
        .expect_err("tools/list must refuse a caller nobody established, not show every tool");
    assert_eq!(listed.code, ErrorCode::INVALID_REQUEST, "{listed:?}");
}

/// **Two callers over one connection are two different askers.** The SAME [`AgentSurface`], the SAME
/// [`Peer`], and two hand-built contexts differing only in which `Asked` their `Parts` carry - if
/// the identity were read once and cached (the trap `docs/adr/0023` names: a value stored on `self`
/// or bound to the session rather than read from each request), the second caller would see or do
/// what the first was granted. It does not, on both halves this module's own docs separate:
/// presentation (`tools/list`) and the control (`tools/call`).
#[tokio::test]
async fn two_callers_over_one_connection_are_two_different_askers() {
    let recording = Arc::new(testing::RecordingSurface::new());
    let surface = AgentSurface::new(
        Arc::clone(&recording),
        Asking::PerRequest,
        CatalogProse::Quoted,
        super::admission(""),
        super::reply(""),
        testing::instructions(),
        None,
    );
    let peer = a_peer().await;

    let narrow = subject_asked(
        "alice@example.com",
        Permitted::granted_by([Capability::DescribeCatalog.scope()]),
    );
    let everything = subject_asked("bob@example.com", Permitted::every_capability());

    // Presentation: each caller is shown only what it was granted, on the SAME surface instance.
    let shown_to_alice = surface
        .list_tools(None, hand_built_context(1, peer.clone(), Some(narrow.clone())))
        .await
        .expect("tools/list answers")
        .tools;
    let names: Vec<&str> = shown_to_alice.iter().map(|tool| tool.name.as_ref()).collect();
    assert_eq!(names, [Capability::DescribeCatalog.id()], "{names:?}");

    let shown_to_bob = surface
        .list_tools(None, hand_built_context(2, peer.clone(), Some(everything.clone())))
        .await
        .expect("tools/list answers")
        .tools;
    assert_eq!(shown_to_bob.len(), Capability::every().count(), "{shown_to_bob:?}");

    // The control: alice is refused the tool her scope does not name, on this same connection's peer.
    let refused = surface
        .call_tool(
            super::ask(&super::a_certified_question()),
            hand_built_context(3, peer.clone(), Some(narrow)),
        )
        .await;
    let error = refused.expect_err("a caller without the scope must be refused, not answered");
    assert_eq!(error.code, ErrorCode::METHOD_NOT_FOUND, "{error:?}");

    let bob = Subject::verified("bob@example.com").expect("a test subject id is a subject id");

    // The control's second half, held at the PORT rather than at the channel: bob, granted the
    // scope, is not refused - and the identity his `ask_metric` reaches `Surface::answer` under is
    // bob, never `Subject::TheDeploymentItself`. `RecordingSurface` records the subject of each
    // context the port is handed, so a transport that resolved the caller and then read the
    // deployment's own identity instead would fail the assertion the way a substitution in the
    // handler cannot: the wrong subject is the one the port sees.
    surface
        .call_tool(
            super::ask(&super::a_certified_question()),
            hand_built_context(4, peer, Some(everything)),
        )
        .await
        .expect("bob's ask_metric must not be refused at the channel");
    assert_eq!(
        recording.subjects(),
        [bob],
        "bob's ask_metric must reach the port as Subject::Verified, never Subject::TheDeploymentItself"
    );
}

/// The raw door of the driving port is held to the same identity claim as the ask door: a
/// `run_sql` call answers as the caller this request's `Asked` named, never as the deployment.
///
/// Its own cell, rather than riding inside the two-callers test, because the two arms are two
/// substitutions a careless change could make separately - and the proof that either is caught
/// needs an assertion that fails on its own, not one masked by the other's failure first.
#[tokio::test]
async fn run_sql_reaches_the_port_as_the_caller_this_request_named() {
    let recording = Arc::new(testing::RecordingSurface::new());
    let surface = AgentSurface::new(
        Arc::clone(&recording),
        Asking::PerRequest,
        CatalogProse::Quoted,
        super::admission(""),
        super::reply(""),
        testing::instructions(),
        None,
    );
    let peer = a_peer().await;
    // The raw door only exists under a grant that names it - bob is granted every capability, which
    // is how a deployment turns this tool on for a caller.
    let everything = subject_asked("bob@example.com", Permitted::every_capability());

    surface
        .call_tool(
            super::raw(&serde_json::json!({ "statement": "select 1" })),
            hand_built_context(1, peer, Some(everything)),
        )
        .await
        .expect("bob's run_sql must not be refused at the channel");
    assert_eq!(
        recording.subjects(),
        [Subject::verified("bob@example.com").expect("a test subject id is a subject id")],
        "bob's run_sql must reach the port as Subject::Verified, never Subject::TheDeploymentItself"
    );
}

/// Two verified callers with different audience grants get two different `describe_catalog`
/// listings from ONE surface instance - `docs/adr/0028`'s invisible-at-both-doors on the agent
/// surface, mirroring `sutura_http`'s `two_verified_callers_get_two_different_catalogs_from_one_bundle`.
///
/// The HTTP twin drives the property through the assembled router; this cell drives it through
/// `AgentSurface` under `Asking::PerRequest`, which is the shape that carries per-call identity on
/// the served MCP surface. `bundle_with_a_restricted_metric` keeps `revenue` open and `finance_only`
/// restricted to the `finance` audience, so the two listings differ exactly where the grants do:
/// the outsider and the finance-granted caller both see `revenue` (the numbers they share), and only
/// the latter sees `finance_only`. On the base commit this cell is RED - every caller got the whole
/// bundle because `describe` built `CatalogContent` from an unscoped `&PinnedDefinitions`.
#[tokio::test]
async fn two_verified_callers_get_two_different_catalogs_from_one_bundle() {
    let surface = AgentSurface::new(
        Arc::new(testing::RestrictedSurface::new()),
        Asking::PerRequest,
        CatalogProse::Quoted,
        super::admission(""),
        super::reply(""),
        testing::instructions(),
        None,
    );
    let peer = a_peer().await;
    let catalog = Permitted::granted_by([Capability::DescribeCatalog.scope()]);
    let finance = sutura_domain::model::AudienceId::parse("finance").expect("a test audience id is one");
    let outsider = subject_asked("outsider@example.com", catalog.clone());
    let finance_caller = audience_asked(
        "finance-caller@example.com",
        catalog,
        std::collections::BTreeSet::from([finance]),
    );

    let seen_by_outsider = surface
        .call_tool(super::describe(), hand_built_context(1, peer.clone(), Some(outsider)))
        .await
        .expect("the catalog tool answers an outsider");
    let seen_by_finance = surface
        .call_tool(super::describe(), hand_built_context(2, peer, Some(finance_caller)))
        .await
        .expect("the catalog tool answers a finance-granted caller");

    // `call_tool` answers with `CallToolResponse`, which wraps the completed `CallToolResult` in its
    // `Complete` arm - never the input-required or task arms, which no tool here returns.
    let outsider_result = match seen_by_outsider {
        rmcp::model::CallToolResponse::Complete(result) => result,
        other => panic!("a catalog tool call completes, got {other:?}"),
    };
    let finance_result = match seen_by_finance {
        rmcp::model::CallToolResponse::Complete(result) => result,
        other => panic!("a catalog tool call completes, got {other:?}"),
    };

    let outsider_json = outsider_result
        .structured_content
        .expect("the catalog carries structured content");
    let finance_json = finance_result
        .structured_content
        .expect("the catalog carries structured content");

    // The metric the two callers share is present for both - identity narrowing never removes what
    // everyone may see, which is what keeps this from becoming accidental leg-two row access.
    for listing in [&outsider_json, &finance_json] {
        assert!(
            listing.to_string().contains("\"revenue\""),
            "the open metric must be visible to every caller: {listing}"
        );
    }

    // Only the finance-granted caller sees the restricted metric - the whole point of a per-identity
    // view on the served surface.
    assert!(
        !outsider_json.to_string().contains("\"finance_only\""),
        "an outsider must not see the restricted metric in describe_catalog: {outsider_json}"
    );
    assert!(
        finance_json.to_string().contains("\"finance_only\""),
        "a finance-granted caller must see the restricted metric in describe_catalog: {finance_json}"
    );
}

/// **Issue #971's acceptance, half two: a caller scoped away from a metric does not receive that
/// metric's knowledge.**
///
/// The glossary entry names the `finance`-restricted `finance_only` metric. An outsider may not see
/// the metric, so it must not receive the entry's meaning either - the same invisible-at-both-doors
/// rule `docs/adr/0028` applies to the metric listing, now applied to the knowledge that stays with
/// it. `RestrictedKnowledgeSurface` keeps `revenue` open and `finance_only` restricted; only the
/// finance-granted caller's descriptor carries `capital expense`.
#[tokio::test]
async fn a_caller_scoped_away_from_a_metric_does_not_receive_that_metrics_knowledge() {
    let surface = AgentSurface::new(
        Arc::new(testing::RestrictedKnowledgeSurface::new()),
        Asking::PerRequest,
        CatalogProse::Quoted,
        super::admission(""),
        super::reply(""),
        testing::instructions(),
        None,
    );
    let peer = a_peer().await;
    let catalog = Permitted::granted_by([Capability::DescribeCatalog.scope()]);
    let finance = sutura_domain::model::AudienceId::parse("finance").expect("a test audience id is one");
    let outsider = subject_asked("outsider@example.com", catalog.clone());
    let finance_caller = audience_asked(
        "finance-caller@example.com",
        catalog,
        std::collections::BTreeSet::from([finance]),
    );

    let seen_by_outsider = surface
        .call_tool(super::describe(), hand_built_context(1, peer.clone(), Some(outsider)))
        .await
        .expect("the catalog tool answers an outsider");
    let seen_by_finance = surface
        .call_tool(super::describe(), hand_built_context(2, peer, Some(finance_caller)))
        .await
        .expect("the catalog tool answers a finance-granted caller");

    let outsider_result = match seen_by_outsider {
        rmcp::model::CallToolResponse::Complete(result) => result,
        other => panic!("a catalog tool call completes, got {other:?}"),
    };
    let finance_result = match seen_by_finance {
        rmcp::model::CallToolResponse::Complete(result) => result,
        other => panic!("a catalog tool call completes, got {other:?}"),
    };

    let outsider_json = outsider_result
        .structured_content
        .expect("the catalog carries structured content");
    let finance_json = finance_result
        .structured_content
        .expect("the catalog carries structured content");

    let outsider_knowledge = outsider_json
        .get("knowledge")
        .and_then(serde_json::Value::as_str)
        .expect("the catalog carries a knowledge section");
    let finance_knowledge = finance_json
        .get("knowledge")
        .and_then(serde_json::Value::as_str)
        .expect("the catalog carries a knowledge section");

    assert!(
        !outsider_knowledge.contains("capital expense"),
        "an outsider must not receive the restricted metric's glossary meaning: {outsider_knowledge}"
    );
    assert!(
        finance_knowledge.contains("capital expense"),
        "a finance-granted caller must receive the restricted metric's glossary meaning: {finance_knowledge}"
    );
    // The same scoping holds for the caveat and the worked example that name the restricted metric.
    assert!(
        !outsider_knowledge.contains("only a finance-granted caller should trust this number"),
        "an outsider must not receive the restricted metric's caveat: {outsider_knowledge}"
    );
    assert!(
        finance_knowledge.contains("only a finance-granted caller should trust this number"),
        "a finance-granted caller must receive the restricted metric's caveat: {finance_knowledge}"
    );
    assert!(
        !outsider_knowledge.contains("ask exactly this"),
        "an outsider must not receive the restricted metric's worked example: {outsider_knowledge}"
    );
    assert!(
        finance_knowledge.contains("ask exactly this"),
        "a finance-granted caller must receive the restricted metric's worked example: {finance_knowledge}"
    );
    // The absence declaration: the caller-scoped view withdraws `Absences`, so it must render the
    // honest "cannot record - infer NOTHING" claim - never "kept and empty", which would be a false
    // negative inviting an agent to approximate a term it should decline. The finance caller (also a
    // caller scoped view, despite its wider grant) must not read the withheld list as empty either.
    assert!(
        outsider_knowledge.contains("infer NOTHING"),
        "a caller-scoped view must tell the caller to infer nothing from an absent term: {outsider_knowledge}"
    );
    assert!(
        !outsider_knowledge.contains("that list and it is empty"),
        "a caller-scoped view must not claim the withheld absence list is empty: {outsider_knowledge}"
    );
    assert!(
        !finance_knowledge.contains("that list and it is empty"),
        "any caller-scoped view must not claim the withheld absence list is empty: {finance_knowledge}"
    );
}
