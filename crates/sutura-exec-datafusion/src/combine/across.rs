//! A combine whose stages above a repartition run on several nodes, on `datafusion-distributed`:
//! `docs/adr/20261030182759-shuffle-across-nodes.md`.
//!
//! **The legs stay on this node, by the engine's own routing.** A leg is a `MemTable`, a leaf the
//! engine plans as one task, and a stage of one task runs on the node that plans the question, in
//! process and unencoded. Every node above a leaf asks for one task per declared node, so a stage
//! above a hash repartition - the join's or the aggregate's - is spread across them, and what
//! crosses a link is that stage's input, as Arrow batches.
//!
//! **Limits, next to the claim.**
//!
//! - That the legs stay is the engine routing a one-task stage, not a check of ours. What a check
//!   holds is the other side: a [`Nodes::worker`] refuses a stage with a leaf that reads anything
//!   but another node's stage or rows the plan itself carries, such as a file scan
//!   (`StageReadsASource`, Decision 3). The leaves it accepts are four types, so a leg's rows sent
//!   to another node would run there; they are rows this node read and chose to send.
//! - A link is gRPC over mutual TLS: each end presents a certificate the deployment's node authority
//!   signed and refuses a peer without one ([`NodeIdentity`], Decision 4). It carries no grant, so a
//!   node serving [`Nodes::worker`] runs any stage that reads no source another node of the
//!   authority sends it; Decision 5 is not built. The identity is read once, so a renewed
//!   certificate is presented after a restart.
//! - A channel, once dialled, is kept for the process's life, one for every node it has dialled.
//! - A stage on a worker, this node's own included, reserves against that worker's pool, which is
//!   sized once for the node and not per question. Only the stage that collects the answer reserves
//!   against the question's own ceiling.
//! - No binary builds a [`Nodes`] yet.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;

use datafusion::common::tree_node::{TreeNode as _, TreeNodeRecursion};
use datafusion::datasource::memory::{DataSourceExec, MemorySourceConfig};
use datafusion::error::DataFusionError;
use datafusion::execution::SessionStateBuilder;
use datafusion::physical_plan::ExecutionPlan;
use datafusion::physical_plan::empty::EmptyExec;
use datafusion::physical_plan::placeholder_row::PlaceholderRowExec;
use datafusion::prelude::{SessionConfig, SessionContext};
use datafusion_distributed::grpc::{BoxCloneSyncChannel, create_worker_client};
use datafusion_distributed::{
    ChannelResolver, DesiredTaskCountEvent, DesiredTaskCountEventResponse, DistributedExt as _, NetworkBoundaryExt as _,
    SessionStateBuilderExt as _, Worker, WorkerChannel, WorkerPlanRewriteEvent, WorkerPlanRewriteEventResponse,
    WorkerQueryContext, WorkerResolver,
};
use hyper_util::rt::TokioIo;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use url::{Host, Url};

use super::CombineError;
use crate::pool::{self, WorkingSet};

/// The nodes one combine's stages may run on, this one among them.
pub struct Nodes {
    /// This node's worker. The stages planned onto this node run in it, and the node serves the
    /// same worker at `own_url`, which is where another node reads their output.
    own: Worker,
    own_url: Url,
    /// Every node a stage may be planned onto, `own_url` first.
    all: Declared,
    /// One set of channels for every combine, instead of the engine's default, which keys its
    /// channels on the runtime environment and so would dial again for each question.
    channels: Links,
}

impl core::fmt::Debug for Nodes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Nodes").field("all", &self.all.0).finish_non_exhaustive()
    }
}

/// The declared list, as the engine reads it.
#[derive(Clone)]
struct Declared(std::sync::Arc<[Url]>);

impl WorkerResolver for Declared {
    fn get_urls(&self) -> Result<Vec<Url>, DataFusionError> {
        Ok(self.0.to_vec())
    }
}

