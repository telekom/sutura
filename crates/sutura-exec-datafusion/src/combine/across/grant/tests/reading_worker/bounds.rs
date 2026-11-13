//! Authenticated bounds at the consumer, after a fake source has allocated its batches.

use core::mem::size_of;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use datafusion::arrow::array::Int64Array;
use datafusion::arrow::datatypes::SchemaRef;
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::error::DataFusionError;
use datafusion::execution::SendableRecordBatchStream;
use datafusion::physical_plan::ExecutionPlan as _;
use datafusion::physical_plan::execution_plan::EmissionType;
use datafusion::physical_plan::stream::RecordBatchStreamAdapter;
use datafusion::prelude::{SessionConfig, SessionContext};
use futures_util::{StreamExt as _, TryStreamExt as _, stream};
use sutura_domain::model::SourceName;
use sutura_domain::warehouse::Value;

use super::super::super::super::partition::{PartitionReader, PartitionScanExec, ReaderExtension};
use super::{
    CombineError, Duration, Expiry, LegDigest, MaskedCaller, Partition, ReadingGrant, ReadingKey, StreamBound, WorkerRefusal,
    admitted, encode, executed, mint, relayed, schema, servers, source,
};

fn leaf(rows: u32, bytes: u32) -> PartitionScanExec {
    let (wire, partition, key_bytes) = (b"wire-bound", b"partition-bound", [26_u8; 32]);
    let grant = ReadingGrant::new(
        LegDigest::of(wire),
        source(),
        Partition::of(partition.to_vec()),
        Expiry::at(Duration::from_secs(4_000_000_000)),
        MaskedCaller::of_masked(String::from("masked")),
        (sutura_domain::identity::VerifierId::primary(), [0; 32]),
        StreamBound { rows, bytes },
    );
    let (grant, signature) = mint(&ReadingKey::of(key_bytes), grant);
    PartitionScanExec::new(
        wire.to_vec(),
        source(),
        partition.to_vec(),
        key_bytes,
        Some(encode(&grant, &signature)),
        schema(),
    )
}

fn batch() -> RecordBatch {
    RecordBatch::try_new(schema(), vec![Arc::new(Int64Array::from(vec![1_i64, 2]))]).expect("a batch builds")
}

/// The existing domain budget charges buffers twice and the converted row structure. This sizes
/// the fixture's boundary; these cells exercise the grant consumer, not that accounting formula.
fn two_batches_cost() -> u32 {
    let batch = batch();
    let one = batch.get_array_memory_size() * 2 + batch.num_rows() * (size_of::<Vec<Value>>() + size_of::<Value>());
    u32::try_from(one * 2).expect("two small batches fit a wire budget")
}

fn opened(leaf: &PartitionScanExec, fails: bool) -> SendableRecordBatchStream {
    let reader: Arc<dyn PartitionReader> = Arc::new(move |_: &SourceName, _: &[u8], schema: &SchemaRef| {
        let mut batches = vec![Ok(batch()), Ok(batch())];
        if fails {
            batches.push(Err(DataFusionError::Execution(String::from("source failed"))));
        }
        Ok(Box::pin(RecordBatchStreamAdapter::new(Arc::clone(schema), stream::iter(batches))) as SendableRecordBatchStream)
    });
    let config = SessionConfig::new().with_extension(Arc::new(ReaderExtension(reader)));
    leaf.execute(0, SessionContext::new_with_config(config).task_ctx())
        .expect("an admitted scan opens")
}

#[test]
fn a_reading_grant_bounds_cumulative_rows_before_emitting_any_batch() {
    servers().block_on(async {
        let exact = admitted(leaf(4, 1 << 20)).expect("a positive bound is admitted");
        let batches: Vec<_> = opened(&exact, false)
            .try_collect()
            .await
            .expect("the exact row boundary fits");
        assert_eq!(batches.iter().map(RecordBatch::num_rows).collect::<Vec<_>>(), vec![2, 2]);

        let smaller = admitted(leaf(3, 1 << 20)).expect("a smaller signed row bound is admitted");
        let mut batches = opened(&smaller, false);
        let cause = batches
            .next()
            .await
            .expect("an over-bound stream answers a refusal")
            .expect_err("no accepted prefix may precede a cumulative row refusal");
        let refused = relayed(CombineError::Execute { cause });
        assert!(matches!(refused, CombineError::PartitionDidNotFit), "{refused:?}");
        assert!(batches.next().await.is_none(), "a refused partition emitted another item");
        assert!(matches!(exact.properties().emission_type, EmissionType::Final));
    });
}

#[test]
fn a_reading_grant_bounds_cumulative_bytes_before_emitting_any_batch() {
    servers().block_on(async {
        let bytes = two_batches_cost();
        let exact = admitted(leaf(1000, bytes)).expect("a positive byte bound is admitted");
        let batches: Vec<_> = opened(&exact, false)
            .try_collect()
            .await
            .expect("the exact byte boundary fits");
        assert_eq!(batches.iter().map(RecordBatch::num_rows).collect::<Vec<_>>(), vec![2, 2]);

        let smaller = admitted(leaf(1000, bytes - 1)).expect("a smaller signed byte bound is admitted");
        let mut batches = opened(&smaller, false);
        let cause = batches
            .next()
            .await
            .expect("an over-budget stream answers a refusal")
            .expect_err("no accepted prefix may precede a cumulative byte refusal");
        let refused = relayed(CombineError::Execute { cause });
        assert!(matches!(refused, CombineError::PartitionDidNotFit), "{refused:?}");
        assert!(batches.next().await.is_none(), "a refused partition emitted another item");
    });
}

#[test]
fn a_reading_grant_with_zero_signed_bytes_is_refused() {
    let cause = admitted(leaf(1000, 0)).expect_err("zero signed bytes must never be widened to a usable budget");
    assert!(
        matches!(
            cause.find_root(),
            DataFusionError::External(cause) if cause.downcast_ref::<WorkerRefusal>() == Some(&WorkerRefusal::GrantNotNaming)
        ),
        "{cause}"
    );
}

#[test]
fn a_partition_scan_without_admission_is_refused_before_reading_its_source() {
    let reads = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&reads);
    let reader: Arc<dyn PartitionReader> = Arc::new(move |_: &SourceName, _: &[u8], schema: &SchemaRef| {
        counted.fetch_add(1, Ordering::SeqCst);
        Ok(Box::pin(RecordBatchStreamAdapter::new(
            Arc::clone(schema),
            stream::empty::<Result<RecordBatch, DataFusionError>>(),
        )) as SendableRecordBatchStream)
    });
    let results = executed(&leaf(4, 1 << 20), reader, 1);
    assert!(
        matches!(results.as_slice(), [Err(DataFusionError::External(cause))]
            if cause.downcast_ref::<WorkerRefusal>() == Some(&WorkerRefusal::GrantNotNaming)),
        "{results:?}"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 0, "an unadmitted scan reached the source");
}

#[test]
fn a_partition_source_failure_is_emitted_without_an_accepted_prefix() {
    servers().block_on(async {
        let leaf = admitted(leaf(4, 1 << 20)).expect("the read is admitted");
        let mut batches = opened(&leaf, true);
        let cause = batches
            .next()
            .await
            .expect("a source failure is emitted")
            .expect_err("a failed source must emit no accepted prefix");
        assert!(matches!(cause, DataFusionError::Execution(message) if message == "source failed"));
        assert!(batches.next().await.is_none(), "a failed partition emitted another item");
    });
}
