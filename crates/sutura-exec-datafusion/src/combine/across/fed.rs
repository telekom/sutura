//! A dealt leg's rows, fed to the task that scans them on the stream its plan was set over, and
//! never carried inside that plan.
//!
//! A leg's scan, planned across nodes, is replaced by a `FedRowsExec` that holds the leg's schema
//! and the declaration of a work-unit feed (`datafusion-distributed`'s own, unchanged). The node
//! that planned the question keeps the rows; each task is sent its share as Arrow IPC batches after
//! its plan, so the plan's size does not depend on the data and lands before any consumer's wait for
//! it ends.
//!
//! **Who may read a feed.** A feed is delivered only on the `CoordinatorChannel` call that installed
//! the plan naming it, to the task that call installs: the worker keys its receivers by feed and
//! partition inside that one task's session, and the engine admits that call only for a node holding
//! the question's key (`grant`). A feed id travels in the plan, but nothing addresses a feed by it
//! from outside its call, so another question, another task, or another node cannot read it.
//!
//! # Limits
//!
//! - The rows are still all held by the planning node until the question ends, as the caller's own
//!   batches, and a remote task's share is encoded to IPC as it is sent. The worker's receive queue
//!   is unbounded and outside its pool, as the plan that carried the rows was, so a slow consumer
//!   leaves a share queued there. This moves where the rows wait; it does not bound them.
//! - A feed whose stream ends without the end marker, or whose count disagrees with it, fails the
//!   task as `Unfed::Cut` or `Unfed::Miscounted`, relayed as a lost node. A feed read by a node
//!   that holds the question's key can still lie about its rows: it is as trusted as the plan that
//!   carried them was.
//! - A feed is read once per partition: a stage that executes a fed partition twice fails with the
//!   engine's "already consumed", where a plan that carried its rows scanned them twice.
//! - A message holds the units ready at one poll, which `Dealt` keeps to about one for each
//!   partition by yielding between units. A unit is one source batch, so a message is only as small
//!   as the batches a source streams.
//! - Plan bytes are measured on the leaf's encoding and by the engine's own `plan_bytes` log in the
//!   multi-node check; no cell measures the bytes on a link.

use core::fmt;
use core::num::NonZeroUsize;
use std::io::Cursor;
use std::sync::Arc;

use datafusion::arrow::datatypes::{Schema, SchemaRef};
use datafusion::arrow::error::ArrowError;
use datafusion::arrow::ipc::reader::StreamReader;
use datafusion::arrow::ipc::writer::StreamWriter;
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::common::tree_node::TreeNodeRecursion;
use datafusion::datasource::memory::{DataSourceExec, MemorySourceConfig};
use datafusion::datasource::source::DataSource as _;
use datafusion::error::DataFusionError;
use datafusion::execution::{SendableRecordBatchStream, TaskContext};
use datafusion::physical_expr::{EquivalenceProperties, PhysicalExpr};
use datafusion::physical_plan::execution_plan::{Boundedness, EmissionType};
use datafusion::physical_plan::metrics::MetricsSet;
use datafusion::physical_plan::stream::RecordBatchStreamAdapter;
use datafusion::physical_plan::{DisplayAs, DisplayFormatType, ExecutionPlan, Partitioning, PlanProperties};
use datafusion_distributed::{
    DistributedWorkUnitFeedContext, ScaleUpLeafNodeEvent, ScaleUpLeafNodeEventResponse, WorkUnitFeed, WorkUnitFeedProto,
    WorkUnitFeedProvider,
};
use datafusion_proto::protobuf::Schema as ProtoSchema;
use futures_util::stream::{self, BoxStream};
use futures_util::{StreamExt as _, TryStreamExt as _};
use prost::Message as _;

/// One message of a feed: a batch of the leg, or the end of the task's share of it.
#[derive(Clone, PartialEq, prost::Message)]
pub(super) struct Unit {
    #[prost(oneof = "Body", tags = "1, 2")]
    body: Option<Body>,
}

#[derive(Clone, PartialEq, prost::Oneof)]
enum Body {
    /// One batch as an Arrow IPC stream of one message, which carries its own schema.
    #[prost(bytes = "vec", tag = "1")]
    Batch(Vec<u8>),
    /// How many batches this partition was sent, after the last of them.
    #[prost(uint64, tag = "2")]
    End(u64),
}

