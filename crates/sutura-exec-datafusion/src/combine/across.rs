//! A combine whose stages above a repartition run on several nodes, on `datafusion-distributed`:
//! `docs/adr/20261030182759-shuffle-across-nodes.md`.
//!
//! **A leg's rows are dealt across the nodes.** A leg is a `MemTable`, and its scan, like every node
//! above it, asks for one task per declared node; `dealt` splits the scan batch by batch, one share
//! per task. Each node scans and repartitions a share of the rows, where a leg planned as one task
//! would run on this node alone, and this node's own worker would then refuse a question one node
//! refuses for its working set. A stage above a hash repartition - the join's or the aggregate's -
//! is spread across the nodes too. What crosses a link is a leg's share, inside the stage sent to a
//! node, and each stage's input, as Arrow batches.
//!
//! **A question goes across only when one node is too small for it.** A combine reading one data
//! system runs on this node, and so does one reading several whose legs hold no more than
//! `server.nodes.across_above_bytes`, the question's own ceiling when unset, unless this node refuses
//! it for its working set. A link costs latency a question one node answers does not need to pay.
//!
//! **A node may be declared by a host, which is resolved when each question is planned.** Each
//! address it resolves to is one node, its IPv4 ones when it has any so a dual-stack pod is not two,
//! dialled at that address and checked against the host: a pool of workers behind one name, as a
//! headless service's is, is the nodes it holds at that moment
//! (`docs/adr/20261030193029-operator-sizes-the-worker-pool.md`, Decision 6).
//!
//! **A question that goes across may wait for other nodes, bounded.** With
//! `server.nodes.wait_for_workers_seconds` declared, it wants one other node for each share of
//! `across_above_bytes` (the ceiling when unset) its legs hold beyond this node's, and one at least.
//! While fewer resolve, it resolves again every 200 ms until the wait ends, then plans across those
//! that did, and is refused, [`CombineError::WorkersNotReady`], only when none did (Decision 11).
//!
//! **Limits, next to the claim.**
//!
//! - A leg's size is Arrow's measure of its buffers, which counts a buffer two columns share once
//!   for each, so a leg decoded from one message can measure more than it holds. It errs towards
//!   going across.
//! - A question one node refuses is refused for its working set by any operator, the answer's own
//!   collection among them, and is asked across the nodes again whichever it was: one the
//!   collection refused is refused there too.
//! - A leg's share is rows this node read, as the caller it serves, and chose to send. What a check
//!   holds is the other side: a [`Nodes::worker`] refuses a stage with a leaf that reads anything
//!   but another node's stage or rows the plan itself carries, such as a file scan
//!   (`StageReadsASource`, Decision 3). The leaves it accepts are four types.
//! - A leg's scan with a limit or a declared order is not dealt: it is planned as one task, which
//!   runs on this node, and so does every node of its stage.
//! - This node holds every leg's rows until the question is answered: they are the caller's
//!   batches, outside any pool. Dealing them changes which node scans them, not which holds them.
//! - A link is gRPC over mutual TLS: each end presents a certificate the deployment's node authority
//!   signed and refuses a peer without one ([`NodeIdentity`], Decision 4). It carries no grant, so a
//!   node serving [`Nodes::worker`] runs any stage that reads no source another node of the
//!   authority sends it; Decision 5 is not built. The identity is read once, so a renewed
//!   certificate is presented after a restart.
//! - A channel, once dialled, is kept for the process's life, one for every address it has dialled,
//!   so a host whose addresses change keeps one for each it was ever dialled at.
//! - A host that resolves to nothing declares no node, as an empty pool does, so a misspelled one
//!   plans every question onto the other nodes rather than refusing it - unless a wait is declared
//!   and no other node resolves, which is refused once the wait ends.
//! - Only an empty pool is refused, which keeps the refusal's "retry" truthful: a pool already at its
//!   largest would otherwise turn a question too large for it into a retry that never succeeds. A
//!   question planned across fewer nodes than it wanted may still be refused for its working set.
//! - A waiting question holds its serving node's slot, and a thread, for as long as it waits.
//! - A resolved address is counted as a worker. This node cannot tell a worker from another serving
//!   node behind the same name, so a host naming both counts both.
//! - A stage on a worker, this node's own included, reserves against that worker's pool, which is
//!   sized once for the node and not per question. Only the stage that collects the answer reserves
//!   against the question's own ceiling.
//! - A link's buffer is bounded by its budget plus one message, which it admits whatever its size,
//!   and two questions on one node each hold their own links' share. A worker holds the budget the
//!   stage it runs carries, which the node that planned it set.
//! - `sutura serve` builds a [`Nodes`] from `server.nodes`, under its `distributed` feature only.

