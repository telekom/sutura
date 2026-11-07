//! The leaf a reading task's plan carries: a granted read of one source's one partition
//! (`docs/adr/20261102082308-workers-read-sources.md` Decision 5), and the worker admit rule and
//! codec that carry and check it.
//!
//! A [`SplitScan`](crate::SplitScan) plans one `PartitionScanExec` per partition, placed on a node
//! reading its source; a served question builds one only for a granted lookup leg, which only a
//! `sutura serve` deployment's PostgreSQL source or `BigQuery` pool source grants. [`Nodes::worker_reading`](crate::Nodes::worker_reading) is a reading worker's own
//! construction, parallel to [`Nodes::worker`](crate::Nodes::worker); `crates/sutura-cli` builds one
//! for its reading role.
//!
//! # Limits
//!
//! - [`PartitionReader`](partition::PartitionReader) is a port: this crate holds no implementation,
//!   the composition root does. A reader that answers [`Unread::LegNotCertified`] fails the task
//!   under the relay code [`relay`] reads back; what it re-certified against is the reader's.
//! - The leaf's schema crosses the wire through `datafusion_proto`'s own `Schema` protobuf, the same
//!   round trip `combine::tests::across::decode` already exercises for one.
//! - A leaf carrying no grant, or one that fails to decode, reads the same as one wrongly named: the
//!   admit rule, not the codec, decides what a missing grant means
//!   ([`WorkerRefusal::GrantNotNaming`](super::WorkerRefusal::GrantNotNaming), widened for exactly this case).
//! - A leaf carrying a token, or a grant naming one, is admitted only when the grant names exactly
//!   the token and pool the leaf carries, which then carries the admitted grant's end to its
//!   reader ([`Forwarded::ends`](partition::Forwarded::ends)). The token leaves the leaf when its
//!   read starts, and a second read of that leaf is refused. The wire bytes it was decoded from are
//!   not wiped.

use core::fmt;
use core::future::Future;
use core::time::Duration;
use std::sync::Arc;
use std::time::SystemTime;

use datafusion::arrow::datatypes::{Schema, SchemaRef};
use datafusion::common::tree_node::{TreeNode as _, TreeNodeRecursion};
use datafusion::error::DataFusionError;
use datafusion::execution::{SendableRecordBatchStream, TaskContext};
use datafusion::physical_expr::EquivalenceProperties;
use datafusion::physical_plan::execution_plan::{Boundedness, EmissionType};
use datafusion::physical_plan::stream::RecordBatchStreamAdapter;
use datafusion::physical_plan::{DisplayAs, DisplayFormatType, ExecutionPlan, Partitioning, PlanProperties};
use datafusion_distributed::{WorkerPlanRewriteEvent, WorkerPlanRewriteEventResponse};
use datafusion_proto::physical_plan::{PhysicalExtensionCodec, PhysicalProtoConverterExtension};
use datafusion_proto::protobuf::Schema as ProtoSchema;
use futures_util::{StreamExt as _, stream};
use parking_lot::Mutex;
use prost::Message as _;
use ring::digest;
use sutura_domain::identity::{Secret, WorkloadPool};
use sutura_domain::model::SourceName;
use sutura_domain::warehouse::ResultBatches;

use super::grant::{self, ReadingKeys};
use super::{StageReadsASource, carried};

/// Reads one partition of one source for a reading task. `pub`, not `pub(super)`:
/// [`super::Nodes::worker_reading`] names it in a `pub fn` signature, so this trait cannot be more
/// private than that function.
pub trait PartitionReader: Send + Sync {
    /// Reads `source`'s `partition` of the leg `wire` carries, against `schema`.
    ///
    /// # Errors
    ///
    /// [`Unread::LegNotCertified`] when this node does not certify the leg `wire` carries;
    /// [`Unread::Failed`] for anything else.
    fn read(
        &self,
        wire: &[u8],
        source: &SourceName,
        partition: &[u8],
        schema: &SchemaRef,
    ) -> Result<SendableRecordBatchStream, Unread>;

