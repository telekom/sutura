//! A reading worker's admit rule end to end (`super::super::super::partition`, Decision 5): a
//! granted read runs through [`Nodes::worker_reading`] and its codec exactly as a plain worker's
//! stage does, and each way a call fails to name the read it asks for is refused before any reader
//! is asked to read anything.
//!
//! # What this does not exercise
//!
//! No caller outside this module's own tests mints a grant or plans [`PartitionScanExec`]
//! (`partition.rs`'s own module doc states that limit). A refused re-certification is read back off
//! an in-process worker's stream, so the protobuf a real link carries it in is not exercised here.

use core::time::Duration;
use std::sync::Arc;

use datafusion::arrow::array::Int64Array;
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::error::DataFusionError;
use datafusion::execution::SendableRecordBatchStream;
use datafusion::physical_plan::ExecutionPlan;
use datafusion::physical_plan::stream::RecordBatchStreamAdapter;
use datafusion::prelude::{CsvReadOptions, SessionContext};
use datafusion_distributed::{ExecuteTaskRequest, MaybeEncoded, ProducerHead, SetPlanRequest, TaskKey};
use futures_util::{StreamExt as _, stream};
use sutura_domain::model::SourceName;
use tonic::codegen::http::HeaderMap;
use url::Url;

use sutura_domain::plan::FederationCombiner as _;

use super::super::super::partition::{PartitionReader, PartitionScanExec, Unread, read_later};
use super::super::super::{NodeIdentity, Nodes, StageReadsASource, WorkerRefusal, relayed};
use super::super::reading::{Expiry, LegDigest, MaskedCaller, Partition, ReadingGrant, ReadingKey, StreamBound, encode, mint};
use crate::combine::{CombineError, DataFusionCombiner};
use crate::pool::WorkingSet;

/// A node signed for 127.0.0.1 by an authority of its own: no cell here dials a link. The one shared
/// definition every localhost test tree under `combine` calls, rather than each building its own.
fn identity() -> NodeIdentity {
    NodeIdentity::signed_for_localhost()
}

fn roomy() -> WorkingSet {
    WorkingSet::of_bytes(core::num::NonZeroUsize::new(1 << 30).expect("a nonzero size"))
}

fn servers() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("a server runtime builds")
}

fn schema() -> SchemaRef {
    Arc::new(Schema::new(vec![Field::new("n", DataType::Int64, false)]))
}

fn source() -> SourceName {
    SourceName::parse("orders").expect("a test source is a source")
}

/// A grant naming `wire`/`source()`/`partition`, far enough in the future that the worker's own
/// wall clock never finds it expired, signed under `key` and encoded as a leaf carries it.
fn granted(key: &ReadingKey, wire: &[u8], partition: &[u8]) -> Vec<u8> {
    let built = ReadingGrant::new(
        LegDigest::of(wire),
        source(),
        Partition::of(partition.to_vec()),
        Expiry::at(Duration::from_secs(4_000_000_000)),
        MaskedCaller::of_masked(String::from("masked")),
        StreamBound {
            rows: 1000,
            bytes: 1 << 20,
        },
    );
    let (built, signature) = mint(key, built);
    encode(&built, &signature)
}

/// A reading worker's own address, declared with no other node.
fn declared(
    worker: datafusion_distributed::Worker,
) -> (datafusion_distributed::Worker, datafusion::prelude::SessionContext, Url) {
    let at = Url::parse("https://127.0.0.1:1").expect("an address is a URL");
    let planning = Nodes::declared(worker.clone(), at.clone(), [], identity())
        .session(roomy())
        .expect("a session builds");
    (worker, planning, at)
}

fn task_key(query_id: &str) -> TaskKey {
    TaskKey {
        query_id: query_id.parse().expect("a uuid"),
        stage_id: 1,
        task_number: 0,
    }
}

