//! The worker transport subtree: a node's half of a query planned across nodes.
//!
//! None of the layers `/v1`, `/mcp` and Flight run behind: the caller is a node, not a person, so
//! there is no subject to establish, no deployment token and no capability to scope. What stands in
//! their place is the connection: [`certified_node`] answers only a request whose client presented a
//! certificate the node authority signed, which `crate::server::Peer` reads off the TLS stream. A
//! plaintext deployment, or one with no node authority, never carries that marker, so the mount
//! refuses every request there rather than serving one.
//!
//! **Limits.** The marker says that a node connected, not which one, and nothing yet says which task
//! it may run. The host check, the rate limiter and the request bound's carve-outs are not applied
//! here beyond what the outer router layers on every route.

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse as _, Response};

use super::Ungoverned;
use crate::server::NodeCertified;
use crate::state::{ServiceState, WorkerMount};

/// The worker subtree, or no mount when this build and deployment carry none.
pub(super) fn worker_subtree(state: &ServiceState) -> Option<Ungoverned> {
    state.worker_surface().map(certified)
}

/// `mount` behind [`certified_node`].
pub(super) fn certified(mount: &WorkerMount) -> Ungoverned {
    mount
        .ungoverned()
        .layered(|router| router.route_layer(axum::middleware::from_fn(certified_node)))
}

/// Passes a request on only when its connection presented a node certificate.
///
/// The refusal has no body, so a gRPC client maps the `401` to `Unauthenticated` rather than
/// decoding a JSON body as message frames.
async fn certified_node(request: Request, next: Next) -> Response {
    if request.extensions().get::<NodeCertified>().is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    next.run(request).await
}

/// `#[cfg(test)]` on the declaration for `telekom/sutura#657`'s reason.
#[cfg(test)]
mod tests;