    /// [`Self::read`], with `forwarded`, the caller's own token the partition's grant names.
    ///
    /// # Errors
    ///
    /// By default every such read, as [`Unread::Failed`]: a reader that reads no source with a
    /// caller's token is never handed one to read past.
    fn read_forwarded(
        &self,
        _wire: &[u8],
        source: &SourceName,
        _partition: &[u8],
        _schema: &SchemaRef,
        forwarded: Forwarded,
    ) -> Result<SendableRecordBatchStream, Unread> {
        drop(forwarded);
        Err(Unread::Failed(DataFusionError::Execution(format!(
            "this reading worker reads no partition of `{source}` with a caller's token"
        ))))
    }
}

/// The caller's own token a granted partition is read with, a digest of the pool whose principal
/// it is, and when the grant a worker admitted it under ends. Its `Debug` prints none of them.
pub struct Forwarded {
    token: Secret,
    pool: [u8; 32],
    ends: SystemTime,
}

impl Forwarded {
    /// `token`, a token of `pool`'s principal for the caller, admitted under no grant yet.
    #[must_use]
    pub fn new(token: Secret, pool: &WorkloadPool) -> Self {
        Self {
            token,
            pool: pool_digest(pool),
            ends: SystemTime::UNIX_EPOCH,
        }
    }

    /// When the grant this worker admitted the token under ends, on this worker's clock - no later
    /// than the token itself, as the planning node clamps a grant to its token. The epoch, already
    /// ended, for a token no admission named.
    #[must_use]
    pub const fn ends(&self) -> SystemTime {
        self.ends
    }

    /// How long the token's grant lasts from now, on this worker's clock: `None` once it has ended.
    #[must_use]
    pub fn left(&self) -> Option<Duration> {
        self.ends
            .duration_since(SystemTime::now())
            .ok()
            .filter(|left| !left.is_zero())
    }

    /// Whether this token was forwarded for `pool`'s principal, the one a reader's own source
    /// federates at.
    #[must_use]
    pub fn for_pool(&self, pool: &WorkloadPool) -> bool {
        self.pool == pool_digest(pool)
    }

    /// The token, moved out for the one read it is handed to.
    #[must_use]
    pub fn into_token(self) -> Secret {
        self.token
    }

    /// A copy for one more task's plan.
    pub(super) fn copied(&self) -> Self {
        Self {
            token: self.token.clone(),
            pool: self.pool,
            ends: self.ends,
        }
    }

    pub(super) const fn token(&self) -> &Secret {
        &self.token
    }

    pub(super) const fn pool(&self) -> [u8; 32] {
        self.pool
    }
}

impl fmt::Debug for Forwarded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Forwarded").finish_non_exhaustive()
    }
}

fn pool_digest(pool: &WorkloadPool) -> [u8; 32] {
    let hashed = digest::digest(&digest::SHA256, pool.audience().as_bytes());
    let mut bytes = [0; 32];
    bytes.copy_from_slice(hashed.as_ref());
    bytes
}

/// A closure reads without the leg's wire, and its failure is never a refused certification.
impl<F> PartitionReader for F
where
    F: Fn(&SourceName, &[u8], &SchemaRef) -> Result<SendableRecordBatchStream, DataFusionError> + Send + Sync,
{
    fn read(
        &self,
        _wire: &[u8],
        source: &SourceName,
        partition: &[u8],
        schema: &SchemaRef,
    ) -> Result<SendableRecordBatchStream, Unread> {
        self(source, partition, schema).map_err(Unread::Failed)
    }
}

/// Why a reading worker read nothing with a caller's token.
#[derive(Debug, thiserror::Error)]
pub enum TokenUnread {
    /// The grant the token was admitted under has ended on this node's clock, so the token may have.
    #[error("the grant a partition of `{at}` was admitted under has ended")]
    Ended { at: SourceName },
    /// A source of this kind reads no partition with a caller's token.
    #[error("this reading worker reads no partition of `{at}` with a caller's token")]
    NotReadWithAToken { at: SourceName },
    /// A source of this kind reads a partition only with the caller's token, and none was granted.
    #[error("a partition of `{at}` is read only with its caller's token, and its grant names none")]
    NoToken { at: SourceName },
}

