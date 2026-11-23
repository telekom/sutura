//! A repartition exchange caps an oversized input batch to the session's batch size, and drops the
//! buffers of rows a smaller output batch does not keep.

use std::sync::Arc;

use datafusion::arrow::array::{ArrayRef, Int64Array, StringArray};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::physical_plan::repartition::RepartitionExec;
use datafusion::physical_plan::test::TestMemoryExec;
use datafusion::physical_plan::{Partitioning, collect_partitioned};
use datafusion::prelude::{SessionConfig, SessionContext};

use super::servers;

#[test]
fn repartition_caps_a_large_input_batch_and_keeps_every_row() {
    let rows =
        RecordBatch::try_from_iter([("k", Arc::new(Int64Array::from_iter_values(0..9)) as ArrayRef)]).expect("the batch builds");
    let schema = rows.schema();
    // Deliver the oversized batch directly to the exchange.
    let source = TestMemoryExec::try_new_exec(&[vec![rows]], schema, None).expect("the scan builds");
    let exchange = RepartitionExec::try_new(source, Partitioning::RoundRobinBatch(2)).expect("the exchange builds");
    let session = SessionContext::new_with_config(SessionConfig::new().with_batch_size(4));
    let partitions = servers()
        .block_on(collect_partitioned(Arc::new(exchange), session.task_ctx()))
        .expect("the exchange runs");
    let sizes = partitions.iter().flatten().map(RecordBatch::num_rows).collect::<Vec<_>>();
    assert_eq!(sizes, [4, 4, 1]);
    let keys = partitions
        .iter()
        .flatten()
        .flat_map(|batch| {
            batch
                .column(0)
                .as_any()
                .downcast_ref::<Int64Array>()
                .expect("the output keeps its integer type")
                .values()
                .iter()
                .copied()
        })
        .collect::<Vec<_>>();
    assert_eq!(keys, (0..9).collect::<Vec<_>>());
}

#[test]
fn repartition_drops_unused_string_buffers_before_queueing_batches() {
    let suffix = "x".repeat(2048);
    let labels = (0..13)
        .map(|id| (id != 5).then(|| format!("{id}-{suffix}")))
        .collect::<Vec<_>>();
    let expected = labels.iter().map(Option::as_deref).collect::<Vec<_>>();
    let strings: StringArray = expected.iter().collect();
    let rows = RecordBatch::try_from_iter([("label", Arc::new(strings) as ArrayRef)]).expect("the batch builds");
    let retained = rows.get_array_memory_size();
    let schema = rows.schema();
    let source = TestMemoryExec::try_new_exec(&[vec![rows]], schema, None).expect("the scan builds");
    let exchange = RepartitionExec::try_new(source, Partitioning::RoundRobinBatch(2)).expect("the exchange builds");
    let session = SessionContext::new_with_config(SessionConfig::new().with_batch_size(4));
    let partitions = servers()
        .block_on(collect_partitioned(Arc::new(exchange), session.task_ctx()))
        .expect("the exchange runs");
    let mut actual = Vec::new();
    for batch in partitions.iter().flatten() {
        assert!(
            batch.get_array_memory_size().saturating_mul(2) < retained,
            "each small batch retains only its own strings"
        );
        actual.extend(
            batch
                .column(0)
                .as_any()
                .downcast_ref::<StringArray>()
                .expect("the output keeps its text type")
                .iter(),
        );
    }
    assert_eq!(actual, expected);
}
