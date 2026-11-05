//! What each worker role's configuration must and may not declare.
//!
//! A file of its own because `settings.rs` is at the thousand-line limit `cargo xtask max-lines`
//! enforces. A child of `settings` so it reads the private fields `Settings::refusals` reads.

use super::layers::LayerOrigin;
use super::{NotFitToServe, Settings};
use crate::server::ServerRole;

/// Every reason a worker configuration is not fit to run as its role, or an empty list for
/// `serving`.
///
/// A `worker` declares no source and no catalog. A `reading-worker` declares both. Either needs a
/// node authority, and neither mounts `/mcp` or Flight.
///
/// **The limit on `catalogs`:** it is read off the layer that supplied the key, so a catalog counts
/// when a file or a variable wrote it. A text overlay is stamped like the embedded defaults, so a
/// catalog written there is not seen, and a `reading-worker` whose catalog is only there is refused
/// as declaring none; the binary reads files and variables only.
pub(super) fn worker_refusals(settings: &Settings) -> Vec<NotFitToServe> {
    let declares_catalogs = matches!(
        settings.layers.origin_of("catalogs"),
        Some(LayerOrigin::File(_) | LayerOrigin::Environment)
    );
    let mut refusals = Vec::new();
    match settings.server.role() {
        ServerRole::Serving => return refusals,
        ServerRole::Worker => {
            if !settings.sources.is_empty() {
                refusals.push(NotFitToServe::WorkerDeclaresSources {
                    count: settings.sources.count(),
                });
            }
            if declares_catalogs {
                refusals.push(NotFitToServe::WorkerDeclaresCatalogs);
            }
        }
        ServerRole::ReadingWorker => {
            if settings.sources.is_empty() {
                refusals.push(NotFitToServe::ReadingWorkerWithoutSources);
            }
            if !declares_catalogs {
                refusals.push(NotFitToServe::ReadingWorkerWithoutCatalog);
            }
        }
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

/// Every declared peer's source alias that this deployment's own `sources:` section does not
/// declare - `#828`'s cross-field check, over the already-parsed [`crate::sources::SourceRegistry`]
/// a per-field `TlsMaterial` parse cannot see.
///
/// **Only for `ServerRole::Serving`.** A `worker` or `reading-worker` reads `server.nodes` for the
/// same peers a serving node declares - its own entry among them is a `worker` or `reading-worker`
/// too - so without this guard a worker whose peers include a `reading-worker` reading a source the
/// worker itself never declares (workers declare none, by `#828`'s decision 7) would be refused for
/// a fact that describes a different node, not this one.
pub(super) fn declared_node_refusals(settings: &Settings) -> Vec<NotFitToServe> {
    if settings.server.role() != ServerRole::Serving {
        return Vec::new();
    }
    let Some(nodes) = settings.server.tls().and_then(crate::server::TlsMaterial::nodes) else {
        return Vec::new();
    };
    nodes
        .others()
        .iter()
        .flat_map(|node| {
            node.sources()
                .iter()
                .filter(|alias| settings.sources.get(alias).is_none())
                .map(|alias| NotFitToServe::DeclaredNodeReadsUndeclaredSource {
                    url: node.url().to_string(),
                    alias: alias.as_str().to_owned(),
                })
        })
        .collect()
}

/// Every declared peer reading a source this deployment serves, on a serving node that verifies
/// no caller - `security.inbound` absent.
///
/// A lookup leg such a source splits is granted to its readers for the verified caller who asked,
/// and `sutura_app` refuses one asked by nobody verified before the source is asked anything. So
/// without leg 1 every such leg is refused at question time; this refuses the deployment before a
/// listener binds instead. An alias this deployment does not declare is
/// [`NotFitToServe::DeclaredNodeReadsUndeclaredSource`]'s, not this one's.
///
/// **The limit:** it reads the declaration. A deployment with `security.inbound` whose callers
/// reach `/v1` with no verified subject still meets the question-time refusal.
pub(super) fn reading_node_refusals(settings: &Settings) -> Vec<NotFitToServe> {
    if settings.server.role() != ServerRole::Serving || settings.security.inbound().is_some() {
        return Vec::new();
    }
    let Some(nodes) = settings.server.tls().and_then(crate::server::TlsMaterial::nodes) else {
        return Vec::new();
    };
    nodes
        .others()
        .iter()
        .flat_map(|node| {
            node.sources()
                .iter()
                .filter(|alias| settings.sources.get(alias).is_some())
                .map(|alias| NotFitToServe::ReadingNodeWithoutInboundIdentity {
                    url: node.url().to_string(),
                    alias: alias.as_str().to_owned(),
                })
        })
        .collect()
}

#[cfg(test)]
mod reading;
#[cfg(test)]
mod tests;