/// Why a [`PartitionReader`] read nothing.
#[derive(Debug)]
pub enum Unread {
    /// This node's own catalog does not certify the leg the task carries. Crosses the link as a code
    /// the planning node maps to its own refusal, never as text.
    LegNotCertified,
    /// The partition's rows did not fit the bound this node reads one stream under. Crosses the link
    /// as a code, never as text.
    DidNotFit,
    /// The read failed, as the engine's failure.
    Failed(DataFusionError),
}

impl Unread {
    /// `cause` as the engine's failure.
    pub fn failed(cause: impl core::error::Error + Send + Sync + 'static) -> Self {
        Self::Failed(DataFusionError::External(Box::new(cause)))
    }

    /// A partition of `source`, which this node's reader does not read, as the engine's failure.
    #[must_use]
    pub fn source_not_read(source: &SourceName) -> Self {
        Self::Failed(DataFusionError::Execution(format!(
            "this reading worker does not read source `{source}`"
        )))
    }

    fn into_error(self) -> DataFusionError {
        match self {
            Self::LegNotCertified => super::relay::not_certified(DataFusionError::Execution(String::from(
                "this worker's catalog does not certify the leg it was asked to read",
            ))),
            Self::DidNotFit => super::relay::did_not_fit(DataFusionError::Execution(String::from(
                "a granted partition did not fit this worker's per-stream bound",
            ))),
            Self::Failed(cause) => cause,
        }
    }
}

/// The stream of a read that runs only once it is first polled - `read` is typically a blocking
/// read moved off the runtime - against `schema`. A failure is relayed as
/// [`PartitionScanExec::execute`] relays one.
pub fn read_later(
    schema: SchemaRef,
    read: impl Future<Output = Result<ResultBatches, Unread>> + Send + 'static,
) -> SendableRecordBatchStream {
    let batches = stream::once(read).flat_map(|read| {
        stream::iter(match read {
            Ok(read) => read.into_batches().into_iter().map(Ok).collect(),
            Err(unread) => vec![Err(unread.into_error())],
        })
    });
    Box::pin(RecordBatchStreamAdapter::new(schema, batches))
}

/// A reading worker's own reader, carried on its session so [`PartitionScanExec::execute`] can
/// reach it without the leaf itself holding one - the leaf crosses a codec, a reader does not.
pub(super) struct ReaderExtension(pub(super) Arc<dyn PartitionReader>);

/// A granted read of one source's one partition: the leg it reads, the partition descriptor, the
/// reading key this call presented, the grant and signature naming the read - or no grant, a case
/// the admit rule reads as [`WorkerRefusal::GrantNotNaming`](super::WorkerRefusal::GrantNotNaming) -
/// and the caller's token the grant names, if any, handed out once.
pub(super) struct PartitionScanExec {
    wire: Vec<u8>,
    source: SourceName,
    partition: Vec<u8>,
    presented_key: [u8; 32],
    grant: Option<Vec<u8>>,
    carried: Mutex<Carried>,
    properties: Arc<PlanProperties>,
}

/// The caller's token a scan carries. A worker keeps an installed plan for minutes after its read,
/// so the token leaves the scan when the read starts and is dropped with it if the read never does.
enum Carried {
    Nothing,
    Token(Forwarded),
    Taken,
}

impl Carried {
    const fn shown(&self) -> &'static str {
        match self {
            Self::Nothing => "none",
            Self::Token(_) => "held",
            Self::Taken => "taken",
        }
    }
}

