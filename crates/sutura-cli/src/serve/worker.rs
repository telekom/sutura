//! A `server.role: worker` or `reading-worker` node: the worker transport as its one surface, and
//! for a `reading-worker` its own declared catalog and sources opened beside it.
//!
//! `Settings::load` has refused a `worker` that declares a source or a catalog, a `reading-worker`
//! that declares no source or no catalog, and either role naming no `server.tls_node_authority` or
//! enabling `/mcp` or Flight, so what is left is this node's worker, built from the listener's pair
//! and the node authority, served behind the node-certificate gate. `server.nodes` is not read.
//!
//! **A `reading-worker` opens its catalog and its declared sources at boot, through the same
//! `crate::catalog::open_catalog`/`load` and `crate::serve::open_engine` a serving node's own boot
//! uses** - never a second copy of either constructor. A `worker` still opens neither, having
//! declared neither.
//!
//! **Limits.** As on a serving node, the pair, the authority and - for a `reading-worker` - the
//! catalog and the sources are all read once, at boot: a catalog that is re-pinned later is not
//! re-opened here (`crate::serve::refresh` is wired only into the serving root). The worker admits
//! only a call that carries its question's task grant, under the key the first call of that
//! question installed: a node of the same authority installs that key. **Nothing reads a partition
//! yet.** A `reading-worker` still serves the same worker transport as a `worker`, which refuses
//! every stage whose leaf reads a source: opening a source here only proves it is reachable and
//! lets [`sutura_app::recertify_leg`] check a leg against it; reading a granted partition needs a
//! task grant that names it, which the grant does not carry, and a `PartitionReader` port that does
//! not exist yet - a later slice's.

use sutura_config::{RuntimeSettings, ServerRole, Settings, TlsMaterial};
use sutura_runtime::{Shutdown, banner, shutdown, telemetry};

/// Builds this node's worker, then serves it until a signal stops the process.
///
/// # Errors
///
/// No node authority, a pair or an authority that does not make a node's identity, or a router,
/// runtime or listener that cannot start.
pub(super) fn run(settings: &Settings) -> Result<(), String> {
    let Some((tls, authority)) = settings
        .server()
        .tls()
        .and_then(|tls| tls.node_authority().map(|authority| (tls, authority)))
    else {
        return Err(String::from("a worker needs server.tls_node_authority, and it is not set"));
    };
    let mount = mount(tls, authority, settings.runtime())?;
    telemetry::install(settings.telemetry()).map_err(super::flatten)?;
    sutura_runtime::install_panic_hook();
    banner::announce(settings);
    let router = sutura_http::worker_router(settings, &mount).map_err(super::flatten)?;
    if settings.server().role() == ServerRole::ReadingWorker {
        open_reading_sources(settings)?;
        tracing::info!("serving as a reading worker: catalog and declared sources opened, no /v1, /mcp or Flight");
    } else {
        tracing::info!("serving as a worker: no catalog or source opened, no /v1, /mcp or Flight");
    }
    let address = settings.server().bind().socket();
    let material = settings.server().tls().cloned();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|cause| format!("the async runtime could not be built: {cause}"))?;
    let stopping = Shutdown::with_grace(settings.runtime().shutdown_grace().duration());
    let served = runtime.block_on(async {
        drop(tokio::spawn(shutdown::listen(stopping.clone())));
        super::serve_as_configured(router, address, stopping.clone(), material).await
    });
    super::stop(runtime, &stopping);
    served
}

/// This node's worker, accepting only a node `authority` signed. Built with no runtime entered,
/// as the serving arm builds its own. A message sent to it whose length prefix declares more bytes
/// than this node's working set is refused before any of it is buffered; the bound is per message,
/// so concurrent streams each reach it.
pub(super) fn mount(
    tls: &TlsMaterial,
    authority: &std::path::Path,
    runtime: RuntimeSettings,
) -> Result<sutura_http::WorkerMount, String> {
    let identity = super::nodes::identity(tls, authority)?;
    let working_set = crate::sources::working_set(runtime);
    let own = sutura_exec_datafusion::Nodes::worker(working_set, identity)
        .map_err(|cause| format!("{}\ncould not build this node's worker", super::flatten(cause)))?;
    Ok(sutura_http::WorkerMount::new(
        own.with_call_authorizer(sutura_exec_datafusion::TaskGrant::default())
            .into_worker_server()
            .max_decoding_message_size(working_set.bytes()),
    ))
}

/// Opens a `reading-worker`'s own declared catalog and its declared sources, through the exact
/// constructors `crate::serve::run`'s own boot calls for a serving node - `crate::catalog::open_catalog`,
/// `crate::catalog::load`, then `super::open_engine`.
///
/// **What this proves, and what it does not.** A failure here means the catalog or a source could
/// not be opened - the same refusal a serving node gives for the same misconfiguration - and stops
/// this process before it serves. Nothing is kept: the pinned catalog and the opened sources are
/// dropped on return, so a source that becomes unreachable after boot is not noticed here, and the
/// slice that reads a partition is the one that holds them for `sutura_app::recertify_leg`.
fn open_reading_sources(settings: &Settings) -> Result<(), String> {
    let outbound = super::outbound::resolve(settings)?;
    let catalogs = crate::catalog::open_catalog(settings.catalogs(), outbound.as_ref())?;
    let pinned = crate::catalog::load(&catalogs)?;
    let _opened = super::open_engine(
        &pinned,
        settings.sources(),
        settings.runtime(),
        settings.server().request_timeout(),
        outbound.as_ref(),
    )?;
    Ok(())
}

#[cfg(test)]
mod reading;
#[cfg(test)]
mod tests;