use core::num::NonZeroU64;
use core::time::Duration;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs as _};
use std::sync::Arc;

use datafusion::common::tree_node::{TreeNode as _, TreeNodeRecursion};
use datafusion::datasource::memory::{DataSourceExec, MemorySourceConfig};
use datafusion::datasource::source::DataSource as _;
use datafusion::error::DataFusionError;
use datafusion::execution::SessionStateBuilder;
use datafusion::physical_plan::ExecutionPlan;
use datafusion::physical_plan::empty::EmptyExec;
use datafusion::physical_plan::placeholder_row::PlaceholderRowExec;
use datafusion::prelude::{SessionConfig, SessionContext};
use datafusion_distributed::grpc::{BoxCloneSyncChannel, create_worker_client};
use datafusion_distributed::{
    ChannelResolver, DesiredTaskCountEvent, DesiredTaskCountEventResponse, DistributedConfig, DistributedExt as _,
    DistributedLeafExec, NetworkBoundaryExt as _, ScaleUpLeafNodeEvent, ScaleUpLeafNodeEventResponse,
    SessionStateBuilderExt as _, Worker, WorkerChannel, WorkerPlanRewriteEvent, WorkerPlanRewriteEventResponse,
    WorkerQueryContext, WorkerResolver,
};
use hyper_util::rt::TokioIo;
use sutura_domain::plan::{SlotsHeld, WorkerSlotsWanted};
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use url::{Host, Url};

use super::CombineError;
use crate::pool::{self, WorkingSet};

/// How many other nodes a question wants, and the bounded wait for them.
mod wait;

/// The nodes one combine's stages may run on, this one among them.
pub struct Nodes {
    /// This node's worker. The stages planned onto this node run in it, and the node serves the
    /// same worker at `own_url`, which is where another node reads their output.
    own: Worker,
    own_url: Url,
    /// The other nodes a stage may be planned onto, as declared.
    others: Vec<Url>,
    /// One set of channels for every combine, instead of the engine's default, which keys its
    /// channels on the runtime environment and so would dial again for each question.
    channels: Links,
    /// The leg bytes above which a question is planned across before it is tried on this node.
    across_above: Option<NonZeroU64>,
    /// How long a question that goes across waits for the other nodes it wants to resolve.
    wait_for_workers: Option<Duration>,
    /// The other nodes the questions going across now want, summed.
    slots_wanted: WorkerSlotsWanted,
}

impl core::fmt::Debug for Nodes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Nodes").field("others", &self.others).finish_non_exhaustive()
    }
}