#[test]
fn a_granted_partition_read_runs_end_to_end_through_in_process_workers() {
    let wire = b"wire-granted".to_vec();
    let partition = b"partition-granted".to_vec();
    let key_bytes = [7_u8; 32];
    let grant = granted(&ReadingKey::of(key_bytes), &wire, &partition);
    let batch = RecordBatch::try_new(schema(), vec![Arc::new(Int64Array::from(vec![1_i64, 2, 3]))]).expect("a batch builds");
    let reader: Arc<dyn PartitionReader> = Arc::new(move |_: &SourceName, _: &[u8], schema: &SchemaRef| {
        let batch = batch.clone();
        Ok(Box::pin(RecordBatchStreamAdapter::new(
            Arc::clone(schema),
            stream::iter(vec![Ok::<_, DataFusionError>(batch)]),
        )) as SendableRecordBatchStream)
    });
    let worker = Nodes::worker_reading(roomy(), identity(), Some(reader)).expect("a reading worker builds");
    let (worker, planning, at) = declared(worker);
    let leaf = Arc::new(PartitionScanExec::new(
        wire,
        source(),
        partition,
        key_bytes,
        Some(grant),
        schema(),
    )) as Arc<dyn ExecutionPlan>;
    let task = task_key("11111111-1111-1111-1111-111111111111");

    let collected = servers().block_on(async {
        let encoded = MaybeEncoded::Decoded(leaf)
            .encode(&planning.task_ctx())
            .expect("a leaf encodes");
        drop(
            worker
                .coordinator_channel(
                    HeaderMap::new(),
                    SetPlanRequest {
                        task_key: task,
                        task_count: 1,
                        plan: MaybeEncoded::Encoded(encoded),
                        work_unit_feed_declarations: vec![],
                        target_worker_url: at,
                        query_start_time_ns: 0,
                    },
                    Box::pin(stream::pending()),
                )
                .await
                .expect("a granted read is installed"),
        );
        let (streams, _) = worker
            .execute_task(ExecuteTaskRequest {
                task_key: task,
                target_partition_start: 0,
                target_partition_end: 1,
                producer_head: ProducerHead::None,
            })
            .await
            .expect("an installed task runs");
        stream::iter(streams)
            .flatten()
            .map(|batch| batch.map(|batch| batch.num_rows()))
            .collect::<Vec<_>>()
            .await
    });

    let rows: Vec<usize> = collected
        .into_iter()
        .map(|batch| batch.expect("a granted read runs"))
        .collect();
    assert_eq!(rows, vec![3]);
}

#[test]
fn a_reading_worker_refuses_an_ungranted_source_leaf() {
    let worker = Nodes::worker_reading(roomy(), identity(), None).expect("a reading worker builds");
    let path = std::env::temp_dir().join(format!("sutura-reading-worker-source-{}.csv", std::process::id()));
    std::fs::write(&path, "region\nnorth\n").expect("a temporary file writes");
    let refused = servers().block_on(async {
        let scan = SessionContext::new()
            .read_csv(path.to_str().expect("a temporary path is text"), CsvReadOptions::new())
            .await
            .expect("a file reads")
            .create_physical_plan()
            .await
            .expect("a file scan plans");
        worker
            .coordinator_channel(
                HeaderMap::new(),
                SetPlanRequest {
                    task_key: task_key("22222222-2222-2222-2222-222222222222"),
                    task_count: 1,
                    plan: MaybeEncoded::Decoded(scan),
                    work_unit_feed_declarations: vec![],
                    target_worker_url: Url::parse("https://127.0.0.1:1").expect("an address is a URL"),
                    query_start_time_ns: 0,
                },
                Box::pin(stream::pending()),
            )
            .await
            .err()
    });
    std::fs::remove_file(&path).expect("a temporary file removes");
    let refused = refused.expect("a reading worker ran a stage that reads a file no grant names");
    assert!(
        matches!(
            refused.find_root(),
            DataFusionError::External(cause)
                if cause.downcast_ref::<StageReadsASource>().is_some_and(|refusal| refusal.leaf == "DataSourceExec")
        ),
        "{refused}"
    );
}

