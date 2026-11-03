//! The federated row ceiling, checked early by the worker running a task of a question's final
//! stage (`docs/adr/20261102082308-workers-read-sources.md`, Decision 4).
//!
//! The planning node marks the stage directly under the network boundary that hands it the combined
//! set with the whole `governance.federated_row_ceiling`, which the stage carries as text. A task of
//! that stage counts the rows its partitions emit and fails at the batch that takes it past the
//! ceiling, as `WorkerRefusal::TaskOverCeiling`. The planning node's own exact count of the combined
//! set is unchanged.
//!
//! # Limits
//!
//! - Per task, so partial: tasks that each stay at or under the ceiling can sum past it, which only
//!   the planning node's exact count refuses.
//! - Only a stage reached from the top of the plan through nodes that keep each row they are handed
//!   and add none is marked. A regrouping, filter, limit or fetch above the boundary leaves the stage
//!   unmarked, and so is every stage below another stage.
//! - The ceiling rides in the stage, which the task grant does not cover: the node that installs a
//!   stage supplies its question's key, so it could raise or drop the ceiling under a grant covering
//!   it as well, as it could install a stage with no check at all. That loosens only this early
//!   check.
//! - A worker's refusal crosses the link under the code the planning node reads back as its own
//!   refusal for rows past the ceiling, `CombineError::RowsOverCeiling`. That refusal has no fields,
//!   so the caller is told this node's ceiling, not the one the stage carried. A node that wraps the
//!   code in an Arrow or an external error passes it on as text, and the caller then sees the
//!   engine's failure of the combine.
//! - Tasks run in one process off one decoded stage share one count. That refuses no question the
//!   planning node's count answers, since their rows are disjoint pieces of the combined set.

use core::fmt;
use core::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use datafusion::catalog::Session;
use datafusion::common::tree_node::TreeNodeRecursion;
use datafusion::error::DataFusionError;
use datafusion::execution::context::QueryPlanner;
use datafusion::execution::{SendableRecordBatchStream, SessionStateBuilder, TaskContext};
use datafusion::logical_expr::LogicalPlan;
use datafusion::physical_expr::PhysicalExpr;
use datafusion::physical_plan::execution_plan::CardinalityEffect;
use datafusion::physical_plan::stream::RecordBatchStreamAdapter;
use datafusion::physical_plan::{DisplayAs, DisplayFormatType, ExecutionPlan, PlanProperties, replace_children_if_necessary};
use datafusion_distributed::{DistributedExec, DistributedExt as _, NetworkCoalesceExec};
use datafusion_proto::physical_plan::{PhysicalExtensionCodec, PhysicalProtoConverterExtension};
use futures_util::StreamExt as _;
use sutura_domain::plan::FederatedRowCeiling;

/// Why a worker refused a task it was running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WorkerRefusal {
    /// A task of the question's final stage emitted more rows than the whole federated row ceiling,
    /// so the combined set it is a piece of holds more too.
    #[error("a task of the question's final stage held more rows than its federated row ceiling of {} rows", .ceiling.get())]
    TaskOverCeiling {
        /// The ceiling the stage carried.
        ceiling: FederatedRowCeiling,
    },
    /// A source-reading task's grant did not name the leg digest, source and partition the task
    /// actually reads, or named them correctly but failed to verify under the task's own key - which
    /// also covers a grant whose expiry, caller or stream bound was tampered with after signing,
    /// since every field feeds the signature, not only the three this refusal names
    /// (`grant::reading::verify`, Decision 5). `verify` takes a grant non-optionally, so a task
    /// carrying no grant at all cannot reach this variant; that refusal arrives only with the later
    /// slice that wires a reading stage and has to decide what a missing grant means.
    #[error("this reading task's grant does not name its leg, source and partition")]
    GrantNotNaming,
    /// A source-reading task's grant named its leg, source and partition, but its expiry had already
    /// passed the worker's own clock.
    #[error("this reading task's grant is past its expiry")]
    GrantExpired,
}

/// A final-stage task's rows, counted against the whole federated row ceiling as they pass.
#[derive(Debug)]
pub(super) struct CeilingExec {
    input: Arc<dyn ExecutionPlan>,
    ceiling: FederatedRowCeiling,
    rows: Arc<AtomicU64>,
}

impl CeilingExec {
    pub(super) fn new(input: Arc<dyn ExecutionPlan>, ceiling: FederatedRowCeiling) -> Self {
        Self {
            input,
            ceiling,
            rows: Arc::default(),
        }
    }
}

impl DisplayAs for CeilingExec {
    fn fmt_as(&self, _: DisplayFormatType, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CeilingExec: rows<={}", self.ceiling.get())
    }
}