/// One question's nodes, `own_url` first, as the engine reads them.
#[derive(Clone)]
struct Declared(Arc<[Url]>);

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
        Self {
            own,
            own_url,
            others: others.into_iter().collect(),
            channels: Links::of(identity),
            across_above: None,
            wait_for_workers: None,
            slots_wanted: WorkerSlotsWanted::default(),
        }
    }

    /// The same nodes, planning a question whose legs hold more than `bytes` across them at once,
    /// or more than its own ceiling when that is `None`.
    #[must_use]
    pub fn across_above(self, bytes: Option<NonZeroU64>) -> Self {
        Self {
            across_above: bytes,
            ..self
        }
    }

    /// The same nodes, a question that goes across waiting up to `wait` for the other nodes it wants,
    /// or planned across those resolved at once when that is `None`.
    #[must_use]
    pub fn wait_for_workers(self, wait: Option<Duration>) -> Self {
        Self {
            wait_for_workers: wait,
            ..self
        }
    }

    /// Whether a question whose legs hold `legs_bytes`, under a ceiling of `ceiling_bytes`, is
    /// planned across these nodes before it is tried on this one.
    pub(super) fn first_across(&self, legs_bytes: u64, ceiling_bytes: u64) -> bool {
        legs_bytes > self.across_above.map_or(ceiling_bytes, NonZeroU64::get)
    }

    /// The other nodes the questions going across now want, summed.
    pub(super) const fn slots_wanted(&self) -> &WorkerSlotsWanted {
        &self.slots_wanted
    }

    /// The other nodes a question whose legs hold `legs_bytes` under `ceiling_bytes` wants, counted
    /// in [`Self::slots_wanted`] until the hold is dropped.
    pub(super) fn hold(&self, legs_bytes: u64, ceiling_bytes: u64) -> SlotsHeld<'_> {
        self.slots_wanted
            .hold(wait::others_wanted(self.across_above, legs_bytes, ceiling_bytes))
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

    /// [`Self::session`], over the other nodes resolved once as many as a question whose legs hold
    /// `legs_bytes` under `ceiling_bytes` wants have, or the declared wait ends.
    ///
    /// # Errors
    ///
    /// [`CombineError::WorkersNotReady`] if no other node had resolved when the wait ended.
    pub(super) fn session_for(
        &self,
        working_set: WorkingSet,
        legs_bytes: u64,
        ceiling_bytes: u64,
    ) -> Result<SessionContext, CombineError> {
        self.wait_for_workers.map_or_else(
            || self.session(working_set),
            |wait| {
                let wanted = wait::others_wanted(self.across_above, legs_bytes, ceiling_bytes);
                let ready = wait::ready(wanted, wait, || self.others.iter().flat_map(resolved).collect())?;
                self.session_over(working_set, ready)
            },
        )
    }

    /// A session over the other nodes resolved now. See [`Self::session_over`].
    pub(super) fn session(&self, working_set: WorkingSet) -> Result<SessionContext, CombineError> {
        self.session_over(working_set, self.others.iter().flat_map(resolved))
    }

    /// A session bounded to `working_set` whose stages above a repartition are planned across this
    /// node and `others`, each stage one task per node and each task one partition per node.
    ///
    /// A link charges the batches it holds to its node's pool without asking, so each link may hold
    /// a quarter of the working set shared by the nodes: a task reads one link from every node for
    /// each input, and a join's two inputs then hold half the working set at most, leaving the other
    /// half to its operators. The budget rides with each stage, so the node running it reads it too.
    fn session_over(
        &self,
        working_set: WorkingSet,
        others: impl IntoIterator<Item = Url>,
    ) -> Result<SessionContext, CombineError> {
        let environment = pool::environment(working_set).map_err(|cause| CombineError::Environment { cause })?;
        let all = Declared(core::iter::once(self.own_url.clone()).chain(others).collect());
        let count = all.0.len();
        let mut links = DistributedConfig::default();
        links.worker_connection_buffer_budget_bytes =
            working_set.bytes().checked_div(count.saturating_mul(4)).unwrap_or_default();
        let state = SessionStateBuilder::new()
            .with_config(SessionConfig::new().with_target_partitions(count))
            .with_distributed_option_extension(links)
            .with_runtime_env(environment.into_runtime())
            .with_default_features()
            .with_distributed_worker_resolver(all)
            .with_distributed_channel_resolver(self.channels.clone())
            .with_distributed_local_worker_context(self.own.to_local_worker_context(self.own_url.clone()))
            .with_distributed_desired_task_count_handler(move |event: DesiredTaskCountEvent<'_>| {
                // Any other leaf, answered `None`, is planned as one task, which routes to this node.
                (!event.plan.children().is_empty() || dealable(event.plan).is_some())
                    .then(|| Ok(DesiredTaskCountEventResponse::desired(count)))
            })
            .with_distributed_scale_up_leaf_node_handler(dealt)
            .with_distributed_planner()
            .build();
        Ok(SessionContext::new_with_state(state))
    }
}

