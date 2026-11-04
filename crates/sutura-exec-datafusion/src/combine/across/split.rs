//! A split leg read on the workers that read its source
//! (`docs/adr/20261102082308-workers-read-sources.md`, Decisions 1 and 5): each partition of a
//! [`SplitScan`](crate::SplitScan) is read by a `PartitionScanExec` carrying its own grant and the
//! key drawn for that partition alone, and a task of that stage is placed only on a node
//! [`Nodes::reading`](crate::Nodes::reading) names for the leg's source. The planning node reads none
//! of it.
//!
//! A partition's key and grant ride in its task's own plan variant, which the engine encodes for,
//! and sends to, the one node that task is routed to: no other stage node is sent them.
//!
//! # Limits
//!
//! - No served question builds a [`SplitScan`](crate::SplitScan) yet: the planning node checks a
//!   lookup leg's link keys for duplicates only over rows it read itself, so a split leg is still read
//!   whole there until the cross-partition ambiguous-link check lands.
//! - The planning node mints, and so holds, every partition's key.
//! - The tasks are at most the nodes resolved, this one among them. With fewer tasks than
//!   partitions, a task reads each partition whose index leaves its own remainder by the task count,
//!   each under its own key.
//! - A leg planned as one task - one partition, or no other node resolved - fails rather than being
//!   read: its scan reads only inside a reading task, and the codec refuses to send it whole.
//! - A stage with two split legs goes only to nodes reading both sources. A stage whose sources no
//!   resolved node reads together is refused (`NoReadingNode`), never placed elsewhere.
//! - Filters and limits run above the scan, on the reading worker, not in the source.
//! - A `BigQuery` leg is not read on a worker: its token handoff is not built.
//! - [`StreamBound`](crate::StreamBound) is signed into each grant, and no worker enforces it.
//! - A worker admits a partition's first key: the same leg's same partition asked of it again within
//!   that key's idle window, under a fresh key, is refused.

use core::fmt;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use datafusion::arrow::datatypes::SchemaRef;
use datafusion::catalog::{Session, TableProvider};
use datafusion::common::tree_node::{TreeNode as _, TreeNodeRecursion};
use datafusion::error::DataFusionError;
use datafusion::execution::{SendableRecordBatchStream, TaskContext};
use datafusion::logical_expr::{Expr, TableType};
use datafusion::physical_expr::expressions::Column;
use datafusion::physical_expr::{EquivalenceProperties, PhysicalExpr};
use datafusion::physical_plan::empty::EmptyExec;
use datafusion::physical_plan::execution_plan::{Boundedness, EmissionType};
use datafusion::physical_plan::projection::ProjectionExec;
use datafusion::physical_plan::union::UnionExec;
use datafusion::physical_plan::{DisplayAs, DisplayFormatType, ExecutionPlan, Partitioning, PlanProperties};
use datafusion_distributed::{
    DesiredTaskCountEvent, DesiredTaskCountEventResponse, DistributedLeafExec, NetworkBoundaryExt as _, RouteTasksEvent,
    RouteTasksEventResponse, ScaleUpLeafNodeEvent, ScaleUpLeafNodeEventResponse,
};
use sutura_domain::identity::SubjectKey;
use sutura_domain::model::SourceName;
use url::Url;

use super::NoReadingNode;
use super::grant;
use super::partition::PartitionScanExec;

/// A leg split into partitions, each read only by a task on a worker that reads its source.
#[derive(Debug)]
pub struct SplitScan(Arc<Leg>);

struct Leg {
    wire: Vec<u8>,
    source: SourceName,
    schema: SchemaRef,
    partitions: Vec<Granted>,
}

impl fmt::Debug for Leg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Leg")
            .field("source", &self.source)
            .field("partitions", &self.partitions.len())
            .finish_non_exhaustive()
    }
}

/// The bound a split leg's every partition stream is read under, signed into its grant: at most
/// `rows` rows and `bytes` bytes.
#[derive(Clone, Copy, Debug)]
pub struct StreamBound {
    rows: u32,
    bytes: u32,
}

impl StreamBound {
    /// A partition's stream bounded at `rows` rows and `bytes` bytes.
    #[must_use]
    pub const fn new(rows: u32, bytes: u32) -> Self {
        Self { rows, bytes }
    }
}

