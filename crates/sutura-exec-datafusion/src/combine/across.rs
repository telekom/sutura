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
//!   signed and refuses a peer without one ([`NodeIdentity`], Decision 4). The identity is read once,
//!   so a renewed certificate is presented after a restart.
//! - Each call a link carries holds the task grant of its question ([`TaskGrant`], Decision 5): a
//!   worker that asks for it runs a stage of a question only for a node that holds that question's
//!   key, or was itself installed with one of its stages. Who may install a stage is the link's
//!   mutual TLS alone. A worker served without [`TaskGrant`] admits any call.
//! - A channel, once dialled, is kept for the process's life, one for every address it has dialled,
//!   so a host whose addresses change keeps one for each it was ever dialled at.
//! - A host that resolves to nothing declares no node, as an empty pool does, so a misspelled one
//!   plans every question onto the other nodes rather than refusing it - unless a wait is declared
//!   and no other node resolves, which is refused once the wait ends.
//! - Only an empty pool is refused, which keeps the refusal's "retry" truthful: a pool already at its
//!   largest would otherwise turn a question too large for it into a retry that never succeeds. A
//!   question planned across fewer nodes than it wanted may still be refused for its working set.
//! - A waiting question holds its serving node's slot, and a thread, for as long as it waits.
//! - A node a question was planned onto whose link the engine reports unavailable - a connection
//!   refused, reset or timed out - or whose connection ends under a call, closed, reset or no longer
//!   read, fails that question, which is refused as [`CombineError::NodeLost`] and may be asked again.
//!   Nothing re-plans it onto the nodes that remain. A link that fails with any other status, an
//!   HTTP/2 stream reset for any reason but refusing the stream among them, leaves as
//!   [`CombineError::Execute`], unless the failure carries the code of a stage a node refused for
//!   rows past the federated row ceiling, which leaves as [`CombineError::RowsOverCeiling`]. A cell
//!   loses a node nothing listens on and one that drops its connection under a call, over plain
//!   TCP; the multi-node VM check kills one while it answers.
//! - That code is read only exactly, only off a failure, and answered from this node's own ceiling:
//!   a peer can turn a failed question into that refusal, never into an answer. A code a node wraps
//!   in an Arrow or an external error crosses its next link as text and stays a failure. One stage
//!   raises it: a final-stage task past the worker's row ceiling.
//! - A resolved address is counted as a worker. This node cannot tell a worker from another serving
//!   node behind the same name, so a host naming both counts both.
//! - A stage on a worker, this node's own included, reserves against that worker's pool, which is
//!   sized once for the node and not per question. Only the stage that collects the answer reserves
//!   against the question's own ceiling.
//! - A link's buffer is bounded by its budget plus one message, of at most the working set of the
//!   node or question reading it or `LEAST_REPLY`, whichever is larger - the client's own gRPC
//!   decode is capped there, so a peer's length prefix cannot size this node's buffer past that.
//!   Two questions on one node each hold their own links' share, and a worker holds the budget the
//!   stage it runs carries, which the node that planned it set. What this does NOT bound: a message
//!   under the cap is still buffered whole before it is read, so a reply of `LEAST_REPLY` is held
//!   whatever the working set, and decoding a body under the cap into Arrow is not bounded here
//!   (#1680 1(c)).
//! - A worker fails a task of a question's final stage whose rows pass the whole federated row
//!   ceiling, [`Nodes::row_ceiling`], as [`WorkerRefusal`] under that code (Decision 4), which the
//!   planning node answers as [`CombineError::RowsOverCeiling`]. The check is per task, so tasks that
//!   each stay under the ceiling can sum past it: the planning node's exact count stays the one that
//!   refuses every answer over the ceiling.
//! - A [`SplitScan`]'s partitions are read only on the nodes [`Nodes::reading`] names for its
//!   source, each under a key of its own sent only to the node that reads it, and a stage no such
//!   node resolved for is refused. [`refuse_ambiguous_split`] checks a split lookup leg's link
//!   values across its partitions, counting each value after a repartition on the link columns. A
//!   combine registers a lookup leg its source granted as one, and checks and joins it across these
//!   nodes, never on this one: each partition is read twice, once by the check and once by the join,
//!   under the one snapshot its source holds open across both. Only a `sutura serve`
//!   deployment's PostgreSQL source grants a served leg (`github.com/telekom/sutura#828`).
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
use datafusion_distributed::grpc::{BoxCloneSyncChannel, create_worker_client_with_decode_limit};
use datafusion_distributed::{
    ChannelResolver, DesiredTaskCountEvent, DesiredTaskCountEventResponse, DistributedConfig, DistributedExt as _,
    DistributedLeafExec, NetworkBoundaryExt as _, RouteTasksEvent, ScaleUpLeafNodeEvent, ScaleUpLeafNodeEventResponse,
    SessionStateBuilderExt as _, Worker, WorkerChannel, WorkerPlanRewriteEvent, WorkerPlanRewriteEventResponse,
    WorkerQueryContext, WorkerResolver,
};
use hyper_util::rt::TokioIo;
use sutura_domain::model::SourceName;
use sutura_domain::plan::{FederatedRowCeiling, SlotsHeld, WorkerSlotsWanted};
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tower::ServiceExt as _;
use url::{Host, Url};

