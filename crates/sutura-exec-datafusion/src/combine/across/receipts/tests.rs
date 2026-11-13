//! Falsifiers for native work surviving its async receiver and for local physical-plan scope.

use std::sync::Arc;
use std::time::Duration;

use datafusion::arrow::array::Int64Array;
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::datasource::MemTable;
use datafusion::prelude::{SessionConfig, SessionContext};
use futures_util::{StreamExt as _, poll};
use sutura_domain::model::SourceName;
use sutura_domain::warehouse::ResultBatches;
use sutura_domain::warehouse::execution::{Execution, PlanStage};

use super::{CaptureDistributed, SubmissionCapture, TaskReceiptBudget, completed, executing};
use crate::pool::WorkingSet;

fn facts(source: &str) -> ResultBatches {
    ResultBatches::none_under(Arc::new(Schema::empty())).with_execution(Execution::DataFusion {
        source: Some(SourceName::parse(source).expect("a fixture source is valid")),
        stage: PlanStage::Files,
        logical_plan: String::from("fixture zero-row native submission"),
    })
}

fn slot(source: &str, budget: Arc<TaskReceiptBudget>) -> SubmissionCapture {
    SubmissionCapture::new(SourceName::parse(source).expect("a fixture source is valid"), budget)
}

#[test]
fn worker_capture_waits_for_native_work_after_its_async_receiver_is_dropped() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("a fixture runtime builds")
        .block_on(async {
            let budget = Arc::new(TaskReceiptBudget::new(2, 4096));
            let capture = slot("native_source", Arc::clone(&budget));
            let reading = capture.for_read();
            let native_capture = reading.clone();
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            #[expect(
                clippy::disallowed_methods,
                reason = "a lifetime falsifier controls a blocking fake with no request span"
            )]
            let native = tokio::task::spawn_blocking(move || {
                let recording = native_capture.begin().expect("the native submission begins");
                started_tx.send(()).expect("the receiver waits for native start");
                release_rx.recv().expect("the fake source is released");
                recording.complete(&mut facts("native_source")).expect("the actual facts fit");
            });
            let receiver = tokio::spawn(native);
            started_rx.await.expect("the native closure is running");
            receiver.abort();
            assert!(receiver.await.expect_err("the async receiver was aborted").is_cancelled());
            capture.seal();
            let mut finished = Box::pin(capture.finish());
            let pending_before_release = poll!(&mut finished).is_pending();
            release_tx.send(()).expect("the blocking closure still holds its receiver");
            assert!(pending_before_release, "stream drop must not finalize running native work");
            let captured = tokio::time::timeout(Duration::from_secs(10), finished)
                .await
                .expect("released native work finishes")
                .expect("finalized capture succeeds");
            assert_eq!(captured, vec![(0, facts("native_source").take_executions())]);
            reading.require_recorded().expect("the successful read supplied its facts");
            assert!(
                capture.for_read().require_recorded().is_err(),
                "one read cannot cover another"
            );
            assert!(capture.begin().is_err(), "sealed capture admits no late native work");

            let abandoned = slot("source", Arc::new(TaskReceiptBudget::new(1, 4096)));
            drop(abandoned.begin().expect("a submission starts"));
            abandoned.seal();
            assert!(abandoned.finish().await.is_err(), "a dropped native guard is incomplete");
            let first = slot("other_source", Arc::clone(&budget));
            first
                .begin()
                .expect("one task record remains")
                .complete(&mut facts("other_source"))
                .expect("facts fit");
            let second = slot("source", budget);
            assert!(second.begin().is_err(), "all leaves share the task count bound");
            let foreign = slot("expected_source", Arc::new(TaskReceiptBudget::new(1, 4096)));
            assert!(
                foreign
                    .begin()
                    .expect("a submission starts")
                    .complete(&mut facts("foreign_source"))
                    .is_err()
            );
            foreign.seal();
            assert!(foreign.finish().await.is_err(), "foreign source facts cannot finalize");
            let too_small = slot("source", Arc::new(TaskReceiptBudget::new(1, 1)));
            assert!(
                too_small
                    .begin()
                    .expect("a submission starts")
                    .complete(&mut facts("source"))
                    .is_err()
            );
            too_small.seal();
            assert!(too_small.finish().await.is_err(), "overflow cannot produce an empty success");
        });
}

#[test]
fn worker_capture_accepts_only_a_witnessed_local_plan_without_worker_receipts() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a fixture runtime builds")
        .block_on(async {
            let context = SessionContext::new_with_config(SessionConfig::new().with_extension(Arc::new(CaptureDistributed)));
            let frame = context.sql("SELECT 7 AS n").await.expect("a constant plans");
            let working_set = WorkingSet::of_bytes(core::num::NonZeroUsize::new(1 << 20).expect("a positive bound"));
            let (mut stream, capture) = executing(frame, working_set)
                .await
                .expect("the local physical plan is witnessed");
            assert!(capture.is_none());
            let mut rows = 0;
            while let Some(batch) = stream.next().await {
                rows += batch.expect("a constant executes").num_rows();
            }
            drop(stream);
            assert_eq!(rows, 1);
            assert_eq!(
                completed(capture, working_set).expect("a local plan has no worker facts"),
                Vec::new()
            );
            let incomplete = Arc::new(
                datafusion_distributed::QueryCompletion::new(1, 1, 4096, tokio::time::Instant::now() + Duration::from_secs(10))
                    .expect("a capture scope is bounded"),
            );
            assert!(
                completed(Some(incomplete), working_set).is_err(),
                "an unwitnessed missing barrier is an error"
            );
        });
}

#[test]
fn receipt_setup_preserves_registered_tables_for_later_queries() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a fixture runtime builds")
        .block_on(async {
            let context = SessionContext::new_with_config(SessionConfig::new().with_extension(Arc::new(CaptureDistributed)));
            let schema = Arc::new(Schema::new(vec![Field::new("n", DataType::Int64, false)]));
            let batch = RecordBatch::try_new(Arc::clone(&schema), vec![Arc::new(Int64Array::from(vec![7_i64]))])
                .expect("the fixture has one integer column");
            let table = MemTable::try_new(schema, vec![vec![batch]]).expect("the fixture table builds");
            context
                .register_table("fact", Arc::new(table))
                .expect("the fixture table is registered");
            let working_set = WorkingSet::of_bytes(core::num::NonZeroUsize::new(1 << 20).expect("a positive bound"));
            for _ in 0..2 {
                let frame = context.sql("SELECT n FROM fact").await.expect("the registered table plans");
                let (mut stream, capture) = executing(frame, working_set).await.expect("the table plan is witnessed");
                let mut values = Vec::new();
                while let Some(batch) = stream.next().await {
                    let batch = batch.expect("the registered table executes");
                    let integers = batch
                        .column(0)
                        .as_any()
                        .downcast_ref::<Int64Array>()
                        .expect("the result retains the integer column");
                    values.extend(integers.values().iter().copied());
                }
                drop(stream);
                assert_eq!(values, vec![7]);
                assert!(
                    context.table_exist("fact").expect("the catalog can be read"),
                    "receipt setup replaced the registered catalog"
                );
                assert_eq!(
                    completed(capture, working_set).expect("a local table has no worker facts"),
                    Vec::new()
                );
            }
        });
}