/// One partition's descriptor, the key drawn for it, and the grant signed under that key.
struct Granted {
    partition: Vec<u8>,
    key: [u8; 32],
    grant: Vec<u8>,
}

impl SplitScan {
    /// The leg `wire` carries, on `source`, read as `partitions`, each granted to `caller` until
    /// `until` under `bound` and a key of its own.
    ///
    /// # Errors
    ///
    /// [`DataFusionError::Plan`] for no partitions, and the engine's failure if no key can be drawn.
    pub fn granted(
        wire: Vec<u8>,
        source: SourceName,
        schema: SchemaRef,
        partitions: impl IntoIterator<Item = Vec<u8>>,
        caller: &SubjectKey,
        until: SystemTime,
        bound: StreamBound,
    ) -> Result<Self, DataFusionError> {
        // A deadline before the epoch is already past: every grant under it is refused.
        let until = until.duration_since(UNIX_EPOCH).unwrap_or_default();
        let partitions = partitions
            .into_iter()
            .map(|partition| {
                let (key, grant) = grant::granted_read(&wire, &source, &partition, caller, until, bound.rows, bound.bytes)?;
                Ok(Granted { partition, key, grant })
            })
            .collect::<Result<Vec<_>, DataFusionError>>()?;
        if partitions.is_empty() {
            return Err(DataFusionError::Plan(format!("a split leg of `{source}` has no partition")));
        }
        Ok(Self(Arc::new(Leg {
            wire,
            source,
            schema,
            partitions,
        })))
    }
}

#[async_trait::async_trait]
impl TableProvider for SplitScan {
    fn schema(&self) -> SchemaRef {
        Arc::clone(&self.0.schema)
    }

    fn table_type(&self) -> TableType {
        TableType::Base
    }

    async fn scan(
        &self,
        _: &dyn Session,
        projection: Option<&Vec<usize>>,
        _: &[Expr],
        _: Option<usize>,
    ) -> Result<Arc<dyn ExecutionPlan>, DataFusionError> {
        let leaf: Arc<dyn ExecutionPlan> = Arc::new(SplitLeafExec::of(Arc::clone(&self.0)));
        let Some(columns) = projection else {
            return Ok(leaf);
        };
        let fields = self.0.schema.fields();
        let exprs = columns
            .iter()
            .map(|&index| {
                let field = fields
                    .get(index)
                    .ok_or_else(|| DataFusionError::Plan(format!("a split leg has no column {index}")))?;
                Ok((
                    Arc::new(Column::new(field.name(), index)) as Arc<dyn PhysicalExpr>,
                    field.name().clone(),
                ))
            })
            .collect::<Result<Vec<_>, DataFusionError>>()?;
        Ok(Arc::new(ProjectionExec::try_new(exprs, leaf)?))
    }
}

/// The planning node's marker for a split leg: one partition per partition of the leg, never read
/// here. The engine replaces it with one reading variant per task.
#[derive(Debug)]
struct SplitLeafExec {
    leg: Arc<Leg>,
    properties: Arc<PlanProperties>,
}

impl SplitLeafExec {
    fn of(leg: Arc<Leg>) -> Self {
        let properties = Arc::new(PlanProperties::new(
            EquivalenceProperties::new(Arc::clone(&leg.schema)),
            Partitioning::UnknownPartitioning(leg.partitions.len()),
            EmissionType::Incremental,
            Boundedness::Bounded,
        ));
        Self { leg, properties }
    }
}

impl DisplayAs for SplitLeafExec {
    fn fmt_as(&self, _: DisplayFormatType, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SplitLeafExec: source={}", self.leg.source)
    }
}

