//! A `server.role: worker` node: no catalog, no source, and the worker transport as its one surface.
//!
//! `Settings::load` has refused a worker that declares a source or a catalog, names no
//! `server.tls_node_authority`, or enables `/mcp` or Flight, so what is left is this node's worker,
//! built from the listener's pair and the node authority, served behind the node-certificate gate.
//! `server.nodes` is not read.
//!
//! **Limits.** As on a serving node, the pair and the authority are read once, at boot, and a link
//! carries no task grant: this node runs any stage that reads no source which a node of the same
//! authority sends it.

use sutura_config::{RuntimeSettings, Settings, TlsMaterial};
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
        return Err(String::from("server.role is worker and server.tls_node_authority is not set"));
    };
    let mount = mount(tls, authority, settings.runtime())?;
    telemetry::install(settings.telemetry()).map_err(super::flatten)?;
    sutura_runtime::install_panic_hook();
    banner::announce(settings);
    let router = sutura_http::worker_router(settings, &mount).map_err(super::flatten)?;
    tracing::info!("serving as a worker: no catalog, no source, no /v1, /mcp or Flight");
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
/// as the serving arm builds its own.
pub(super) fn mount(
    tls: &TlsMaterial,
    authority: &std::path::Path,
    runtime: RuntimeSettings,
) -> Result<sutura_http::WorkerMount, String> {
    let identity = super::nodes::identity(tls, authority)?;
    let own = sutura_exec_datafusion::Nodes::worker(crate::sources::working_set(runtime), identity)
        .map_err(|cause| format!("{}\ncould not build this node's worker", super::flatten(cause)))?;
    Ok(sutura_http::WorkerMount::new(own.into_worker_server()))
}