/// Why a task's share of a leg did not arrive whole.
#[derive(Debug, thiserror::Error)]
pub(super) enum Unfed {
    /// The feed's stream ended before the planning node said the share was whole.
    #[error("the rows fed to this task stopped after {seen} batches, before the feed was ended")]
    Cut { seen: u64 },
    /// The planning node ended the share at a count that is not the batches that arrived.
    #[error("the feed was ended at {announced} batches and {seen} arrived")]
    Miscounted { announced: u64, seen: u64 },
    /// A message of the feed holds neither a batch nor an end.
    #[error("a message of the feed holds neither a batch nor its end")]
    Empty,
    /// A batch did not decode, or is not one message of an IPC stream.
    #[error("a fed batch is not one Arrow IPC message")]
    Undecodable {
        #[source]
        cause: Option<ArrowError>,
    },
    /// A batch's columns are not the columns of the leg this leaf scans.
    #[error("a fed batch is not a batch of the leg this scan reads")]
    NotTheLeg,
    /// The rows were dealt across a number of tasks other than the stage runs.
    #[error("the leg's rows were dealt across {dealt} tasks and the stage runs {running}")]
    Mistasked { dealt: usize, running: usize },
    /// A batch could not be written as Arrow IPC.
    #[error("a batch of the leg could not be written as Arrow IPC")]
    Unwritable {
        #[source]
        cause: ArrowError,
    },
}

impl Unfed {
    /// This as the engine's failure, under the code the planning node reads back as a lost node
    /// when the feed was cut short, and plain otherwise.
    fn into_error(self) -> DataFusionError {
        let lost = matches!(self, Self::Cut { .. } | Self::Miscounted { .. });
        let cause = DataFusionError::External(Box::new(self));
        if lost { super::relay::feed_lost(cause) } else { cause }
    }
}

/// A leg's rows as the coordinator holds them, and how they are dealt between the stage's tasks.
///
/// The feed a stage's plan names is read by global index, a task's partitions one after another:
/// index `task * partitions + partition`, as the engine asks for them.
#[derive(Debug)]
pub(super) struct Dealt {
    rows: Arc<[Vec<RecordBatch>]>,
    tasks: NonZeroUsize,
}

impl WorkUnitFeedProvider for Dealt {
    type WorkUnit = Unit;

    fn feed(&self, partition: usize, ctx: Arc<TaskContext>) -> Result<BoxStream<'static, Result<Unit>>, DataFusionError> {
        let running = DistributedWorkUnitFeedContext::from_ctx(&ctx).fan_out_tasks;
        if running != self.tasks.get() {
            return Err(Unfed::Mistasked {
                dealt: self.tasks.get(),
                running,
            }
            .into_error());
        }
        let width = self.rows.len();
        let global = partition;
        let (Some(task), Some(partition)) = (global.checked_div(width), global.checked_rem(width)) else {
            return Ok(stream::empty().boxed());
        };
        let share = Share {
            rows: Arc::clone(&self.rows),
            partition,
            next: task,
            step: self.tasks.get(),
            sent: 0,
            ended: false,
        };
        Ok(stream::unfold(share, Share::unit).boxed())
    }
}

type Result<T, E = DataFusionError> = core::result::Result<T, E>;

/// One partition's share for one task: every `step`th batch from `next`, then its end.
struct Share {
    rows: Arc<[Vec<RecordBatch>]>,
    partition: usize,
    next: usize,
    step: usize,
    sent: u64,
    ended: bool,
}

impl Share {
    /// The next message of this share, after yielding to the runtime so the engine's chunking of
    /// ready messages ends here rather than gathering the whole share into one gRPC message.
    async fn unit(mut self) -> Option<(Result<Unit>, Self)> {
        let batch = self.rows.get(self.partition).and_then(|batches| batches.get(self.next));
        if let Some(batch) = batch {
            let written = write(batch);
            self.next = self.next.saturating_add(self.step);
            self.sent = self.sent.saturating_add(1);
            tokio::task::yield_now().await;
            let unit = written.map(|bytes| Unit {
                body: Some(Body::Batch(bytes)),
            });
            return Some((unit, self));
        }
        if self.ended {
            return None;
        }
        self.ended = true;
        let end = Unit {
            body: Some(Body::End(self.sent)),
        };
        Some((Ok(end), self))
    }
}

fn write(batch: &RecordBatch) -> Result<Vec<u8>> {
    let unwritable = |cause| Unfed::Unwritable { cause }.into_error();
    let mut writer =
        StreamWriter::try_new(Vec::with_capacity(batch.get_array_memory_size()), &batch.schema()).map_err(unwritable)?;
    writer.write(batch).map_err(unwritable)?;
    writer.finish().map_err(unwritable)?;
    writer.into_inner().map_err(unwritable)
}