impl ExecutionPlan for SplitLeafExec {
    fn name(&self) -> &'static str {
        "SplitLeafExec"
    }

    fn properties(&self) -> &Arc<PlanProperties> {
        &self.properties
    }

    fn children(&self) -> Vec<&Arc<dyn ExecutionPlan>> {
        Vec::new()
    }

    fn apply_expressions(
        &self,
        _: &mut dyn FnMut(&Arc<dyn PhysicalExpr>) -> Result<TreeNodeRecursion, DataFusionError>,
    ) -> Result<TreeNodeRecursion, DataFusionError> {
        Ok(TreeNodeRecursion::Continue)
    }

    fn with_new_children(
        self: Arc<Self>,
        children: Vec<Arc<dyn ExecutionPlan>>,
    ) -> Result<Arc<dyn ExecutionPlan>, DataFusionError> {
        if children.is_empty() {
            Ok(self)
        } else {
            Err(DataFusionError::Internal(String::from(
                "a split leg reads no input, so it took no children",
            )))
        }
    }

    fn execute(&self, _: usize, _: Arc<TaskContext>) -> Result<SendableRecordBatchStream, DataFusionError> {
        Err(DataFusionError::Execution(format!(
            "a split leg of `{}` is read only by its reading tasks, and was planned as one",
            self.leg.source
        )))
    }
}

/// A split leg's stage asks for one task per partition at most.
pub(super) fn desired(event: DesiredTaskCountEvent<'_>) -> Option<Result<DesiredTaskCountEventResponse, DataFusionError>> {
    let split = event.plan.downcast_ref::<SplitLeafExec>()?;
    Some(Ok(DesiredTaskCountEventResponse::maximum(split.leg.partitions.len())))
}

/// A split leg as one variant per task: each partition's slot holds its granted scan in the task
/// whose index is the partition's remainder by the task count, and no rows in every other task.
pub(super) fn scaled(event: ScaleUpLeafNodeEvent<'_>) -> Option<Result<ScaleUpLeafNodeEventResponse, DataFusionError>> {
    let leg = &event.plan.downcast_ref::<SplitLeafExec>()?.leg;
    let tasks = event.task_count;
    (tasks > 1).then(|| {
        let variants = (0..tasks)
            .map(|task| {
                UnionExec::try_new(
                    leg.partitions
                        .iter()
                        .enumerate()
                        .map(|(index, granted)| {
                            if index.checked_rem(tasks) == Some(task) {
                                Arc::new(PartitionScanExec::new(
                                    leg.wire.clone(),
                                    leg.source.clone(),
                                    granted.partition.clone(),
                                    granted.key,
                                    Some(granted.grant.clone()),
                                    Arc::clone(&leg.schema),
                                )) as Arc<dyn ExecutionPlan>
                            } else {
                                Arc::new(EmptyExec::new(Arc::clone(&leg.schema)))
                            }
                        })
                        .collect(),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let leaf = DistributedLeafExec::try_new(Arc::clone(event.plan), variants)?;
        Ok(ScaleUpLeafNodeEventResponse::new(Arc::new(leaf)))
    })
}

/// The split leg `node` is, before or after the engine scaled it into variants.
fn split_of(node: &Arc<dyn ExecutionPlan>) -> Option<&Leg> {
    node.downcast_ref::<DistributedLeafExec>()
        .map_or(node, DistributedLeafExec::original)
        .downcast_ref::<SplitLeafExec>()
        .map(|split| split.leg.as_ref())
}

/// A stage reading split legs, its tasks dealt in turn over the nodes `readers` names as reading
/// every one of their sources, or refused as [`NoReadingNode`] when none does. Any other stage is
/// left to the engine's own routing.
pub(super) fn routed(
    event: &RouteTasksEvent<'_>,
    readers: &HashMap<SourceName, Vec<Url>>,
) -> Option<Result<RouteTasksEventResponse, DataFusionError>> {
    let mut sources: Vec<SourceName> = Vec::new();
    let walked = event.plan.apply(|node| {
        if node.as_network_boundary().is_some() {
            return Ok(TreeNodeRecursion::Jump);
        }
        if let Some(leg) = split_of(node) {
            sources.push(leg.source.clone());
        }
        Ok(TreeNodeRecursion::Continue)
    });
    if let Err(cause) = walked {
        return Some(Err(cause));
    }
    let first = sources.first()?;
    let reading = |source: &SourceName| readers.get(source).map_or_default(Vec::as_slice);
    let nodes: Vec<&Url> = reading(first)
        .iter()
        .filter(|&node| sources.iter().all(|source| reading(source).contains(node)))
        .collect();
    if nodes.is_empty() {
        return Some(Err(DataFusionError::External(Box::new(NoReadingNode {
            alias: first.clone(),
        }))));
    }
    Some(Ok(RouteTasksEventResponse::new(
        nodes.into_iter().cycle().take(event.task_count).cloned().collect(),
    )))
}