impl Nodes {
    /// This node's worker, served at `own_url`, and the other nodes a stage may be planned onto,
    /// each reached as `identity`.
    #[must_use]
    pub fn declared(own: Worker, own_url: Url, others: impl IntoIterator<Item = Url>, identity: NodeIdentity) -> Self {
        let all = core::iter::once(own_url.clone()).chain(others).collect();
        Self {
            own,
            own_url,
            all: Declared(all),
            channels: Links::of(identity),
        }
    }

    /// The worker a node serves: its stages reserve against a pool of `working_set` and spill
    /// nowhere, as a combine's own session does, read another node's stage as `identity`, and read
    /// no source (`StageReadsASource`).
    ///
    /// # Errors
    ///
    /// [`CombineError::Environment`] if the bounded environment cannot be built.
    pub fn worker(working_set: WorkingSet, identity: NodeIdentity) -> Result<Worker, CombineError> {
        let environment = pool::environment(working_set).map_err(|cause| CombineError::Environment { cause })?;
        let links = Links::of(identity);
        let worker = Worker::from_session_builder(move |context: WorkerQueryContext| {
            let links = links.clone();
            async move {
                Ok(context
                    .builder
                    .with_distributed_channel_resolver(links)
                    .with_distributed_worker_plan_rewrite_handler(reads_no_source)
                    .build())
            }
        });
        Ok(worker.with_runtime_env(environment.into_runtime()))
    }

    /// A session bounded to `working_set` whose stages above a repartition are planned across these
    /// nodes.
    pub(super) fn session(&self, working_set: WorkingSet) -> Result<SessionContext, CombineError> {
        let environment = pool::environment(working_set).map_err(|cause| CombineError::Environment { cause })?;
        let count = self.all.0.len();
        let state = SessionStateBuilder::new()
            .with_config(SessionConfig::new())
            .with_runtime_env(environment.into_runtime())
            .with_default_features()
            .with_distributed_worker_resolver(self.all.clone())
            .with_distributed_channel_resolver(self.channels.clone())
            .with_distributed_local_worker_context(self.own.to_local_worker_context(self.own_url.clone()))
            .with_distributed_desired_task_count_handler(move |event: DesiredTaskCountEvent<'_>| {
                // A leaf answered `None` is planned as one task, which routes to this node.
                (!event.plan.children().is_empty()).then(|| Ok(DesiredTaskCountEventResponse::desired(count)))
            })
            .with_distributed_planner()
            .build();
        Ok(SessionContext::new_with_state(state))
    }
}

/// A stage sent to a node has a leaf that reads a source: a node reads one only for a question it
/// plans itself, as the caller it serves.
#[derive(Debug, thiserror::Error)]
#[error("a stage sent to this node reads `{leaf}`, and a node reads no source for another node")]
pub(super) struct StageReadsASource {
    pub(super) leaf: String,
}

/// `event`'s stage, refused if a leaf reads anything but another node's stage or rows the plan
/// carries.
fn reads_no_source(event: WorkerPlanRewriteEvent<'_>) -> Result<WorkerPlanRewriteEventResponse, DataFusionError> {
    let mut source = None;
    event.plan.apply(|node| {
        if node.children().is_empty() && !carried(node) {
            source = Some(String::from(node.name()));
            return Ok(TreeNodeRecursion::Stop);
        }
        Ok(TreeNodeRecursion::Continue)
    })?;
    match source {
        Some(leaf) => Err(DataFusionError::External(Box::new(StageReadsASource { leaf }))),
        None => Ok(WorkerPlanRewriteEventResponse::new(event.plan)),
    }
}

/// A leaf that reads no source: another node's stage, or rows the plan itself carries.
fn carried(leaf: &Arc<dyn ExecutionPlan>) -> bool {
    leaf.as_network_boundary().is_some()
        || leaf.is::<EmptyExec>()
        || leaf.is::<PlaceholderRowExec>()
        || leaf
            .downcast_ref::<DataSourceExec>()
            .is_some_and(|scan| scan.data_source().is::<MemorySourceConfig>())
}

/// What a node presents on a link and which nodes it accepts on the other end: its certificate and
/// key, and the deployment's node authority.
#[derive(Clone)]
pub struct NodeIdentity(TlsConnector);

impl core::fmt::Debug for NodeIdentity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NodeIdentity").finish_non_exhaustive()
    }
}

