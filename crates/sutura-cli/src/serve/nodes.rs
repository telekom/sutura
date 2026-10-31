//! The combiner every arm of `run` serves through: across the nodes `server.nodes` declares, or on
//! this node alone.
//!
//! Declared nodes build this node's worker once. The combine runs its own stages in it, and the
//! listener serves the same worker to the other nodes behind the node-certificate gate. A link
//! presents `server.tls_certificate` and `server.tls_key`, the pair the listener presents, and
//! accepts only a node `server.tls_node_authority` signed.
//!
//! **Limits.** The pair and the authority are read once, at boot: the listener's rotation does not
//! reach a link, so a renewed node certificate is presented on links after a restart. A link carries
//! no task grant, so this node runs any stage that reads no source which a node of the same
//! authority sends it.

use sutura_config::RuntimeSettings;
use sutura_config::TlsMaterial;
use sutura_exec_datafusion::DataFusionCombiner;
use sutura_http::ServiceState;

/// What `run` mounts beside the other transports: this node's worker, when nodes are declared.
#[cfg(feature = "distributed")]
pub(super) type Served = Option<sutura_http::WorkerMount>;
#[cfg(not(feature = "distributed"))]
pub(super) type Served = ();

/// The combiner `run` serves through, and what it mounts beside the other transports.
pub(super) type Chosen = (DataFusionCombiner, Served);

/// The combiner, and the worker the listener serves when it plans across declared nodes.
///
/// # Errors
///
/// A pair or an authority that cannot be read or does not make a node's identity. Declared nodes
/// never fall back to a combine on one node.
#[cfg(feature = "distributed")]
pub(super) fn combiner(tls: Option<&TlsMaterial>, runtime: RuntimeSettings) -> Result<Chosen, String> {
    use sutura_exec_datafusion::Nodes;

    let Some((tls, nodes)) = tls.and_then(|tls| tls.nodes().map(|nodes| (tls, nodes))) else {
        return Ok((crate::catalog::one_node_combiner()?, None));
    };
    let identity = identity(tls, nodes)?;
    let own = Nodes::worker(crate::sources::working_set(runtime), identity.clone())
        .map_err(|cause| format!("{}\ncould not build this node's worker", super::flatten(cause)))?;
    let mount = sutura_http::WorkerMount::new(own.clone().into_worker_server());
    let across = Nodes::declared(own, nodes.own().clone(), nodes.others().iter().cloned(), identity);
    let combiner = DataFusionCombiner::across(across)
        .map_err(|cause| format!("{}\ncould not build the federation combiner", super::flatten(cause)))?;
    Ok((combiner, Some(mount)))
}

/// The same, on a build that cannot plan across nodes: declared ones are refused.
#[cfg(not(feature = "distributed"))]
pub(super) fn combiner(tls: Option<&TlsMaterial>, _runtime: RuntimeSettings) -> Result<Chosen, String> {
    super::not_linked::nodes_refused_if_declared(tls)?;
    Ok((crate::catalog::one_node_combiner()?, ()))
}

#[cfg(feature = "distributed")]
fn identity(tls: &TlsMaterial, nodes: &sutura_config::DeclaredNodes) -> Result<sutura_exec_datafusion::NodeIdentity, String> {
    let pair = sutura_tls::Identity::new(tls.certificate().to_path_buf(), tls.key().to_path_buf());
    let (chain, key) = sutura_tls::load_identity(&pair)
        .map_err(|cause| {
            format!(
                "{}\nserver.tls_certificate and server.tls_key are not a node's identity",
                super::flatten(cause)
            )
        })?
        .into_parts();
    let authority = sutura_tls::load_anchors(&sutura_tls::Anchors::Bundle(nodes.authority().to_path_buf()))
        .map_err(|cause| format!("{}\nserver.tls_node_authority is not a node authority", super::flatten(cause)))?;
    sutura_exec_datafusion::NodeIdentity::new(chain, key, authority)
        .map_err(|cause| format!("{}\nserver.nodes cannot be reached", super::flatten(cause)))
}

/// `state` with this node's worker mounted, when there is one.
#[cfg(feature = "distributed")]
pub(super) fn mounted(state: ServiceState, worker: Served) -> ServiceState {
    match worker {
        Some(mount) => state.with_worker_surface(mount),
        None => state,
    }
}

#[cfg(not(feature = "distributed"))]
pub(super) const fn mounted(state: ServiceState, (): Served) -> ServiceState {
    state
}