use super::CombineError;
use crate::pool::{self, WorkingSet};

/// How many other nodes a question wants, and the bounded wait for them.
mod wait;

/// The task grant a node asks of every call to its worker, keyed per question (Decision 5).
mod grant;

/// The federated row ceiling a worker counts a final-stage task's rows against (Decision 4).
mod ceiling;

/// The leaf a reading task's plan carries, its worker admit rule, and the composite codec a
/// reading node's worker and coordinator both install (Decision 5).
mod partition;

/// A split leg's partitions as tasks of a source-reading stage, each placed on a node reading its
/// source (Decisions 1 and 5).
mod split;

/// A split lookup leg's ambiguous-link check, counted after a repartition on its link columns.
mod link;

pub use ceiling::WorkerRefusal;
pub use grant::TaskGrant;
pub use link::refuse_ambiguous_split;
pub use partition::{PartitionReader, Unread, read_later};
pub use split::{SplitScan, StreamBound};

/// A refusal another node raised in a stage, read back off the failure its link relays.
mod relay;

pub(super) use relay::relayed;

#[cfg(test)]
mod lost;

/// A final-stage task over the ceiling, run on a worker. `#![cfg(test)]` sits in the file rather
/// than on this declaration, which the causality gate's base run then restores away with the rest of
/// this file: every cell there names what the base tree lacks.
#[path = "tests/across/ceiling.rs"]
mod final_stage;

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
    /// The questions this node planned, each bound to its key, which [`Self::served`] admits.
    keys: grant::Keys,
    /// The rows a task of a question's final stage may emit before its worker fails it.
    row_ceiling: FederatedRowCeiling,
    /// The other nodes declared as reading each source, as declared.
    readers: HashMap<SourceName, Vec<Url>>,
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
            keys: grant::Keys::default(),
            row_ceiling: FederatedRowCeiling::LOOSEST,
            readers: HashMap::new(),
        }
    }

    /// This node's worker, asking every call it is sent for the task grant, which admits the
    /// questions this node plans as well as those another node installs on it.
    #[must_use]
    pub fn served(&self) -> Worker {
        self.own.clone().with_call_authorizer(TaskGrant::of(self.keys.clone()))
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

    /// The same nodes, a worker failing a task of a question's final stage once its rows pass
    /// `ceiling`, the loosest a deployment may configure until this is called.
    #[must_use]
    pub fn row_ceiling(self, ceiling: FederatedRowCeiling) -> Self {
        Self {
            row_ceiling: ceiling,
            ..self
        }
    }

    /// The same nodes, a [`SplitScan`] of `source` read only on `readers`, each as declared. A reader
    /// that is not among the other nodes declared, or does not resolve, reads nothing.
    #[must_use]
    pub fn reading(mut self, source: SourceName, readers: impl IntoIterator<Item = Url>) -> Self {
        self.readers.insert(source, readers.into_iter().collect());
        self
    }

    /// Whether any other node is declared as reading `source`, resolved or not.
    pub(super) fn reads(&self, source: &SourceName) -> bool {
        self.readers.get(source).is_some_and(|readers| !readers.is_empty())
    }

    /// Whether a node declared as reading `source` is among the other nodes resolved now, as
    /// [`Self::session_over`] reads them.
    ///
    /// **Limit:** each call resolves every other node again, so a host that resolves here and not
    /// in the session planned beside it, or the other way round, is read differently by the two.
    pub(super) fn resolves_a_reader(&self, source: &SourceName) -> bool {
        self.readers.get(source).is_some_and(|declared| {
            self.others
                .iter()
                .flat_map(resolved)
                .any(|node| declared.contains(&unresolved(&node)))
        })
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
        let bounded = links.bounded(working_set);
        let worker = Worker::from_session_builder(move |context: WorkerQueryContext| {
            let bounded = bounded.clone();
            async move {
                Ok(grant::forwarded(context.builder, &context.headers)?
                    .with_distributed_channel_resolver(bounded)
                    .with_distributed_worker_plan_rewrite_handler(reads_no_source)
                    .with_distributed_user_codec(partition::Codec)
                    .build())
            }
        });
        Ok(worker.with_runtime_env(environment.into_runtime()))
    }

    /// The worker a reading node serves: as [`Self::worker`], but admitting a source-reading task's
    /// [`PartitionScanExec`](partition::PartitionScanExec) leaf under a verified reading grant
    /// (Decision 5) instead of refusing every leaf outright, reading an admitted one through `reader`
    /// when given one. `reader` is `None` for a combining-only worker:
    /// [`reads_partition_or_nothing`](partition::reads_partition_or_nothing) still refuses a leaf it
    /// cannot verify before `reader` is ever asked for, so a combining-only worker refuses a
    /// partition scan exactly as [`Self::worker`] refuses any other leaf it did not plan.
    ///
    /// # Errors
    ///
    /// [`CombineError::Environment`] if the bounded environment cannot be built.
    pub fn worker_reading(
        working_set: WorkingSet,
        identity: NodeIdentity,
        reader: Option<Arc<dyn partition::PartitionReader>>,
    ) -> Result<Worker, CombineError> {
        let environment = pool::environment(working_set).map_err(|cause| CombineError::Environment { cause })?;
        let links = Links::of(identity);
        let bounded = links.bounded(working_set);
        let keys = grant::ReadingKeys::default();
        let worker = Worker::from_session_builder(move |context: WorkerQueryContext| {
            let bounded = bounded.clone();
            let keys = keys.clone();
            let reader = reader.clone();
            async move {
                let mut builder = grant::forwarded(context.builder, &context.headers)?
                    .with_distributed_channel_resolver(bounded)
                    .with_distributed_worker_plan_rewrite_handler(
                        move |event: datafusion_distributed::WorkerPlanRewriteEvent<'_>| {
                            partition::reads_partition_or_nothing(event, &keys)
                        },
                    )
                    .with_distributed_user_codec(partition::Codec);
                if let Some(reader) = reader {
                    let slot = builder.config();
                    *slot = Some(
                        slot.take()
                            .unwrap_or_default()
                            .with_extension(Arc::new(partition::ReaderExtension(reader))),
                    );
                }
                Ok(builder.build())
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
        let reading: HashMap<SourceName, Vec<Url>> = self
            .readers
            .iter()
            .map(|(source, declared)| {
                let resolved = all.0.iter().skip(1).filter(|node| declared.contains(&unresolved(node)));
                (source.clone(), resolved.cloned().collect())
            })
            .collect();
        let reading = Arc::new(reading);
        let routing = Arc::clone(&reading);
        let count = all.0.len();
        let mut links = DistributedConfig::default();
        links.worker_connection_buffer_budget_bytes =
            working_set.bytes().checked_div(count.saturating_mul(4)).unwrap_or_default();
        let planned = SessionStateBuilder::new()
            .with_config(SessionConfig::new().with_target_partitions(count))
            .with_distributed_option_extension(links)
            .with_runtime_env(environment.into_runtime())
            .with_default_features()
            .with_distributed_worker_resolver(all)
            .with_distributed_channel_resolver(self.channels.bounded(working_set))
            .with_distributed_local_worker_context(self.own.to_local_worker_context(self.own_url.clone()))
            .with_distributed_desired_task_count_handler(move |event: DesiredTaskCountEvent<'_>| split::desired(event, &reading))
            .with_distributed_scale_up_leaf_node_handler(split::scaled)
            .with_distributed_route_tasks_handler(move |event: RouteTasksEvent<'_>| split::routed(&event, &routing))
            .with_distributed_desired_task_count_handler(move |event: DesiredTaskCountEvent<'_>| {
                // Any other leaf, answered `None`, is planned as one task, which routes to this node.
                (!event.plan.children().is_empty() || dealable(event.plan).is_some())
                    .then(|| Ok(DesiredTaskCountEventResponse::desired(count)))
            })
            .with_distributed_scale_up_leaf_node_handler(dealt)
            .with_distributed_planner();
        let unbuilt = |cause| CombineError::Environment {
            cause: crate::DataFusionError::Environment { cause },
        };
        let counted = ceiling::counted(planned, self.row_ceiling).map_err(unbuilt)?;
        let state = grant::keyed(counted, self.keys.clone()).map_err(unbuilt)?.build();
        Ok(SessionContext::new_with_state(state))
    }
}

/// `error` as [`CombineError::NodeLost`] when it failed because a node it ran on could not be reached.
///
/// The engine carries a failed call to another node as the call's own status, inside an external
/// error, so a status in `error`'s chain whose code says the link was unavailable is a lost node, and
/// so is one whose cause is the link's connection ending under the call.
pub(super) fn lost(error: CombineError) -> CombineError {
    match error {
        CombineError::Execute { cause } if unreachable(&cause) => CombineError::NodeLost { cause },
        other => other,
    }
}

fn unreachable(cause: &DataFusionError) -> bool {
    chain(cause)
        .filter_map(|error| error.downcast_ref::<tonic::Status>())
        .any(|status| status.code() == tonic::Code::Unavailable || chain(status).any(went_away))
}

/// Whether `error` is a connection that ended under a call: the node closed it, reset it or stopped
/// reading it. The engine's client reads this as an unknown status, from the cause it keeps.
fn went_away(error: &(dyn core::error::Error + 'static)) -> bool {
    error
        .downcast_ref::<h2::Error>()
        .and_then(h2::Error::get_io)
        .or_else(|| error.downcast_ref::<std::io::Error>())
        .is_some_and(|io| {
            matches!(
                io.kind(),
                std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::BrokenPipe
            )
        })
}

fn chain<'a>(error: &'a (dyn core::error::Error + 'static)) -> impl Iterator<Item = &'a (dyn core::error::Error + 'static)> {
    core::iter::successors(Some(error), |error| error.source())
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

/// `node` as declared: without the address [`resolved`] put in its fragment.
fn unresolved(node: &Url) -> Url {
    let mut declared = node.clone();
    declared.set_fragment(None);
    declared
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

/// A stage reading split legs of `aliases` that no resolved node is declared as reading together: it
/// is read on none, and never on a node that does not read them. `unread` is the first of them no
/// resolved node reads at all, `None` when each has a reader and none reads them all.
#[derive(Debug, thiserror::Error)]
#[error(
    "no resolved node reads all of {}, so the stage's split legs are read on none",
    .aliases.iter().map(|alias| format!("`{alias}`")).collect::<Vec<_>>().join(", ")
)]
pub(super) struct NoReadingNode {
    pub(super) aliases: Vec<SourceName>,
    pub(super) unread: Option<SourceName>,
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

    /// A node signed for 127.0.0.1 by an authority of its own: no caller of this dials a link.
    #[cfg(test)]
    pub(super) fn signed_for_localhost() -> Self {
        let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).expect("no names are valid parameters");
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let authority = rcgen::CertifiedIssuer::self_signed(params, rcgen::KeyPair::generate().expect("a key generates"))
            .expect("an authority signs itself");
        let key = rcgen::KeyPair::generate().expect("a key generates");
        let certificate = rcgen::CertificateParams::new(vec![String::from("127.0.0.1")])
            .expect("an address is valid parameters")
            .signed_by(&key, &authority)
            .expect("the authority signs a node");
        Self::new(
            vec![certificate.der().clone()],
            PrivateKeyDer::Pkcs8(tokio_rustls::rustls::pki_types::PrivatePkcs8KeyDer::from(key.serialize_der())),
            [authority.der().clone()],
        )
        .expect("a signed node is an identity")
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

    /// These links, resolving a client whose decode of a peer's reply is capped at `working_set`'s
    /// bytes, so a peer's own length prefix cannot size this node's buffer past what it would hold.
    fn bounded(&self, working_set: WorkingSet) -> Bounded {
        Bounded {
            links: self.clone(),
            working_set,
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
        Ok(BoxCloneSyncChannel::new(
            endpoint.connect_with_connector_lazy(dial).map_request(grant::keyless),
        ))
    }
}

/// The cap under a working set smaller than it. A peer's refusal is itself a reply, so a pool too
/// small to decode one would turn the pool's own typed refusal into a decode error naming no pool.
const LEAST_REPLY: usize = 64 * 1024;

/// [`Links`] whose clients decode a peer's reply up to a working set's bytes or [`LEAST_REPLY`].
/// The only [`ChannelResolver`] over them, reached through [`Links::bounded`], so a caller names the
/// working set a reply is capped at; a [`WorkingSet`] near `usize::MAX` is still no cap in practice.
#[derive(Clone)]
struct Bounded {
    links: Links,
    working_set: WorkingSet,
}

#[async_trait::async_trait]
impl ChannelResolver for Bounded {
    async fn get_worker_client_for_url(&self, url: &Url) -> Result<Box<dyn WorkerChannel>, DataFusionError> {
        let channel = match self.links.channels.lock().await.entry(url.clone()) {
            Entry::Occupied(kept) => kept.get().clone(),
            Entry::Vacant(unseen) => unseen.insert(self.links.dial(url)?).clone(),
        };
        Ok(create_worker_client_with_decode_limit(
            channel,
            self.working_set.bytes().max(LEAST_REPLY),
        ))
    }
}