/// The nodes `declared` names now: itself when its host is an address, and otherwise one for each
/// address its host resolves to, of one family, with that address in its fragment.
fn resolved(declared: &Url) -> Vec<Url> {
    let (Some(Host::Domain(host)), Some(port)) = (declared.host(), declared.port_or_known_default()) else {
        return vec![declared.clone()];
    };
    let addresses: Vec<SocketAddr> = (host, port).to_socket_addrs().map_or_default(Iterator::collect);
    let ipv4 = addresses.iter().any(SocketAddr::is_ipv4);
    addresses
        .into_iter()
        .filter(|address| address.is_ipv4() == ipv4)
        .map(|address| {
            let mut node = declared.clone();
            node.set_fragment(Some(&address.ip().to_string()));
            node
        })
        .collect()
}

/// `event`'s leg scan as one variant per task, each with every `task_count`th batch of each
/// partition, so the tasks read the leg's rows between them and none twice.
fn dealt(event: ScaleUpLeafNodeEvent<'_>) -> Option<Result<ScaleUpLeafNodeEventResponse, DataFusionError>> {
    let rows = dealable(event.plan)?;
    let tasks = event.task_count;
    (tasks > 1).then(|| {
        let variants = (0..tasks)
            .map(|task| {
                let partitions: Vec<Vec<_>> = rows
                    .partitions()
                    .iter()
                    .map(|batches| batches.iter().skip(task).step_by(tasks).cloned().collect())
                    .collect();
                MemorySourceConfig::try_new_exec(&partitions, rows.original_schema(), rows.projection().clone())
                    .map(|scan| scan as Arc<dyn ExecutionPlan>)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let leaf = DistributedLeafExec::try_new(Arc::clone(event.plan), variants)?;
        Ok(ScaleUpLeafNodeEventResponse::new(Arc::new(leaf)))
    })
}

/// A leg's scan whose rows may be split between tasks: one with no limit and no declared order.
fn dealable(leaf: &Arc<dyn ExecutionPlan>) -> Option<&MemorySourceConfig> {
    let rows = leaf
        .downcast_ref::<DataSourceExec>()?
        .data_source()
        .downcast_ref::<MemorySourceConfig>()?;
    (rows.fetch().is_none() && rows.sort_information().is_empty()).then_some(rows)
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

    /// A channel to `url` that dials it over TLS, whatever its scheme, the first time it is used, at
    /// the address its fragment names when it names one, and checks the peer against its host.
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
        let address = match url.fragment() {
            Some(address) => address
                .parse::<IpAddr>()
                .map_err(|cause| DataFusionError::External(Box::new(cause)))?
                .to_string(),
            None => host,
        };
        let connector = self.identity.0.clone();
        let dial = tower::service_fn(move |_uri| {
            let (connector, name, address) = (connector.clone(), name.clone(), address.clone());
            async move {
                let tcp = tokio::net::TcpStream::connect((address.as_str(), port)).await?;
                let tls = connector.connect(name, tcp).await?;
                Ok::<_, std::io::Error>(TokioIo::new(tls))
            }
        });
        let mut origin = url.clone();
        origin.set_fragment(None);
        let endpoint = tonic::transport::Endpoint::from_shared(origin.to_string())
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
