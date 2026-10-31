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
//! - Nothing here refuses a plan that would send a leg to another node. That the legs stay is the
//!   engine routing a one-task stage, not a check of ours: the record's Decision 3 refusal is not
//!   built.
//! - A link is plaintext gRPC and carries no grant, so a node serving [`Nodes::worker`] runs any
//!   stage it is sent. The record's Decisions 4 and 5 are not built.
//! - A stage on a worker, this node's own included, reserves against that worker's pool, which is
//!   sized once for the node and not per question. Only the stage that collects the answer reserves
//!   against the question's own ceiling.
//! - No binary builds a [`Nodes`] yet.

use datafusion::error::DataFusionError;
use datafusion::execution::SessionStateBuilder;
use datafusion::prelude::{SessionConfig, SessionContext};
use datafusion_distributed::grpc::DefaultChannelResolver;
use datafusion_distributed::{
    DesiredTaskCountEvent, DesiredTaskCountEventResponse, DistributedExt as _, SessionStateBuilderExt as _, Worker,
    WorkerResolver,
};
use url::Url;

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
    channels: DefaultChannelResolver,
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
    /// This node's worker, served at `own_url`, and the other nodes a stage may be planned onto.
    #[must_use]
    pub fn declared(own: Worker, own_url: Url, others: impl IntoIterator<Item = Url>) -> Self {
        let all = core::iter::once(own_url.clone()).chain(others).collect();
        Self {
            own,
            own_url,
            all: Declared(all),
            channels: DefaultChannelResolver::default(),
        }
    }

    /// The worker a node serves: its stages reserve against a pool of `working_set` and spill
    /// nowhere, as a combine's own session does.
    ///
    /// # Errors
    ///
    /// [`CombineError::Environment`] if the bounded environment cannot be built.
    pub fn worker(working_set: WorkingSet) -> Result<Worker, CombineError> {
        let environment = pool::environment(working_set).map_err(|cause| CombineError::Environment { cause })?;
        Ok(Worker::default().with_runtime_env(environment.into_runtime()))
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