/// A batch of the leg `schema` names, as one IPC message holds it.
fn read(bytes: Vec<u8>, schema: &Schema) -> Result<RecordBatch, Unfed> {
    let undecodable = |cause| Unfed::Undecodable { cause };
    let length = bytes.len();
    let mut reader = StreamReader::try_new(Cursor::new(bytes), None).map_err(|cause| undecodable(Some(cause)))?;
    let batch = reader
        .next()
        .ok_or_else(|| undecodable(None))?
        .map_err(|cause| undecodable(Some(cause)))?;
    if reader.next().is_some() || usize::try_from(reader.get_ref().position()).ok() != Some(length) {
        return Err(undecodable(None));
    }
    if batch.schema().fields() == schema.fields() {
        Ok(batch)
    } else {
        Err(Unfed::NotTheLeg)
    }
}

/// `units` as the batches of the leg `schema` names, failing rather than ending short: a stream
/// that closes before its end marker, or whose count differs from the marker's, is a lost feed.
fn received(schema: SchemaRef, units: BoxStream<'static, Result<Unit>>) -> impl stream::Stream<Item = Result<RecordBatch>> {
    stream::try_unfold((units, schema, 0_u64), |(mut units, schema, seen)| async move {
        let Some(unit) = units.try_next().await? else {
            return Err(Unfed::Cut { seen }.into_error());
        };
        match unit.body {
            Some(Body::Batch(bytes)) => {
                let batch = read(bytes, &schema).map_err(Unfed::into_error)?;
                Ok(Some((batch, (units, schema, seen.saturating_add(1)))))
            }
            Some(Body::End(announced)) if announced == seen => Ok(None),
            Some(Body::End(announced)) => Err(Unfed::Miscounted { announced, seen }.into_error()),
            None => Err(Unfed::Empty.into_error()),
        }
    })
}

/// A leg's scan whose rows reach the task as a feed after its plan: the leaf a dealt leg is planned
/// with across nodes.
pub(super) struct FedRowsExec {
    /// The feed the engine finds in a stage's plan by the getter registered with the session.
    pub(super) feed: WorkUnitFeed<Dealt>,
    properties: Arc<PlanProperties>,
}

impl fmt::Debug for FedRowsExec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FedRowsExec")
            .field("partitions", &self.properties.partitioning.partition_count())
            .finish_non_exhaustive()
    }
}

impl FedRowsExec {
    fn new(feed: WorkUnitFeed<Dealt>, schema: SchemaRef, partitions: usize) -> Self {
        let properties = Arc::new(PlanProperties::new(
            EquivalenceProperties::new(schema),
            Partitioning::UnknownPartitioning(partitions),
            EmissionType::Incremental,
            Boundedness::Bounded,
        ));
        Self { feed, properties }
    }

    /// `scan`'s rows, projected as it reads them, dealt across `tasks` and fed to each.
    fn dealing(scan: &DataSourceExec, rows: &MemorySourceConfig, tasks: NonZeroUsize) -> Result<Self> {
        let projected = |batches: &Vec<RecordBatch>| -> Result<Vec<RecordBatch>> {
            batches
                .iter()
                .map(|batch| {
                    rows.projection().as_ref().map_or_else(
                        || Ok(batch.clone()),
                        |columns| batch.project(columns).map_err(DataFusionError::from),
                    )
                })
                .collect()
        };
        let held: Vec<Vec<RecordBatch>> = rows.partitions().iter().map(projected).collect::<Result<_>>()?;
        let partitions = held.len();
        let provider = Dealt {
            rows: held.into(),
            tasks,
        };
        Ok(Self::new(WorkUnitFeed::new(provider), scan.schema(), partitions))
    }
}

impl DisplayAs for FedRowsExec {
    fn fmt_as(&self, _: DisplayFormatType, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "FedRowsExec: partitions={}",
            self.properties.partitioning.partition_count()
        )
    }
}