#[test]
fn a_reading_worker_refuses_a_grant_whose_leg_digest_is_not_the_wires() {
    let signed_wire = b"wire-signed-over".to_vec();
    let sent_wire = b"wire-actually-sent".to_vec();
    let partition = b"partition-digest-mismatch".to_vec();
    let key_bytes = [9_u8; 32];
    let grant = granted(&ReadingKey::of(key_bytes), &signed_wire, &partition);
    let worker = Nodes::worker_reading(roomy(), identity(), None).expect("a reading worker builds");
    let (worker, planning, at) = declared(worker);
    let leaf = Arc::new(PartitionScanExec::new(
        sent_wire,
        source(),
        partition,
        key_bytes,
        Some(grant),
        schema(),
    )) as Arc<dyn ExecutionPlan>;
    let task = task_key("33333333-3333-3333-3333-333333333333");

    let refused = servers().block_on(async {
        let encoded = MaybeEncoded::Decoded(leaf)
            .encode(&planning.task_ctx())
            .expect("a leaf encodes");
        worker
            .coordinator_channel(
                HeaderMap::new(),
                SetPlanRequest {
                    task_key: task,
                    task_count: 1,
                    plan: MaybeEncoded::Encoded(encoded),
                    work_unit_feed_declarations: vec![],
                    target_worker_url: at,
                    query_start_time_ns: 0,
                },
                Box::pin(stream::pending()),
            )
            .await
            .err()
    });

    let refused = refused.expect("a reading worker ran a leaf whose grant names a different leg");
    assert!(
        matches!(
            refused.find_root(),
            DataFusionError::External(cause) if cause.downcast_ref::<WorkerRefusal>() == Some(&WorkerRefusal::GrantNotNaming)
        ),
        "{refused}"
    );
}

#[test]
fn a_reading_worker_refuses_a_leaf_carrying_no_grant() {
    let wire = b"wire-no-grant".to_vec();
    let partition = b"partition-no-grant".to_vec();
    let key_bytes = [1_u8; 32];
    let worker = Nodes::worker_reading(roomy(), identity(), None).expect("a reading worker builds");
    let (worker, planning, at) = declared(worker);
    let leaf = Arc::new(PartitionScanExec::new(wire, source(), partition, key_bytes, None, schema())) as Arc<dyn ExecutionPlan>;
    let task = task_key("44444444-4444-4444-4444-444444444444");

    let refused = servers().block_on(async {
        let encoded = MaybeEncoded::Decoded(leaf)
            .encode(&planning.task_ctx())
            .expect("a leaf encodes");
        worker
            .coordinator_channel(
                HeaderMap::new(),
                SetPlanRequest {
                    task_key: task,
                    task_count: 1,
                    plan: MaybeEncoded::Encoded(encoded),
                    work_unit_feed_declarations: vec![],
                    target_worker_url: at,
                    query_start_time_ns: 0,
                },
                Box::pin(stream::pending()),
            )
            .await
            .err()
    });

    let refused = refused.expect("a reading worker ran a leaf with no grant");
    assert!(
        matches!(
            refused.find_root(),
            DataFusionError::External(cause) if cause.downcast_ref::<WorkerRefusal>() == Some(&WorkerRefusal::GrantNotNaming)
        ),
        "{refused}"
    );
}