impl ExecutionPlan for CeilingExec {
    fn name(&self) -> &'static str {
        "CeilingExec"
    }

    fn properties(&self) -> &Arc<PlanProperties> {
        self.input.properties()
    }

    fn children(&self) -> Vec<&Arc<dyn ExecutionPlan>> {
        vec![&self.input]
    }

    fn apply_expressions(
        &self,
        _: &mut dyn FnMut(&Arc<dyn PhysicalExpr>) -> Result<TreeNodeRecursion, DataFusionError>,
    ) -> Result<TreeNodeRecursion, DataFusionError> {
        Ok(TreeNodeRecursion::Continue)
    }

    fn maintains_input_order(&self) -> Vec<bool> {
        vec![true]
    }

    fn cardinality_effect(&self) -> CardinalityEffect {
        CardinalityEffect::Equal
    }

    fn with_new_children(
        self: Arc<Self>,
        children: Vec<Arc<dyn ExecutionPlan>>,
    ) -> Result<Arc<dyn ExecutionPlan>, DataFusionError> {
        let [input] = <[_; 1]>::try_from(children).map_err(|children| {
            DataFusionError::Internal(format!("a stage's row ceiling counts one input, not {}", children.len()))
        })?;
        Ok(Arc::new(Self::new(input, self.ceiling)))
    }

    fn execute(&self, partition: usize, context: Arc<TaskContext>) -> Result<SendableRecordBatchStream, DataFusionError> {
        let input = self.input.execute(partition, context)?;
        let schema = input.schema();
        let rows = Arc::clone(&self.rows);
        let ceiling = self.ceiling;
        let counted = input.map(move |batch| {
            let batch = batch?;
            let these = u64::try_from(batch.num_rows()).unwrap_or(u64::MAX);
            if rows.fetch_add(these, Ordering::Relaxed).saturating_add(these) > u64::from(ceiling.get()) {
                return Err(super::relay::over_ceiling(DataFusionError::External(Box::new(
                    WorkerRefusal::TaskOverCeiling { ceiling },
                ))));
            }
            Ok(batch)
        });
        Ok(Box::pin(RecordBatchStreamAdapter::new(schema, counted)))
    }
}

/// The codec a marked stage crosses a link in: its ceiling as decimal text, beside its one input.
#[derive(Debug)]
pub(super) struct Codec;

impl PhysicalExtensionCodec for Codec {
    fn try_decode(
        &self,
        buf: &[u8],
        inputs: &[Arc<dyn ExecutionPlan>],
        _: &TaskContext,
        _: &dyn PhysicalProtoConverterExtension,
    ) -> Result<Arc<dyn ExecutionPlan>, DataFusionError> {
        let ceiling = core::str::from_utf8(buf)
            .ok()
            .and_then(|text| text.parse().ok())
            .and_then(|rows| FederatedRowCeiling::parse(rows).ok());
        match (ceiling, inputs) {
            (Some(ceiling), [input]) => Ok(Arc::new(CeilingExec::new(Arc::clone(input), ceiling))),
            _ => Err(DataFusionError::Internal(String::from(
                "a stage's federated row ceiling did not decode",
            ))),
        }
    }

    fn try_encode(
        &self,
        node: Arc<dyn ExecutionPlan>,
        buf: &mut Vec<u8>,
        _: &dyn PhysicalProtoConverterExtension,
    ) -> Result<(), DataFusionError> {
        let marked = node
            .downcast_ref::<CeilingExec>()
            .ok_or_else(|| DataFusionError::NotImplemented(String::from("not a stage's federated row ceiling")))?;
        buf.extend_from_slice(marked.ceiling.get().to_string().as_bytes());
        Ok(())
    }
}

/// `builder`, whose distributed planner is set, marking each question's final stage with `ceiling`.
pub(super) fn counted(
    mut builder: SessionStateBuilder,
    ceiling: FederatedRowCeiling,
) -> Result<SessionStateBuilder, DataFusionError> {
    let slot = builder.query_planner();
    let planner = slot.take().ok_or_else(|| {
        DataFusionError::Configuration(String::from(
            "a question's final stage is marked only once its distributed planner is set",
        ))
    })?;
    *slot = Some(Arc::new(Marked { planner, ceiling }));
    Ok(builder.with_distributed_user_codec(Codec))
}

/// A planner that marks the final stage of each question it plans.
#[derive(Debug)]
struct Marked {
    planner: Arc<dyn QueryPlanner + Send + Sync>,
    ceiling: FederatedRowCeiling,
}

#[async_trait::async_trait]
impl QueryPlanner for Marked {
    async fn create_physical_plan(
        &self,
        logical_plan: &LogicalPlan,
        session: &dyn Session,
    ) -> Result<Arc<dyn ExecutionPlan>, DataFusionError> {
        marked(self.planner.create_physical_plan(logical_plan, session).await?, self.ceiling)
    }
}

type Plan = Arc<dyn ExecutionPlan>;

/// `plan` with the stage under its first network boundary counted against `ceiling`, when that
/// boundary gathers the stage's tasks and each node above it keeps every row it is handed and adds
/// none; `plan` itself otherwise.
fn marked(plan: Plan, ceiling: FederatedRowCeiling) -> Result<Plan, DataFusionError> {
    let only = match plan.children().as_slice() {
        [child] => Some(Arc::clone(child)),
        _ => None,
    };
    let Some(child) = only else {
        return Ok(plan);
    };
    let replaced: Plan = if plan.is::<NetworkCoalesceExec>() {
        Arc::new(CeilingExec::new(child, ceiling))
    } else if plan.is::<DistributedExec>()
        || (matches!(plan.cardinality_effect(), CardinalityEffect::Equal) && plan.fetch().is_none())
    {
        marked(child, ceiling)?
    } else {
        return Ok(plan);
    };
    replace_children_if_necessary(plan, vec![replaced])
}