impl ExecutionPlan for FedRowsExec {
    fn name(&self) -> &'static str {
        "FedRowsExec"
    }

    fn properties(&self) -> &Arc<PlanProperties> {
        &self.properties
    }

    fn children(&self) -> Vec<&Arc<dyn ExecutionPlan>> {
        Vec::new()
    }

    fn apply_expressions(
        &self,
        _: &mut dyn FnMut(&Arc<dyn PhysicalExpr>) -> Result<TreeNodeRecursion>,
    ) -> Result<TreeNodeRecursion> {
        Ok(TreeNodeRecursion::Continue)
    }

    fn with_new_children(self: Arc<Self>, children: Vec<Arc<dyn ExecutionPlan>>) -> Result<Arc<dyn ExecutionPlan>> {
        if children.is_empty() {
            Ok(self)
        } else {
            Err(DataFusionError::Internal(String::from(
                "a fed scan reads no input, so it took no children",
            )))
        }
    }

    fn execute(&self, partition: usize, context: Arc<TaskContext>) -> Result<SendableRecordBatchStream> {
        let units = self.feed.feed(partition, context)?;
        let schema = self.schema();
        Ok(Box::pin(RecordBatchStreamAdapter::new(
            Arc::clone(&schema),
            received(schema, units),
        )))
    }

    fn metrics(&self) -> Option<MetricsSet> {
        Some(self.feed.metrics().clone_inner())
    }
}

/// A leg's scan whose rows may be split between tasks: one with no limit and no declared order.
pub(super) fn dealable(leaf: &Arc<dyn ExecutionPlan>) -> Option<&MemorySourceConfig> {
    let rows = leaf
        .downcast_ref::<DataSourceExec>()?
        .data_source()
        .downcast_ref::<MemorySourceConfig>()?;
    (rows.fetch().is_none() && rows.sort_information().is_empty()).then_some(rows)
}

/// `event`'s leg scan as a `FedRowsExec` whose feed deals each task every `task_count`th batch of
/// each partition, so the tasks read the leg's rows between them and none twice.
pub(super) fn dealt(event: ScaleUpLeafNodeEvent<'_>) -> Option<Result<ScaleUpLeafNodeEventResponse>> {
    let rows = dealable(event.plan)?;
    let scan = event.plan.downcast_ref::<DataSourceExec>()?;
    let tasks = NonZeroUsize::new(event.task_count).filter(|tasks| tasks.get() > 1)?;
    Some(FedRowsExec::dealing(scan, rows, tasks).map(|leaf| ScaleUpLeafNodeEventResponse::new(Arc::new(leaf))))
}

/// The leaf's declaration on a link: its feed's id, its schema and its partition count - and no row.
#[derive(Clone, PartialEq, prost::Message)]
struct Declared {
    #[prost(message, optional, tag = "1")]
    feed: Option<WorkUnitFeedProto>,
    #[prost(message, optional, tag = "2")]
    schema: Option<ProtoSchema>,
    #[prost(uint64, tag = "3")]
    partitions: u64,
}

/// `node`, a `FedRowsExec`, written into `buf`.
pub(super) fn encode(node: &Arc<dyn ExecutionPlan>, buf: &mut Vec<u8>) -> Result<()> {
    let leaf = node
        .downcast_ref::<FedRowsExec>()
        .ok_or_else(|| DataFusionError::NotImplemented(String::from("not a fed scan")))?;
    let schema = ProtoSchema::try_from(leaf.properties.eq_properties.schema().as_ref())
        .map_err(|cause| DataFusionError::External(Box::new(cause)))?;
    let declared = Declared {
        feed: Some(leaf.feed.to_proto()),
        schema: Some(schema),
        partitions: u64::try_from(leaf.properties.partitioning.partition_count()).unwrap_or(u64::MAX),
    };
    declared
        .encode(buf)
        .map_err(|cause| DataFusionError::External(Box::new(cause)))
}

/// A `FedRowsExec` that reads its rows off the feed a [`Declared`] names.
pub(super) fn decode(buf: &[u8], inputs: &[Arc<dyn ExecutionPlan>]) -> Result<Arc<dyn ExecutionPlan>> {
    let malformed = || DataFusionError::Internal(String::from("a fed scan did not decode"));
    if !inputs.is_empty() {
        return Err(malformed());
    }
    let declared = Declared::decode(buf).map_err(|_proto| malformed())?;
    let schema = Schema::try_from(&declared.schema.ok_or_else(malformed)?).map_err(|_arrow| malformed())?;
    let partitions = usize::try_from(declared.partitions).map_err(|_width| malformed())?;
    let feed = WorkUnitFeed::from_proto(declared.feed.ok_or_else(malformed)?)?;
    Ok(Arc::new(FedRowsExec::new(feed, Arc::new(schema), partitions)))
}

/// A fed leaf's declaration of itself, in the shape a stage carries it.
#[cfg(test)]
mod tests;