#[test]
fn a_second_reading_key_presented_for_the_same_task_is_refused() {
    let wire = b"wire-same-task".to_vec();
    let partition = b"partition-same-task".to_vec();
    let (first_bytes, second_bytes) = ([2_u8; 32], [3_u8; 32]);
    let first_grant = granted(&ReadingKey::of(first_bytes), &wire, &partition);
    let second_grant = granted(&ReadingKey::of(second_bytes), &wire, &partition);
    let worker = Nodes::worker_reading(roomy(), identity(), None).expect("a reading worker builds");
    let (worker, planning, at) = declared(worker);
    let first_leaf = Arc::new(PartitionScanExec::new(
        wire.clone(),
        source(),
        partition.clone(),
        first_bytes,
        Some(first_grant),
        schema(),
    )) as Arc<dyn ExecutionPlan>;
    let second_leaf = Arc::new(PartitionScanExec::new(
        wire,
        source(),
        partition,
        second_bytes,
        Some(second_grant),
        schema(),
    )) as Arc<dyn ExecutionPlan>;
    let first_task = task_key("55555555-5555-5555-5555-555555555555");
    let second_task = task_key("66666666-6666-6666-6666-666666666666");

    let (first, second) = servers().block_on(async {
        let install = async |leaf: Arc<dyn ExecutionPlan>, task_key: TaskKey| {
            let encoded = MaybeEncoded::Decoded(leaf)
                .encode(&planning.task_ctx())
                .expect("a leaf encodes");
            worker
                .coordinator_channel(
                    HeaderMap::new(),
                    SetPlanRequest {
                        task_key,
                        task_count: 1,
                        plan: MaybeEncoded::Encoded(encoded),
                        work_unit_feed_declarations: vec![],
                        target_worker_url: at.clone(),
                        query_start_time_ns: 0,
                    },
                    Box::pin(stream::pending()),
                )
                .await
                .err()
        };
        (install(first_leaf, first_task).await, install(second_leaf, second_task).await)
    });

    assert!(
        first.is_none(),
        "the first key presented for a reading task is not refused: {first:?}"
    );
    let second = second.expect("a second, differently-keyed grant for the same reading task was admitted");
    assert!(
        matches!(
            second.find_root(),
            DataFusionError::External(cause) if cause.downcast_ref::<WorkerRefusal>() == Some(&WorkerRefusal::GrantNotNaming)
        ),
        "{second}"
    );
}

/// A reader whose node certifies no leg, refusing as the composition root's reader does: inside the
/// stream, once it is first polled.
struct CertifiesNothing;

impl PartitionReader for CertifiesNothing {
    fn read(
        &self,
        _wire: &[u8],
        _source: &SourceName,
        _partition: &[u8],
        schema: &SchemaRef,
    ) -> Result<SendableRecordBatchStream, Unread> {
        Ok(read_later(Arc::clone(schema), async { Err(Unread::LegNotCertified) }))
    }
}

#[test]
fn a_reading_worker_refusing_to_certify_its_leg_is_read_back_as_the_planning_nodes_own_refusal() {
    let wire = b"wire-not-certified".to_vec();
    let partition = b"partition-not-certified".to_vec();
    let key_bytes = [8_u8; 32];
    let grant = granted(&ReadingKey::of(key_bytes), &wire, &partition);
    let reader: Arc<dyn PartitionReader> = Arc::new(CertifiesNothing);
    let worker = Nodes::worker_reading(roomy(), identity(), Some(reader)).expect("a reading worker builds");
    let (worker, planning, at) = declared(worker);
    let leaf = Arc::new(PartitionScanExec::new(
        wire,
        source(),
        partition,
        key_bytes,
        Some(grant),
        schema(),
    )) as Arc<dyn ExecutionPlan>;
    let task = task_key("77777777-7777-7777-7777-777777777777");

    let failures = servers().block_on(async {
        let encoded = MaybeEncoded::Decoded(leaf)
            .encode(&planning.task_ctx())
            .expect("a leaf encodes");
        drop(
            worker
                .coordinator_channel(
                    HeaderMap::new(),
                    SetPlanRequest {
                        task_key: task,
                        task_count: 1,
                        plan: MaybeEncoded::Encoded(encoded),
                        work_unit_feed_declarations: vec![],
                        target_worker_url: at,
                        query_start_time_ns: 0,
                    },
                    Box::pin(stream::pending()),
                )
                .await
                .expect("a granted read is installed"),
        );
        let (streams, _) = worker
            .execute_task(ExecuteTaskRequest {
                task_key: task,
                target_partition_start: 0,
                target_partition_end: 1,
                producer_head: ProducerHead::None,
            })
            .await
            .expect("an installed task runs");
        stream::iter(streams)
            .flatten()
            .filter_map(|batch| async move { batch.err() })
            .collect::<Vec<_>>()
            .await
    });

    let cause = failures
        .into_iter()
        .next()
        .expect("a read whose leg this worker does not certify answered rows");
    let refused = relayed(CombineError::Execute { cause });
    assert!(matches!(refused, CombineError::LegNotCertified), "{refused:?}");
    assert!(
        DataFusionCombiner::new()
            .expect("a combiner builds")
            .leg_not_certified(&refused)
    );
}

