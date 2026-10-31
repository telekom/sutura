//! What a worker configuration may not declare.
//!
//! A file of its own because `settings.rs` is at the thousand-line limit `cargo xtask max-lines`
//! enforces. A child of `settings` so it reads the private fields `Settings::refusals` reads.

use super::layers::LayerOrigin;
use super::{NotFitToServe, Settings};
use crate::server::ServerRole;

/// Every reason a `worker` configuration is not fit to run as one, or an empty list for `serving`.
///
/// **The limit on `catalogs`:** it is read off the layer that supplied the key, so a catalog counts
/// when a file or a variable wrote it. A text overlay is stamped like the embedded defaults, so a
/// catalog written there is not seen; the binary reads files and variables only.
pub(super) fn worker_refusals(settings: &Settings) -> Vec<NotFitToServe> {
    if settings.server.role() != ServerRole::Worker {
        return Vec::new();
    }
    let mut refusals = Vec::new();
    if !settings.sources.is_empty() {
        refusals.push(NotFitToServe::WorkerDeclaresSources {
            count: settings.sources.count(),
        });
    }
    if matches!(
        settings.layers.origin_of("catalogs"),
        Some(LayerOrigin::File(_) | LayerOrigin::Environment)
    ) {
        refusals.push(NotFitToServe::WorkerDeclaresCatalogs);
    }
    if settings
        .server
        .tls()
        .and_then(crate::server::TlsMaterial::node_authority)
        .is_none()
    {
        refusals.push(NotFitToServe::WorkerWithoutNodeAuthority);
    }
    if settings.server.agent_surface_enabled() {
        refusals.push(NotFitToServe::WorkerWithAgentSurface);
    }
    if settings.server.flight_surface_enabled() {
        refusals.push(NotFitToServe::WorkerWithFlightSurface);
    }
    refusals
}

#[cfg(test)]
mod tests;
