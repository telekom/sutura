//! The router a `server.role: worker` node serves: liveness, the token-gated `/metrics` and the
//! worker subtree, inside the same outer layers [`super::assemble`] applies.
//!
//! There is no `/v1`, `/mcp`, Flight or documentation route, so a request for one is the router's
//! `404`. Liveness and `/metrics` keep their own limiter tiers and gates as in `assemble`.
//!
//! **Limits.** As on `assemble`'s liveness and `/metrics`, no host check runs here. The registry
//! holds the transport series alone, so a scrape reports no admission or spend series for a worker.

use std::sync::Arc;

use axum::Router;
use sutura_config::Settings;
use sutura_runtime::metrics::RegistryBuilder;
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
    worker::certified(mount).merge_into(&mut router, &mut ungoverned);
    check_ungoverned(&ungoverned)?;
    let router = outer(router, settings, &observed, crate::correlation::from_the_os);
    middleware::spawn_reaper(&observed, &limiters, middleware::REAP_INTERVAL)
        .map_err(|cause| RouterNotBuilt::Reaper { cause })?;
    Ok(router)
}

/// `#[cfg(test)]` on the declaration for `telekom/sutura#657`'s reason.
#[cfg(test)]
mod tests;
