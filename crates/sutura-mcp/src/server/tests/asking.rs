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
//! `pub(crate)` in the pinned SDK (`rmcp`'s `src/service.rs:801`), so nothing outside the `rmcp`
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
use sutura_domain::identity::{PrincipalChain, RequestContext as PrincipalContext, Subject, SubjectId};

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
    let subject = Subject::Verified {
        id: SubjectId::parse(id).expect("a test subject id is a subject id"),
    };
    Asked::established(PrincipalContext::of(PrincipalChain::of(subject)), permitted)
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
}

/// **Two callers over one connection are two different askers.** The SAME [`AgentSurface`], the SAME
/// [`Peer`], and two hand-built contexts differing only in which `Asked` their `Parts` carry - if
/// the identity were read once and cached (the trap `docs/adr/0023` names: a value stored on `self`
/// or bound to the session rather than read from each request), the second caller would see or do
/// what the first was granted. It does not, on both halves this module's own docs separate:
/// presentation (`tools/list`) and the control (`tools/call`).
#[tokio::test]
async fn two_callers_over_one_connection_are_two_different_askers() {
    let surface = AgentSurface::new(
        Arc::new(testing::FailingSurface::new()),
        Asking::PerRequest,
        CatalogProse::Quoted,
        super::admission(""),
        super::reply(""),
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

    // Bob, granted the scope, is not refused at this channel - whatever the fake port under him
    // does next is a different assertion this fixture is not built to make.
    let answered = surface
        .call_tool(
            super::ask(&super::a_certified_question()),
            hand_built_context(4, peer, Some(everything)),
        )
        .await;
    assert!(
        answered.is_ok(),
        "the caller granted the scope must reach the port rather than be refused: {answered:?}"
    );
}