impl fmt::Debug for PartitionScanExec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PartitionScanExec")
            .field("source", &self.source)
            .field("grant", &self.grant.is_some())
            .field("token", &self.shown())
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
            carried: Mutex::new(Carried::Nothing),
            properties,
        }
    }

    /// This scan, read with `forwarded`.
    pub(super) fn forwarding(self, forwarded: Forwarded) -> Self {
        *self.carried.lock() = Carried::Token(forwarded);
        self
    }

    /// Whether this scan holds a token, never the token. A scan whose token is being moved shows
    /// `busy` rather than waiting on it.
    fn shown(&self) -> &'static str {
        self.carried.try_lock().map_or("busy", |carried| carried.shown())
    }

    /// The token this scan carries, moved out for the one read it is handed to: `None` for a scan
    /// that carries none, and refused once it was taken.
    fn taken(&self) -> Result<Option<Forwarded>, DataFusionError> {
        let was = {
            let mut carried = self.carried.lock();
            let next = if matches!(*carried, Carried::Nothing) {
                Carried::Nothing
            } else {
                Carried::Taken
            };
            core::mem::replace(&mut *carried, next)
        };
        match was {
            Carried::Nothing => Ok(None),
            Carried::Token(forwarded) => Ok(Some(forwarded)),
            Carried::Taken => Err(DataFusionError::Execution(String::from(
                "a partition scan hands out its caller's token once, and this one's was taken",
            ))),
        }
    }
}

impl DisplayAs for PartitionScanExec {
    fn fmt_as(&self, _: DisplayFormatType, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PartitionScanExec: source={}, token={}", self.source, self.shown())
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
        let schema = self.properties.eq_properties.schema();
        self.taken()?
            .map_or_else(
                || reader.0.read(&self.wire, &self.source, &self.partition, schema),
                |forwarded| {
                    reader
                        .0
                        .read_forwarded(&self.wire, &self.source, &self.partition, schema, forwarded)
                },
            )
            .map_err(Unread::into_error)
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
    let token = take_field(&mut rest).ok_or_else(malformed)?;
    let pool = take_field(&mut rest).ok_or_else(malformed)?;
    let forwarded = match (token.is_empty(), <[u8; 32]>::try_from(pool.as_slice())) {
        (true, _) if pool.is_empty() => None,
        (false, Ok(pool)) => Some(Forwarded {
            token: Secret::new(String::from_utf8(token).map_err(|_utf8| malformed())?),
            pool,
            ends: SystemTime::UNIX_EPOCH,
        }),
        _ => return Err(malformed()),
    };
    let schema_bytes = take_field(&mut rest).ok_or_else(malformed)?;
    let proto_schema = ProtoSchema::decode(schema_bytes.as_slice()).map_err(|_proto| malformed())?;
    let schema: Schema = (&proto_schema).try_into().map_err(|_arrow| malformed())?;
    let scan = PartitionScanExec::new(wire, source, part, presented_key, grant, Arc::new(schema));
    Ok(Arc::new(match forwarded {
        Some(forwarded) => scan.forwarding(forwarded),
        None => scan,
    }))
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
    match &*leaf.carried.lock() {
        Carried::Token(forwarded) => {
            #[expect(
                clippy::disallowed_methods,
                reason = "the caller's token, written only into the plan of the one task that reads its partition"
            )]
            let token = forwarded.token.expose_secret().as_bytes();
            put_field(buf, token);
            put_field(buf, &forwarded.pool);
        }
        Carried::Nothing | Carried::Taken => {
            put_field(buf, &[]);
            put_field(buf, &[]);
        }
    }
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
            let mut carried = leaf.carried.lock();
            let forwarded = match &*carried {
                Carried::Token(forwarded) => Some(forwarded),
                Carried::Nothing | Carried::Taken => None,
            };
            let verified = grant::verify_reading(
                keys,
                &grant::ReadingCall {
                    wire: &leaf.wire,
                    source: &leaf.source,
                    partition: &leaf.partition,
                    key: leaf.presented_key,
                    grant: leaf.grant.as_deref(),
                    forwarded,
                },
                now,
            );
            match verified {
                Ok(ends) => {
                    if let Carried::Token(ref mut forwarded) = *carried {
                        forwarded.ends = SystemTime::UNIX_EPOCH.checked_add(ends).unwrap_or(SystemTime::UNIX_EPOCH);
                    }
                    drop(carried);
                }
                Err(cause) => {
                    drop(carried);
                    refusal = Some(DataFusionError::External(Box::new(cause)));
                    return Ok(TreeNodeRecursion::Stop);
                }
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