use super::super::super::partition::Forwarded;

fn pool() -> sutura_domain::identity::WorkloadPool {
    sutura_domain::identity::WorkloadPool::parse(
        "//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/p/providers/q",
    )
    .expect("a test pool is a pool")
}

fn forwarded(token: &str) -> Forwarded {
    Forwarded::new(sutura_domain::identity::Secret::new(token), &pool())
}

/// [`granted`], naming `token` as the token its partition is read with.
fn granted_forwarding(key: &ReadingKey, wire: &[u8], partition: &[u8], token: &Forwarded) -> Vec<u8> {
    let built = ReadingGrant::new(
        LegDigest::of(wire),
        source(),
        Partition::of(partition.to_vec()),
        Expiry::at(Duration::from_secs(4_000_000_000)),
        MaskedCaller::of_masked(String::from("masked")),
        StreamBound {
            rows: 1000,
            bytes: 1 << 20,
        },
    )
    .forwarding(token);
    let (built, signature) = mint(key, built);
    encode(&built, &signature)
}

/// Each token a [`Forwards`] reader was handed, and whether it named [`pool`].
type Handed = Arc<parking_lot::Mutex<Vec<(String, bool)>>>;

/// A reader that reads only with a token, each one it was handed logged in its [`Handed`].
struct Forwards(Handed);

impl PartitionReader for Forwards {
    fn read(
        &self,
        _wire: &[u8],
        _source: &SourceName,
        _partition: &[u8],
        _schema: &SchemaRef,
    ) -> Result<SendableRecordBatchStream, Unread> {
        Err(Unread::Failed(DataFusionError::Execution(String::from("read with no token"))))
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "a test reader logs the token it was handed, so a cell can compare it with the one granted"
    )]
    fn read_forwarded(
        &self,
        _wire: &[u8],
        _source: &SourceName,
        _partition: &[u8],
        schema: &SchemaRef,
        forwarded: Forwarded,
    ) -> Result<SendableRecordBatchStream, Unread> {
        let for_pool = forwarded.for_pool(&pool());
        self.0
            .lock()
            .push((String::from(forwarded.into_token().expose_secret()), for_pool));
        Ok(Box::pin(RecordBatchStreamAdapter::new(
            Arc::clone(schema),
            stream::empty::<Result<RecordBatch, DataFusionError>>(),
        )))
    }
}

/// `leaf` installed as task `task` on a reading worker reading with `reader`, and every failure
/// its install, its one task's execution or that task's stream answered.
fn installed_and_run(reader: Option<Arc<dyn PartitionReader>>, leaf: PartitionScanExec, task: &str) -> Vec<DataFusionError> {
    let worker = Nodes::worker_reading(roomy(), identity(), reader).expect("a reading worker builds");
    let (worker, planning, at) = declared(worker);
    let leaf = Arc::new(leaf) as Arc<dyn ExecutionPlan>;
    let task = task_key(task);
    servers().block_on(async {
        let encoded = MaybeEncoded::Decoded(leaf)
            .encode(&planning.task_ctx())
            .expect("a leaf encodes");
        let installed = worker
            .coordinator_channel(
                HeaderMap::new(),
                SetPlanRequest {
                    task_key: task,
                    task_count: 1,
                    plan: MaybeEncoded::Encoded(encoded),
                    work_unit_feed_declarations: vec![],
                    target_worker_url: at,
                    query_start_time_ns: 0,
                },
                Box::pin(stream::pending()),
            )
            .await;
        if let Err(refused) = installed {
            return vec![refused];
        }
        let executed = worker
            .execute_task(ExecuteTaskRequest {
                task_key: task,
                target_partition_start: 0,
                target_partition_end: 1,
                producer_head: ProducerHead::None,
            })
            .await;
        let streams = match executed {
            Ok((streams, _)) => streams,
            Err(failed) => return vec![failed],
        };
        stream::iter(streams)
            .flatten()
            .filter_map(|batch| async move { batch.err() })
            .collect::<Vec<_>>()
            .await
    })
}

