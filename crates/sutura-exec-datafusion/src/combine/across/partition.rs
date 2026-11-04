//! The leaf a reading task's plan carries: a granted read of one source's one partition
//! (`docs/adr/20261102082308-workers-read-sources.md` Decision 5), and the worker admit rule and
//! codec that carry and check it.
//!
//! **Build steps 1-2 only.** Nothing plans [`PartitionScanExec`](partition::PartitionScanExec) yet -
//! no caller outside this module's own tests mints a grant for one or sends it across a link.
//! [`super::Nodes::worker_reading`]
//! is a reading worker's own construction, parallel to [`super::Nodes::worker`], not yet wired to
//! `crates/sutura-cli`: that, the app-level port, and the second relay code mapping a worker's
//! refused re-certification back to [`crate::CombineError`], are a later slice.
//!
//! # Limits
//!
//! - [`PartitionReader`](partition::PartitionReader) is a fake behind a trait a test supplies; no
//!   adapter implements it here.
//! - The leaf's schema crosses the wire through `datafusion_proto`'s own `Schema` protobuf, the same
//!   round trip `combine::tests::across::decode` already exercises for one.
//! - A leaf carrying no grant, or one that fails to decode, reads the same as one wrongly named: the
//!   admit rule, not the codec, decides what a missing grant means
//!   ([`WorkerRefusal::GrantNotNaming`](super::WorkerRefusal::GrantNotNaming), widened for exactly this case).

use core::fmt;
use core::time::Duration;
use std::sync::Arc;
use std::time::SystemTime;

use datafusion::arrow::datatypes::{Schema, SchemaRef};
use datafusion::common::tree_node::{TreeNode as _, TreeNodeRecursion};
use datafusion::error::DataFusionError;
use datafusion::execution::{SendableRecordBatchStream, TaskContext};
use datafusion::physical_expr::EquivalenceProperties;
use datafusion::physical_plan::execution_plan::{Boundedness, EmissionType};
use datafusion::physical_plan::{DisplayAs, DisplayFormatType, ExecutionPlan, Partitioning, PlanProperties};
use datafusion_distributed::{WorkerPlanRewriteEvent, WorkerPlanRewriteEventResponse};
use datafusion_proto::physical_plan::{PhysicalExtensionCodec, PhysicalProtoConverterExtension};
use datafusion_proto::protobuf::Schema as ProtoSchema;
use prost::Message as _;
use sutura_domain::model::SourceName;

use super::grant::{self, ReadingKeys};
use super::{StageReadsASource, carried};

/// Reads one partition of one source for a reading task - a fake behind this trait in every test
/// here; no adapter implements it in this slice. `pub`, not `pub(super)`: [`super::Nodes::worker_reading`]
/// names it in a `pub fn` signature, so this trait cannot be more private than that function.
pub trait PartitionReader: Send + Sync {
    /// Reads `source`'s `partition`, against `schema`.
    fn read(
        &self,
        source: &SourceName,
        partition: &[u8],
        schema: &SchemaRef,
    ) -> Result<SendableRecordBatchStream, DataFusionError>;
}

impl<F> PartitionReader for F
where
    F: Fn(&SourceName, &[u8], &SchemaRef) -> Result<SendableRecordBatchStream, DataFusionError> + Send + Sync,
{
    fn read(
        &self,
        source: &SourceName,
        partition: &[u8],
        schema: &SchemaRef,
    ) -> Result<SendableRecordBatchStream, DataFusionError> {
        self(source, partition, schema)
    }
}

/// A reading worker's own reader, carried on its session so [`PartitionScanExec::execute`] can
/// reach it without the leaf itself holding one - the leaf crosses a codec, a reader does not.
pub(super) struct ReaderExtension(pub(super) Arc<dyn PartitionReader>);

/// A granted read of one source's one partition: the leg it reads, the partition descriptor, the
/// reading key this call presented, and the grant and signature naming the read - or no grant, a
/// case the admit rule reads as [`WorkerRefusal::GrantNotNaming`](super::WorkerRefusal::GrantNotNaming).
pub(super) struct PartitionScanExec {
    wire: Vec<u8>,
    source: SourceName,
    partition: Vec<u8>,
    presented_key: [u8; 32],
    grant: Option<Vec<u8>>,
    properties: Arc<PlanProperties>,
}

