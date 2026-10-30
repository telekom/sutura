//! The Flight surface subtree, moved out of `router.rs` by the thousand-line limit.

use axum::body::Body;
use axum::http::StatusCode;
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use axum::response::Response;
use sutura_config::InboundIdentity;

use super::{RouterNotBuilt, Ungoverned, host_checked, inbound_layered};
use crate::host::HostAllowlist;
use crate::middleware;
use crate::state::ServiceState;

/// The Flight surface subtree, or no mount when this build and deployment carry none.
///
/// The Flight twin of [`super::agent_subtree`], simpler for the reason [`crate::state::FlightMount`]'s own
/// doc states: no spend-headroom gauge to cross-check, because leg 2 charges its own ledger at the
/// source. Behind the same `establish_asked`/`inbound_layered` layers, the same deployment token
/// gate, `Host` check and rate limiter `/v1` and `/mcp` run behind - see [`super::agent_subtree`] for why
/// that ordering is what it is.
pub(super) fn flight_subtree(
    state: &ServiceState,
    declared: Option<&InboundIdentity>,
    hosts: Option<&HostAllowlist>,
    api: Option<&middleware::RateLimit>,
) -> Result<Option<Ungoverned>, RouterNotBuilt> {
    let Some(mount) = state.flight_surface() else {
        return Ok(None);
    };
    let refusals = state.settings().flight_surface_refusals();
    if !refusals.is_empty() {
        return Err(RouterNotBuilt::FlightSurfaceNotFitToServe { refusals });
    }
    let mount = mount.ungoverned().layered(|router| {
        router.route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::capability::establish_leg_asked,
        ))
    });
    let mount = mount.try_layered(|router| inbound_layered("flight", router, state, declared))?;
    let mount = mount
        .layered(|router| router.route_layer(axum::middleware::from_fn_with_state(state.clone(), middleware::require_token)));
    let mount = mount.layered(|router| host_checked(router, hosts));
    let mount = mount.layered(|router| match api {
        Some(layer) => router.route_layer(layer.clone()),
        None => router.route_layer(middleware::disabled_rate_limit_layer()),
    });
    Ok(Some(mount.layered(|router| {
        router.route_layer(axum::middleware::map_response(bodiless))
    })))
}

/// A refusal from a layer in front of the service, with its body dropped.
///
/// Leg 1, the token gate, the `Host` check and the rate limiter answer a plain HTTP status with a
/// JSON body, which a gRPC client decodes as message frames and reports as `Internal`. With no body
/// it maps the status itself, by gRPC's HTTP-to-status table: `401` to `Unauthenticated`, `403` to
/// `PermissionDenied`, `429` to `Unavailable`. The service's own answers are `200` and pass as they
/// are.
async fn bodiless(response: Response) -> Response {
    if response.status() == StatusCode::OK {
        return response;
    }
    let (mut parts, _body) = response.into_parts();
    drop(parts.headers.remove(CONTENT_LENGTH));
    drop(parts.headers.remove(CONTENT_TYPE));
    Response::from_parts(parts, Body::empty())
}