fn refused_as_not_naming(failures: &[DataFusionError]) -> bool {
    matches!(
        failures.first().map(DataFusionError::find_root),
        Some(DataFusionError::External(cause)) if cause.downcast_ref::<WorkerRefusal>() == Some(&WorkerRefusal::GrantNotNaming)
    )
}

#[test]
fn a_granted_partition_read_hands_its_reader_the_token_and_pool_its_grant_names() {
    let (wire, partition, key_bytes) = (b"wire-forwarded".to_vec(), b"partition-forwarded".to_vec(), [10_u8; 32]);
    let grant = granted_forwarding(&ReadingKey::of(key_bytes), &wire, &partition, &forwarded("token-a"));
    let handed = Handed::default();
    let leaf =
        PartitionScanExec::new(wire, source(), partition, key_bytes, Some(grant), schema()).forwarding(forwarded("token-a"));

    let failures = installed_and_run(
        Some(Arc::new(Forwards(Arc::clone(&handed)))),
        leaf,
        "88888888-8888-8888-8888-888888888888",
    );

    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(*handed.lock(), vec![(String::from("token-a"), true)]);
}

#[test]
fn a_partition_read_carrying_a_token_its_grant_does_not_name_is_refused() {
    let (wire, partition, key_bytes) = (b"wire-swapped".to_vec(), b"partition-swapped".to_vec(), [11_u8; 32]);
    let grant = granted_forwarding(&ReadingKey::of(key_bytes), &wire, &partition, &forwarded("token-a"));
    let handed = Handed::default();
    let leaf =
        PartitionScanExec::new(wire, source(), partition, key_bytes, Some(grant), schema()).forwarding(forwarded("token-b"));

    let failures = installed_and_run(
        Some(Arc::new(Forwards(Arc::clone(&handed)))),
        leaf,
        "99999999-9999-9999-9999-999999999999",
    );

    assert!(refused_as_not_naming(&failures), "{failures:?}");
    assert!(
        handed.lock().is_empty(),
        "a reader was handed a token its grant does not name"
    );
}

#[test]
fn a_partition_read_carrying_no_token_where_its_grant_names_one_is_refused() {
    let (wire, partition, key_bytes) = (b"wire-dropped".to_vec(), b"partition-dropped".to_vec(), [12_u8; 32]);
    let grant = granted_forwarding(&ReadingKey::of(key_bytes), &wire, &partition, &forwarded("token-a"));
    let leaf = PartitionScanExec::new(wire, source(), partition, key_bytes, Some(grant), schema());

    let failures = installed_and_run(None, leaf, "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");

    assert!(refused_as_not_naming(&failures), "{failures:?}");
}

#[test]
fn a_partition_read_carrying_a_token_where_its_grant_names_none_is_refused() {
    let (wire, partition, key_bytes) = (b"wire-added".to_vec(), b"partition-added".to_vec(), [13_u8; 32]);
    let grant = granted(&ReadingKey::of(key_bytes), &wire, &partition);
    let handed = Handed::default();
    let leaf =
        PartitionScanExec::new(wire, source(), partition, key_bytes, Some(grant), schema()).forwarding(forwarded("token-a"));

    let failures = installed_and_run(
        Some(Arc::new(Forwards(Arc::clone(&handed)))),
        leaf,
        "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
    );

    assert!(refused_as_not_naming(&failures), "{failures:?}");
    assert!(handed.lock().is_empty(), "a reader was handed a token no grant names");
}