impl fmt::Debug for PartitionScanExec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PartitionScanExec")
            .field("source", &self.source)
            .field("grant", &self.grant.is_some())
            .finish_non_exhaustive()
    }
}

impl PartitionScanExec {
    pub(super) fn new(
        wire: Vec<u8>,
        source: SourceName,
        partition: Vec<u8>,
        presented_key: [u8; 32],
        grant: Option<Vec<u8>>,
        schema: SchemaRef,
    ) -> Self {
        let properties = Arc::new(PlanProperties::new(
            EquivalenceProperties::new(schema),
            Partitioning::UnknownPartitioning(1),
            EmissionType::Incremental,
            Boundedness::Bounded,
        ));
        Self {
            wire,
            source,
            partition,
            presented_key,
            grant,
            properties,
        }
    }
}

impl DisplayAs for PartitionScanExec {
    fn fmt_as(&self, _: DisplayFormatType, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PartitionScanExec: source={}", self.source)
    }
}

impl ExecutionPlan for PartitionScanExec {
    fn name(&self) -> &'static str {
        "PartitionScanExec"
    }

    fn properties(&self) -> &Arc<PlanProperties> {
        &self.properties
    }

    fn children(&self) -> Vec<&Arc<dyn ExecutionPlan>> {
        Vec::new()
    }

    fn apply_expressions(
        &self,
        _: &mut dyn FnMut(&Arc<dyn datafusion::physical_expr::PhysicalExpr>) -> Result<TreeNodeRecursion, DataFusionError>,
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
                "a partition scan reads no input, so it took no children",
            )))
        }
    }

    fn execute(&self, partition: usize, context: Arc<TaskContext>) -> Result<SendableRecordBatchStream, DataFusionError> {
        if partition != 0 {
            return Err(DataFusionError::Internal(String::from(
                "a partition scan has exactly one partition",
            )));
        }
        let reader = context
            .session_config()
            .get_extension::<ReaderExtension>()
            .ok_or_else(|| DataFusionError::Execution(String::from("this worker was built with no partition reader")))?;
        reader
            .0
            .read(&self.source, &self.partition, self.properties.eq_properties.schema())
    }
}

/// The composite user codec for a reading worker: [`super::ceiling::Codec`]'s stage marker, or this
/// module's [`PartitionScanExec`], told apart by a leading tag byte on decode and dispatched by type
/// on encode. Anything else is refused rather than guessed at.
#[derive(Debug)]
pub(super) struct Codec;

const CEILING: u8 = 0;
const PARTITION: u8 = 1;

impl PhysicalExtensionCodec for Codec {
    fn try_decode(
        &self,
        buf: &[u8],
        inputs: &[Arc<dyn ExecutionPlan>],
        ctx: &TaskContext,
        proto_converter: &dyn PhysicalProtoConverterExtension,
    ) -> Result<Arc<dyn ExecutionPlan>, DataFusionError> {
        let (&tag, rest) = buf.split_first().ok_or_else(|| {
            DataFusionError::Internal(String::from("an empty node carries neither a ceiling nor a partition scan"))
        })?;
        match tag {
            CEILING => super::ceiling::Codec.try_decode(rest, inputs, ctx, proto_converter),
            PARTITION => decode_partition(rest),
            other => Err(DataFusionError::NotImplemented(format!(
                "a reading worker's codec does not know node tag {other}"
            ))),
        }
    }

    fn try_encode(
        &self,
        node: Arc<dyn ExecutionPlan>,
        buf: &mut Vec<u8>,
        proto_converter: &dyn PhysicalProtoConverterExtension,
    ) -> Result<(), DataFusionError> {
        if node.is::<PartitionScanExec>() {
            buf.push(PARTITION);
            encode_partition(&node, buf)
        } else {
            buf.push(CEILING);
            super::ceiling::Codec.try_encode(node, buf, proto_converter)
        }
    }
}

/// Writes `bytes` into `buf` as a decimal length, a `:`, then `bytes` itself - [`take_field`]'s
/// counterpart, and the same construction `grant::reading`'s own field codec uses, duplicated here
/// since that module's helpers are private to it (`grant::reading`'s own doc states the same
/// preference for a local copy over a shared one).
fn put_field(buf: &mut Vec<u8>, bytes: &[u8]) {
    buf.extend_from_slice(bytes.len().to_string().as_bytes());
    buf.push(b':');
    buf.extend_from_slice(bytes);
}