/// The node's certificate, key or authority is not one a link can present or trust.
#[derive(Debug, thiserror::Error)]
#[error("the node's certificate, key or authority is not usable on a link")]
pub struct NotAnIdentity {
    #[source]
    cause: tokio_rustls::rustls::Error,
}

impl NodeIdentity {
    /// `chain` and `key` presented to every node dialled, and a node accepted only with a
    /// certificate `authority` signed.
    ///
    /// # Errors
    ///
    /// [`NotAnIdentity`] if an authority certificate is not a trust anchor, or the key does not
    /// belong to the chain.
    pub fn new(
        chain: Vec<CertificateDer<'static>>,
        key: PrivateKeyDer<'static>,
        authority: impl IntoIterator<Item = CertificateDer<'static>>,
    ) -> Result<Self, NotAnIdentity> {
        let mut roots = RootCertStore::empty();
        for anchor in authority {
            roots.add(anchor).map_err(|cause| NotAnIdentity { cause })?;
        }
        let mut config = ClientConfig::builder_with_provider(Arc::new(tokio_rustls::rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|cause| NotAnIdentity { cause })?
            .with_root_certificates(roots)
            .with_client_auth_cert(chain, key)
            .map_err(|cause| NotAnIdentity { cause })?;
        config.alpn_protocols = vec![b"h2".to_vec()];
        Ok(Self(TlsConnector::from(Arc::new(config))))
    }
}

/// The channels a node reaches other nodes on, each dialled once as `identity` and kept.
///
/// A channel's connection runs on the runtime that first used it and works only while that runtime
/// does, so every runtime a worker or a combine runs on has to run on threads of its own for the
/// process's life - [`DataFusionCombiner::across`](super::DataFusionCombiner::across)'s does.
#[derive(Clone)]
struct Links {
    identity: NodeIdentity,
    channels: Arc<tokio::sync::Mutex<Kept>>,
}

type Kept = HashMap<Url, BoxCloneSyncChannel>;

impl Links {
    fn of(identity: NodeIdentity) -> Self {
        Self {
            identity,
            channels: Arc::default(),
        }
    }

    /// A channel to `url` that dials it over TLS, whatever its scheme, the first time it is used.
    fn dial(&self, url: &Url) -> Result<BoxCloneSyncChannel, DataFusionError> {
        let host = match url.host() {
            Some(Host::Domain(name)) => String::from(name),
            Some(Host::Ipv4(address)) => address.to_string(),
            Some(Host::Ipv6(address)) => address.to_string(),
            None => return Err(DataFusionError::Configuration(format!("the node {url} has no host"))),
        };
        let port = url
            .port_or_known_default()
            .ok_or_else(|| DataFusionError::Configuration(format!("the node {url} has no port")))?;
        let name = ServerName::try_from(host.clone()).map_err(|cause| DataFusionError::External(Box::new(cause)))?;
        let connector = self.identity.0.clone();
        let dial = tower::service_fn(move |_uri| {
            let (connector, name, host) = (connector.clone(), name.clone(), host.clone());
            async move {
                let tcp = tokio::net::TcpStream::connect((host.as_str(), port)).await?;
                let tls = connector.connect(name, tcp).await?;
                Ok::<_, std::io::Error>(TokioIo::new(tls))
            }
        });
        let endpoint = tonic::transport::Endpoint::from_shared(url.to_string())
            .map_err(|cause| DataFusionError::External(Box::new(cause)))?;
        Ok(BoxCloneSyncChannel::new(endpoint.connect_with_connector_lazy(dial)))
    }
}

#[async_trait::async_trait]
impl ChannelResolver for Links {
    async fn get_worker_client_for_url(&self, url: &Url) -> Result<Box<dyn WorkerChannel>, DataFusionError> {
        let channel = match self.channels.lock().await.entry(url.clone()) {
            Entry::Occupied(kept) => kept.get().clone(),
            Entry::Vacant(unseen) => unseen.insert(self.dial(url)?).clone(),
        };
        Ok(create_worker_client(channel))
    }
}
