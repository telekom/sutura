//! The router a `server.role: worker` node serves: liveness, the token-gated `/metrics` and the
//! worker subtree, inside the same outer layers [`super::assemble`] applies.
//!
//! There is no `/v1`, `/mcp`, Flight or documentation route, so a request for one is the router's
//! `404`. Liveness and `/metrics` keep their own limiter tiers and gates as in `assemble`.
//!
//! The registry holds the transport series and `sutura_worker_requests_in_flight`: the node calls
//! the worker subtree is answering, each counted from its arrival until its response body is
//! dropped, which the server does once the body has ended or the caller has gone.
//!
//! **Limits.** As on `assemble`'s liveness and `/metrics`, no host check runs here. A scrape reports
//! no admission or spend series for a worker. The in-flight count is of requests, not of stages: a
//! stage asked for in several streams counts once per stream, so zero means idle and a larger number
//! means busier, nothing finer. A call the certificate check refuses is counted until its empty
//! body is dropped.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use http_body::{Frame, SizeHint};
use sutura_config::Settings;
use sutura_runtime::metrics::{Gauge, RegistryBuilder};
use utoipa_axum::router::OpenApiRouter;

use super::{RouterNotBuilt, announce_keying, announce_rate_limiting, check_ungoverned, limiter, metrics, outer, worker};
use crate::client_address::ClientAddress;
use crate::metrics::Metrics;
use crate::middleware;
use crate::state::WorkerMount;

/// Builds a worker node's router, and starts the sweeper for its keyed state.
///
/// # Errors
///
/// [`RouterNotBuilt::Limiter`] when a rate limit tier cannot be built, [`RouterNotBuilt::Reaper`]
/// when its sweeper cannot start, and [`RouterNotBuilt::UngovernedRouteNotAllowlisted`] as in
/// [`super::assemble`].
pub fn worker_router(settings: &Settings, mount: &WorkerMount) -> Result<Router, RouterNotBuilt> {
    let mut builder = RegistryBuilder::default();
    let observed = Metrics::install(&mut builder);
    let in_flight = builder.gauge("sutura_worker_requests_in_flight");
    let registry = Arc::new(builder.build());
    let limits = settings.rate_limit();
    let key = ClientAddress::from_settings(limits);
    announce_rate_limiting(settings.environment(), limits.enabled());
    announce_keying(limits);
    let mut limiters = Vec::new();

    let public = Router::from(OpenApiRouter::new().routes(utoipa_axum::routes!(crate::routes::health::liveness)));
    let public = if limits.enabled() {
        let (layer, handle) = middleware::probe_rate_limit_layer(&observed, limits.probe(), key.clone()).map_err(limiter)?;
        limiters.push(handle);
        public.layer(layer)
    } else {
        public.layer(middleware::disabled_rate_limit_layer())
    };
    let scrape = Router::from(
        OpenApiRouter::new()
            .routes(utoipa_axum::routes!(metrics::scrape))
            .with_state(metrics::MetricsState::new(registry)),
    )
    .route_layer(axum::middleware::from_fn_with_state(
        settings.security().metrics_token().cloned().map(Arc::new),
        middleware::require_metrics_token,
    ));
    let scrape = if limits.enabled() {
        let (layer, handle) = middleware::metrics_rate_limit_layer(&observed, limits.probe(), key).map_err(limiter)?;
        limiters.push(handle);
        scrape.layer(layer)
    } else {
        scrape.layer(middleware::disabled_rate_limit_layer())
    };

    let mut router = Router::new().merge(scrape).merge(public);
    let mut ungoverned = Vec::new();
    worker::certified(mount)
        .layered(|router| router.route_layer(axum::middleware::from_fn_with_state(in_flight, counted)))
        .merge_into(&mut router, &mut ungoverned);
    check_ungoverned(&ungoverned)?;
    let router = outer(router, settings, &observed, crate::correlation::from_the_os);
    middleware::spawn_reaper(&observed, &limiters, middleware::REAP_INTERVAL)
        .map_err(|cause| RouterNotBuilt::Reaper { cause })?;
    Ok(router)
}

/// Counts `request` on `in_flight` until its response body is dropped, or until the call is.
async fn counted(State(in_flight): State<Gauge>, request: Request, next: Next) -> Response {
    in_flight.adjust(1);
    let held = Held(in_flight);
    next.run(request).await.map(|body| Body::new(Streaming { body, _held: held }))
}

/// One call's place in the in-flight count, given back when it is dropped.
struct Held(Gauge);

impl Drop for Held {
    fn drop(&mut self) {
        self.0.adjust(-1);
    }
}

/// A response body that keeps its call counted for as long as the server holds it.
struct Streaming {
    body: Body,
    _held: Held,
}

impl http_body::Body for Streaming {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        Pin::new(&mut self.get_mut().body).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.body.size_hint()
    }
}

/// `#[cfg(test)]` on the declaration for `telekom/sutura#657`'s reason.
#[cfg(test)]
mod tests;
