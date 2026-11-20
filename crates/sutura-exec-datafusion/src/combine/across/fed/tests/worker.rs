//! A fed leaf installed on a real worker, and the rows fed to it over the stream its plan was set on.

use core::num::NonZeroUsize;
use std::sync::Arc;

use datafusion::arrow::record_batch::RecordBatch;
use datafusion::error::DataFusionError;
use datafusion::execution::SendableRecordBatchStream;
use datafusion::execution::TaskContext;
use datafusion::physical_plan::ExecutionPlan;
use datafusion_distributed::{
    CoordinatorToWorkerMsg, ExecuteTaskRequest, MaybeEncoded, ProducerHead, SetPlanRequest, TaskKey, WorkUnit, WorkUnitBatch,
    WorkUnitFeedDeclaration, WorkUnitFeedProto, WorkUnitMsg, Worker, WorkerToCoordinatorMsg,
};
use datafusion_proto::physical_plan::{DefaultPhysicalProtoConverter, PhysicalExtensionCodec as _};
use datafusion_proto::protobuf::Schema as ProtoSchema;
use futures_util::stream::{self, BoxStream};
use futures_util::{StreamExt as _, TryStreamExt as _};
use prost::Message as _;
use tokio::sync::mpsc;
use tonic::codegen::http::HeaderMap;
use url::Url;

use super::super::{Body, Declared, Unit, write};
use super::{batch, leg, runtime, schema};
use crate::combine::CombineError;
use crate::combine::across::partition::{Codec, FED};
use crate::combine::across::relay::relayed;
use crate::combine::across::{NodeIdentity, Nodes};
use crate::pool::WorkingSet;

fn worker() -> Worker {
    let working_set = WorkingSet::of_bytes(NonZeroUsize::new(1 << 30).expect("a nonzero size"));
    Nodes::worker(working_set, NodeIdentity::signed_for_localhost()).expect("a worker builds")
}

/// A fed leaf of one partition, as a node decodes it off a link: reading the feed whose id is the
/// nil one, which is the id every message below is addressed to.
fn leaf() -> Arc<dyn ExecutionPlan> {
    let declared = Declared {
        feed: Some(WorkUnitFeedProto { id: vec![0; 16] }),
        schema: Some(ProtoSchema::try_from(schema().as_ref()).expect("a schema is a message")),
        partitions: 1,
    };
    let mut bytes = vec![FED];
    declared.encode(&mut bytes).expect("a declaration encodes");
    Codec
        .try_decode(&bytes, &[], &TaskContext::default(), &DefaultPhysicalProtoConverter {})
        .expect("a declared leaf decodes")
}

fn task(number: usize) -> TaskKey {
    #[expect(
        clippy::default_trait_access,
        reason = "the question id is a `uuid`, which this crate does not name; every task here is of one question"
    )]
    TaskKey {
        query_id: Default::default(),
        stage_id: 1,
        task_number: number,
    }
}

/// `unit`, as the planning node's feed sends it to partition 0.
fn message(unit: Unit) -> CoordinatorToWorkerMsg {
    #[expect(
        clippy::default_trait_access,
        reason = "the feed's id is a `uuid`, which this crate does not name; it is the nil one"
    )]
    let work = WorkUnitMsg {
        id: Default::default(),
        partition: 0,
        body: MaybeEncoded::Decoded(Box::new(unit) as Box<dyn WorkUnit>),
        created_timestamp_unix_nanos: 0,
        sent_timestamp_unix_nanos: 0,
        received_timestamp_unix_nanos: 0,
        processed_timestamp_unix_nanos: 0,
    };
    CoordinatorToWorkerMsg::WorkUnitBatch(WorkUnitBatch { batch: vec![work] })
}

fn batch_unit(batch: &RecordBatch) -> CoordinatorToWorkerMsg {
    message(Unit {
        body: Some(Body::Batch(write(batch).expect("a batch writes"))),
    })
}

fn end_unit(count: u64) -> CoordinatorToWorkerMsg {
    message(Unit {
        body: Some(Body::End(count)),
    })
}

type Fed = mpsc::UnboundedSender<Result<CoordinatorToWorkerMsg, DataFusionError>>;

/// A worker task of the stage `leaf`, and the channel the planning node feeds it over, held open by
/// the stream it returns. The worker is given the task's plan and its end of the channel.
async fn installed(
    worker: &Worker,
    key: TaskKey,
) -> (
    Fed,
    SendableRecordBatchStream,
    BoxStream<'static, Result<WorkerToCoordinatorMsg, DataFusionError>>,
) {
    let (feed, queue) = mpsc::unbounded_channel();
    #[expect(
        clippy::default_trait_access,
        reason = "the feed's id is a `uuid`, which this crate does not name; it is the nil one"
    )]
    let set = SetPlanRequest {
        completion: None,
        task_key: key,
        task_count: 1,
        plan: MaybeEncoded::Decoded(leaf()),
        work_unit_feed_declarations: vec![WorkUnitFeedDeclaration {
            id: Default::default(),
            partitions: 1,
        }],
        target_worker_url: Url::parse("https://127.0.0.1:1").expect("an address is a URL"),
        query_start_time_ns: 0,
    };
    let kept = worker
        .coordinator_channel(
            HeaderMap::new(),
            set,
            stream::unfold(queue, |mut queue| async move {
                queue.recv().await.map(|message| (message, queue))
            })
            .boxed(),
        )
        .await
        .expect("a worker installs a fed stage");
    let (mut streams, _) = worker
        .execute_task(ExecuteTaskRequest {
            task_key: key,
            target_partition_start: 0,
            target_partition_end: 1,
            producer_head: ProducerHead::None,
        })
        .await
        .expect("an installed task runs");
    assert_eq!(streams.len(), 1, "a task of one partition has one stream");
    (feed, streams.remove(0), kept)
}