#[test]
fn a_reader_that_reads_with_no_token_refuses_a_partition_granted_with_one() {
    let (wire, partition, key_bytes) = (b"wire-unforwarded".to_vec(), b"partition-unforwarded".to_vec(), [14_u8; 32]);
    let grant = granted_forwarding(&ReadingKey::of(key_bytes), &wire, &partition, &forwarded("token-a"));
    let reader: Arc<dyn PartitionReader> = Arc::new(|_: &SourceName, _: &[u8], schema: &SchemaRef| {
        Ok(Box::pin(RecordBatchStreamAdapter::new(
            Arc::clone(schema),
            stream::empty::<Result<RecordBatch, DataFusionError>>(),
        )) as SendableRecordBatchStream)
    });
    let leaf =
        PartitionScanExec::new(wire, source(), partition, key_bytes, Some(grant), schema()).forwarding(forwarded("token-a"));

    let failures = installed_and_run(Some(reader), leaf, "cccccccc-cccc-cccc-cccc-cccccccccccc");

    assert!(
        failures.iter().any(|failure| failure
            .to_string()
            .contains("reads no partition of `orders` with a caller's token")),
        "{failures:?}"
    );
}

/// A [`Forwards`] reader whose read with no token answers no rows, each such read counted.
struct Either {
    forwards: Forwards,
    unforwarded: Arc<std::sync::atomic::AtomicUsize>,
}

impl PartitionReader for Either {
    fn read(
        &self,
        _wire: &[u8],
        _source: &SourceName,
        _partition: &[u8],
        schema: &SchemaRef,
    ) -> Result<SendableRecordBatchStream, Unread> {
        self.unforwarded.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(Box::pin(RecordBatchStreamAdapter::new(
            Arc::clone(schema),
            stream::empty::<Result<RecordBatch, DataFusionError>>(),
        )))
    }

    fn read_forwarded(
        &self,
        wire: &[u8],
        source: &SourceName,
        partition: &[u8],
        schema: &SchemaRef,
        forwarded: Forwarded,
    ) -> Result<SendableRecordBatchStream, Unread> {
        self.forwards.read_forwarded(wire, source, partition, schema, forwarded)
    }
}

/// Runs the real worker admission before a test executes a leaf directly.
fn admitted(leaf: PartitionScanExec) -> Result<Arc<PartitionScanExec>, DataFusionError> {
    let leaf = Arc::new(leaf);
    let plan = Arc::clone(&leaf) as Arc<dyn ExecutionPlan>;
    super::super::super::partition::reads_partition_or_nothing(
        datafusion_distributed::WorkerPlanRewriteEvent {
            plan,
            session_config: &datafusion::prelude::SessionConfig::new(),
        },
        &super::super::ReadingKeys::default(),
    )?;
    Ok(leaf)
}

/// `leaf` executed `times` times on this node under `reader`, and what each answered.
fn executed(leaf: &PartitionScanExec, reader: Arc<dyn PartitionReader>, times: usize) -> Vec<Result<(), DataFusionError>> {
    let config = datafusion::prelude::SessionConfig::new()
        .with_extension(Arc::new(super::super::super::partition::ReaderExtension(reader)));
    let context = SessionContext::new_with_config(config).task_ctx();
    core::iter::repeat_with(|| leaf.execute(0, Arc::clone(&context)).map(drop))
        .take(times)
        .collect()
}

#[test]
fn a_partition_scan_hands_out_its_token_once() {
    let handed = Handed::default();
    let (wire, partition, key_bytes) = (b"wire-once".to_vec(), b"partition-once".to_vec(), [15_u8; 32]);
    let grant = granted_forwarding(&ReadingKey::of(key_bytes), &wire, &partition, &forwarded("token-a"));
    let leaf = admitted(
        PartitionScanExec::new(wire, source(), partition, key_bytes, Some(grant), schema()).forwarding(forwarded("token-a")),
    )
    .expect("the signed token read is admitted");

    let unforwarded = Arc::default();
    let reader = Either {
        forwards: Forwards(Arc::clone(&handed)),
        unforwarded: Arc::clone(&unforwarded),
    };

    let answered = executed(&leaf, Arc::new(reader), 2);

    assert_eq!(*handed.lock(), vec![(String::from("token-a"), true)]);
    assert!(answered.first().is_some_and(Result::is_ok), "{answered:?}");
    assert!(
        matches!(
            answered.get(1),
            Some(Err(DataFusionError::Execution(refused))) if refused.contains("hands out its caller's token once")
        ),
        "a second read of one scan was not refused for its token being taken: {answered:?}"
    );
    assert_eq!(
        unforwarded.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "a second read of one scan read with no token"
    );
}