/// Reads one [`put_field`]-written field from the front of `*rest`, advancing `*rest` past it.
fn take_field(rest: &mut &[u8]) -> Option<Vec<u8>> {
    let colon = rest.iter().position(|&byte| byte == b':')?;
    let (digits, tail) = rest.split_at(colon);
    let length: usize = core::str::from_utf8(digits).ok()?.parse().ok()?;
    let tail = tail.get(1..)?;
    if tail.len() < length {
        return None;
    }
    let (field, tail) = tail.split_at(length);
    *rest = tail;
    Some(field.to_vec())
}

type DecodedPartitionScan = Result<Arc<dyn ExecutionPlan>, DataFusionError>;

fn decode_partition(buf: &[u8]) -> DecodedPartitionScan {
    let malformed = || DataFusionError::Internal(String::from("a partition scan did not decode"));
    let mut rest = buf;
    let wire = take_field(&mut rest).ok_or_else(malformed)?;
    let source_text = take_field(&mut rest).ok_or_else(malformed)?;
    let source = SourceName::parse(String::from_utf8(source_text).map_err(|_utf8| malformed())?).map_err(|_name| malformed())?;
    let part = take_field(&mut rest).ok_or_else(malformed)?;
    let key = take_field(&mut rest).ok_or_else(malformed)?;
    let presented_key = <[u8; 32]>::try_from(key.as_slice()).map_err(|_length| malformed())?;
    let grant_bytes = take_field(&mut rest).ok_or_else(malformed)?;
    let grant = (!grant_bytes.is_empty()).then_some(grant_bytes);
    let schema_bytes = take_field(&mut rest).ok_or_else(malformed)?;
    let proto_schema = ProtoSchema::decode(schema_bytes.as_slice()).map_err(|_proto| malformed())?;
    let schema: Schema = (&proto_schema).try_into().map_err(|_arrow| malformed())?;
    Ok(Arc::new(PartitionScanExec::new(
        wire,
        source,
        part,
        presented_key,
        grant,
        Arc::new(schema),
    )))
}

fn encode_partition(node: &Arc<dyn ExecutionPlan>, buf: &mut Vec<u8>) -> Result<(), DataFusionError> {
    let leaf = node
        .downcast_ref::<PartitionScanExec>()
        .ok_or_else(|| DataFusionError::NotImplemented(String::from("not a partition scan")))?;
    let proto_schema: ProtoSchema = leaf
        .properties
        .eq_properties
        .schema()
        .as_ref()
        .try_into()
        .map_err(|cause| DataFusionError::External(Box::new(cause)))?;
    put_field(buf, &leaf.wire);
    put_field(buf, leaf.source.as_str().as_bytes());
    put_field(buf, &leaf.partition);
    put_field(buf, &leaf.presented_key);
    put_field(buf, leaf.grant.as_deref().unwrap_or_default());
    put_field(buf, &proto_schema.encode_to_vec());
    Ok(())
}

/// A reading worker's rewrite handler: admits [`PartitionScanExec`] only under a verified grant,
/// everything [`carried`] names as before, and refuses anything else as [`StageReadsASource`].
pub(super) fn reads_partition_or_nothing(
    event: WorkerPlanRewriteEvent<'_>,
    keys: &ReadingKeys,
) -> Result<WorkerPlanRewriteEventResponse, DataFusionError> {
    // A clock before the epoch reads as past every grant's expiry: refused, never admitted.
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::MAX);
    let mut refusal: Option<DataFusionError> = None;
    event.plan.apply(|node| {
        if let Some(leaf) = node.downcast_ref::<PartitionScanExec>() {
            if let Err(cause) = grant::verify_reading(
                keys,
                &leaf.wire,
                &leaf.source,
                &leaf.partition,
                leaf.presented_key,
                leaf.grant.as_deref(),
                now,
            ) {
                refusal = Some(DataFusionError::External(Box::new(cause)));
                return Ok(TreeNodeRecursion::Stop);
            }
        } else if node.children().is_empty() && !carried(node) {
            refusal = Some(DataFusionError::External(Box::new(StageReadsASource {
                leaf: String::from(node.name()),
            })));
            return Ok(TreeNodeRecursion::Stop);
        }
        Ok(TreeNodeRecursion::Continue)
    })?;
    match refusal {
        Some(cause) => Err(cause),
        None => Ok(WorkerPlanRewriteEventResponse::new(event.plan)),
    }
}