fn sent(feed: &Fed, message: CoordinatorToWorkerMsg) {
    feed.send(Ok(message)).expect("the worker still reads its channel");
}

#[test]
fn a_worker_scans_the_rows_it_is_fed_after_its_plan_was_set() {
    let rows = leg(1, 3, 4).remove(0);
    let runtime = runtime();
    let answered: Result<Vec<RecordBatch>, DataFusionError> = runtime.block_on(async {
        let worker = worker();
        let (feed, scan, _kept) = installed(&worker, task(0)).await;
        for rows in &rows {
            sent(&feed, batch_unit(rows));
        }
        sent(&feed, end_unit(3));
        sent(&feed, CoordinatorToWorkerMsg::WorkUnitEos);
        scan.try_collect().await
    });
    assert_eq!(answered.expect("a whole feed is scanned"), rows);
}

#[test]
fn a_worker_whose_feed_is_ended_before_its_end_fails_the_task_as_a_lost_node() {
    let rows = leg(1, 3, 4).remove(0);
    let runtime = runtime();
    let answered = runtime.block_on(async {
        let worker = worker();
        let (feed, scan, _kept) = installed(&worker, task(0)).await;
        sent(&feed, batch_unit(&rows[0]));
        sent(&feed, batch_unit(&rows[1]));
        sent(&feed, CoordinatorToWorkerMsg::WorkUnitEos);
        scan.try_collect::<Vec<_>>().await
    });
    let refused = relayed(CombineError::Execute {
        cause: answered.expect_err("a feed with no end is not a share"),
    });
    assert!(matches!(refused, CombineError::NodeLost { .. }), "{refused:?}");
}

#[test]
fn a_worker_whose_channel_is_lost_under_a_scan_fails_the_task_as_a_lost_node() {
    let rows = leg(1, 3, 4).remove(0);
    let runtime = runtime();
    let (first, then) = runtime.block_on(async {
        let worker = worker();
        let (feed, mut scan, _kept) = installed(&worker, task(0)).await;
        sent(&feed, batch_unit(&rows[0]));
        let first = scan.next().await.expect("the fed batch is scanned");
        drop(feed);
        (first, scan.next().await)
    });
    assert_eq!(first.expect("a batch fed whole scans"), rows[0]);
    let refused = relayed(CombineError::Execute {
        cause: then
            .expect("a lost channel ends the scan with a failure, not silently")
            .expect_err("a lost channel is no share"),
    });
    assert!(matches!(refused, CombineError::NodeLost { .. }), "{refused:?}");
}

#[test]
fn a_task_is_fed_only_over_its_own_channel_and_never_reads_another_tasks_rows() {
    let theirs = leg(1, 2, 4).remove(0);
    let runtime = runtime();
    let (own, other) = runtime.block_on(async {
        let worker = worker();
        let (own_feed, own, _own) = installed(&worker, task(0)).await;
        let (other_feed, other, _other) = installed(&worker, task(1)).await;
        for rows in &theirs {
            sent(&own_feed, batch_unit(rows));
        }
        sent(&own_feed, end_unit(2));
        sent(&own_feed, CoordinatorToWorkerMsg::WorkUnitEos);
        // The other task is fed an empty share under the same feed id.
        sent(&other_feed, end_unit(0));
        sent(&other_feed, CoordinatorToWorkerMsg::WorkUnitEos);
        (own.try_collect::<Vec<_>>().await, other.try_collect::<Vec<_>>().await)
    });
    assert_eq!(own.expect("a task's own feed is scanned"), theirs);
    assert!(
        other.expect("an empty share ends whole").is_empty(),
        "a task read rows fed to another"
    );
}

#[test]
fn a_worker_refuses_half_a_batch_as_a_failure_and_no_lost_node() {
    let runtime = runtime();
    let whole = write(&batch(5, 1)).expect("a batch writes");
    let truncated = whole.get(..whole.len().div_euclid(2)).expect("half of a batch").to_vec();
    let answered = runtime.block_on(async {
        let worker = worker();
        let (feed, scan, _kept) = installed(&worker, task(0)).await;
        sent(
            &feed,
            message(Unit {
                body: Some(Body::Batch(truncated)),
            }),
        );
        sent(&feed, CoordinatorToWorkerMsg::WorkUnitEos);
        scan.try_collect::<Vec<_>>().await
    });
    let refused = relayed(CombineError::Execute {
        cause: answered.expect_err("half a batch is no batch"),
    });
    assert!(matches!(refused, CombineError::Execute { .. }), "{refused:?}");
}