#[test]
fn a_partition_scan_shows_whether_it_holds_a_token_and_never_the_token() {
    let handed = Handed::default();
    let (wire, partition, key_bytes) = (b"wire-shown".to_vec(), b"partition-shown".to_vec(), [16_u8; 32]);
    let grant = granted_forwarding(&ReadingKey::of(key_bytes), &wire, &partition, &forwarded("token-a"));
    let leaf = admitted(
        PartitionScanExec::new(wire, source(), partition, key_bytes, Some(grant), schema()).forwarding(forwarded("token-a")),
    )
    .expect("the signed token read is admitted");
    let shown = |leaf: &PartitionScanExec| {
        (
            format!("{leaf:?}"),
            datafusion::physical_plan::displayable(leaf).one_line().to_string(),
        )
    };

    let held = shown(&leaf);
    drop(executed(&leaf, Arc::new(Forwards(Arc::clone(&handed))), 1));
    let taken = shown(&leaf);

    assert!(
        held.0.contains(r#"token: "held""#) && held.1.contains("token=held"),
        "{held:?}"
    );
    assert!(
        taken.0.contains(r#"token: "taken""#) && taken.1.contains("token=taken"),
        "{taken:?}"
    );
    assert!(
        [&held.0, &held.1, &taken.0, &taken.1]
            .iter()
            .all(|line| !line.contains("token-a")),
        "{held:?} {taken:?}"
    );
}

#[test]
fn a_partition_read_carrying_a_pool_its_grant_does_not_name_is_refused() {
    let (wire, partition, key_bytes) = (b"wire-repooled".to_vec(), b"partition-repooled".to_vec(), [17_u8; 32]);
    let grant = granted_forwarding(&ReadingKey::of(key_bytes), &wire, &partition, &forwarded("token-a"));
    let elsewhere = sutura_domain::identity::WorkloadPool::parse(
        "//iam.googleapis.com/projects/2/locations/global/workloadIdentityPools/p/providers/q",
    )
    .expect("a test pool is a pool");
    let handed = Handed::default();
    let leaf = PartitionScanExec::new(wire, source(), partition, key_bytes, Some(grant), schema())
        .forwarding(Forwarded::new(sutura_domain::identity::Secret::new("token-a"), &elsewhere));

    let failures = installed_and_run(
        Some(Arc::new(Forwards(Arc::clone(&handed)))),
        leaf,
        "dddddddd-dddd-dddd-dddd-dddddddddddd",
    );

    assert!(refused_as_not_naming(&failures), "{failures:?}");
    assert!(
        handed.lock().is_empty(),
        "a reader was handed a token for a pool its grant does not name"
    );
}

#[test]
fn a_partition_read_carrying_no_principal_where_its_grant_names_one_is_refused() {
    let (wire, partition, key_bytes) = (b"wire-unswitched".to_vec(), b"partition-unswitched".to_vec(), [20_u8; 32]);
    let grant = switched::granted_switching(
        &ReadingKey::of(key_bytes),
        &wire,
        &partition,
        "user@example.com",
        "user@example.com",
    );
    let leaf = PartitionScanExec::new(wire, source(), partition, key_bytes, Some(grant), schema());

    let failures = installed_and_run(None, leaf, "12121212-1212-1212-1212-121212121212");

    assert!(refused_as_not_naming(&failures), "{failures:?}");
}

mod bounds;
mod switched;
