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
//! **A `reading-worker` built with the `postgres` adapter keeps them for its partition reader**
//! (`partition`), served through `Nodes::worker_reading`: a source-reading task it admits under a
//! verified reading grant is re-certified against this node's catalog and, for a `PostgreSQL` source,
//! read. A `worker` keeps `Nodes::worker`, which refuses every stage whose leaf reads a source.
//!
//! **Limits.** As on a serving node, the pair, the authority and - for a `reading-worker` - the
//! catalog and the sources are all read once, at boot: a catalog that is re-pinned later is not
//! re-opened here (`crate::serve::refresh` is wired only into the serving root). The worker admits
//! only a call that carries its question's task grant, under the key the first call of that
//! question installed: a node of the same authority installs that key. The planning node routes a
//! split leg's partition only to a node `server.nodes` declares as reading its source, but no served
//! question builds a split leg yet, so outside tests no reading task reaches the reader. A source
//! of any kind but `PostgreSQL` - `BigQuery` among them - is refused, and a build without the
//! `postgres` adapter serves a `reading-worker` as a `worker`, reading nothing.

use std::sync::Arc;

use sutura_config::{RuntimeSettings, ServerRole, Settings, TlsMaterial};
use sutura_domain::pinned::PinnedDefinitions;
use sutura_exec_datafusion::{Nodes, PartitionReader};
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
    telemetry::install(settings.telemetry()).map_err(super::flatten)?;
    sutura_runtime::install_panic_hook();
    banner::announce(settings);
    let reading = settings.server().role() == ServerRole::ReadingWorker;
    let mount = if reading {
        reading_mount(tls, authority, settings)?
    } else {
        mount(tls, authority, settings.runtime())?
    };
    let router = sutura_http::worker_router(settings, &mount).map_err(super::flatten)?;
    if reading {
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
    mounted(tls, authority, runtime, None)
}

/// [`mount`], reading an admitted partition through `reader` when given one; with none, the worker
/// refuses every stage whose leaf reads a source.
fn mounted(
    tls: &TlsMaterial,
    authority: &std::path::Path,
    runtime: RuntimeSettings,
    reader: Option<Arc<dyn PartitionReader>>,
) -> Result<sutura_http::WorkerMount, String> {
    let identity = super::nodes::identity(tls, authority)?;
    let working_set = crate::sources::working_set(runtime);
    let own = match reader {
        Some(reader) => Nodes::worker_reading(working_set, identity, Some(reader)),
        None => Nodes::worker(working_set, identity),
    }
    .map_err(|cause| format!("{}\ncould not build this node's worker", super::flatten(cause)))?;
    Ok(sutura_http::WorkerMount::new(
        own.with_call_authorizer(sutura_exec_datafusion::TaskGrant::default())
            .into_worker_server()
            .max_decoding_message_size(working_set.bytes()),
    ))
}

/// What a `reading-worker` pinned and opened at boot.
type Opened = (PinnedDefinitions, super::OpenedSources);

/// Opens a `reading-worker`'s own declared catalog and its declared sources, through the exact
/// constructors `crate::serve::run`'s own boot calls for a serving node - `crate::catalog::open_catalog`,
/// `crate::catalog::load`, then `super::open_engine`.
///
/// **What this proves, and what it does not.** A failure here means the catalog or a source could
/// not be opened - the same refusal a serving node gives for the same misconfiguration - and stops
/// this process before it serves. The pinned catalog and the opened sources are kept for this node's
/// partition reader, so a source that becomes unreachable after boot is noticed only by a read.
fn open_reading_sources(settings: &Settings) -> Result<Opened, String> {
    let outbound = super::outbound::resolve(settings)?;
    let catalogs = crate::catalog::open_catalog(settings.catalogs(), outbound.as_ref())?;
    let pinned = crate::catalog::load(&catalogs)?;
    let opened = super::open_engine(
        &pinned,
        settings.sources(),
        settings.runtime(),
        settings.server().request_timeout(),
        outbound.as_ref(),
    )?;
    Ok((pinned, opened))
}

/// A `reading-worker`'s [`mount`], reading an admitted partition through a reader over what
/// [`open_reading_sources`] opened, each read bounded by this node's `server.request_timeout`.
#[cfg(feature = "postgres")]
fn reading_mount(
    tls: &TlsMaterial,
    authority: &std::path::Path,
    settings: &Settings,
) -> Result<sutura_http::WorkerMount, String> {
    let (pinned, opened) = open_reading_sources(settings)?;
    let reader = partition::Reader::new(pinned, opened, settings.server().request_timeout().budget());
    mounted(tls, authority, settings.runtime(), Some(Arc::new(reader)))
}

/// A `reading-worker`'s [`mount`] in a build without `PostgreSQL`, the one kind it reads: its
/// catalog and sources are opened, then dropped, and it serves as a `worker`.
#[cfg(not(feature = "postgres"))]
fn reading_mount(
    tls: &TlsMaterial,
    authority: &std::path::Path,
    settings: &Settings,
) -> Result<sutura_http::WorkerMount, String> {
    let _opened = open_reading_sources(settings)?;
    mount(tls, authority, settings.runtime())
}

#[cfg(feature = "postgres")]
mod partition;
#[cfg(test)]
mod reading;
#[cfg(test)]
mod tests;
