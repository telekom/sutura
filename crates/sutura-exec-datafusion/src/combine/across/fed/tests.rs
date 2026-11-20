//! A dealt leg's rows reach each task whole as a feed, and the plan that names the feed carries none
//! of them.

use core::num::NonZeroUsize;
use std::sync::Arc;

use datafusion::arrow::array::{Int64Array, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::common::tree_node::{TreeNode as _, TreeNodeRecursion};
use datafusion::datasource::MemTable;
use datafusion::datasource::memory::{DataSourceExec, MemorySourceConfig};
use datafusion::error::DataFusionError;
use datafusion::execution::TaskContext;
use datafusion::physical_plan::ExecutionPlan;
use datafusion::prelude::SessionConfig;
use datafusion_distributed::{DistributedWorkUnitFeedContext, ScaleUpLeafNodeEvent, WorkUnitFeedProvider as _};
use datafusion_proto::physical_plan::{DefaultPhysicalProtoConverter, PhysicalExtensionCodec as _};
use futures_util::{StreamExt as _, TryStreamExt as _, stream};

use super::{Body, FedRowsExec, Unfed, Unit, dealt, received, write};
use crate::combine::CombineError;
use crate::combine::across::carried;
use crate::combine::across::partition::{Codec, FED};
use crate::combine::across::{NodeIdentity, Nodes};
use crate::pool::WorkingSet;

/// A fed leaf installed on a real worker, fed over the stream its plan was set on.
mod worker;
use crate::combine::across::relay::relayed;

fn schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("k", DataType::Utf8, false),
        Field::new("v", DataType::Int64, false),
    ]))
}

/// `rows` rows whose measure runs on from `first`.
fn batch(first: i64, rows: usize) -> RecordBatch {
    let measures: Vec<i64> = (first..).take(rows).collect();
    let keys: Vec<String> = measures.iter().map(|measure| format!("c{measure}")).collect();
    RecordBatch::try_new(
        schema(),
        vec![Arc::new(StringArray::from(keys)), Arc::new(Int64Array::from(measures))],
    )
    .expect("columns of the schema's own types")
}

/// A leg of `partitions` partitions of `batches` batches, each of `rows` rows, its measures distinct.
fn leg(partitions: usize, batches: usize, rows: usize) -> Vec<Vec<RecordBatch>> {
    (0..partitions)
        .map(|partition| {
            (0..batches)
                .map(|index| {
                    let first = (partition * batches + index) * rows;
                    batch(i64::try_from(first).expect("a small measure"), rows)
                })
                .collect()
        })
        .collect()
}

fn scan(partitions: &[Vec<RecordBatch>]) -> Arc<dyn ExecutionPlan> {
    MemorySourceConfig::try_new_exec(partitions, schema(), None).expect("a leg's partitions are a memory scan")
}

/// `scan` planned across `tasks` tasks, as the planner asks the handler for it.
fn dealt_over(scan: &Arc<dyn ExecutionPlan>, tasks: usize) -> Option<Arc<dyn ExecutionPlan>> {
    let config = SessionConfig::new();
    let event = ScaleUpLeafNodeEvent {
        plan: scan,
        task_count: tasks,
        session_config: &config,
    };
    dealt(event).map(|response| response.expect("a dealable leg is dealt").plan)
}

fn leaf(rows: &[Vec<RecordBatch>], tasks: usize) -> Arc<dyn ExecutionPlan> {
    dealt_over(&scan(rows), tasks).expect("a leg planned across several tasks is dealt")
}

/// A task's context, as the engine's coordinator builds it for a stage of `tasks` tasks.
fn context(tasks: usize) -> Arc<TaskContext> {
    let config = SessionConfig::new().with_extension(Arc::new(DistributedWorkUnitFeedContext { fan_out_tasks: tasks }));
    Arc::new(TaskContext::default().with_session_config(config))
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("a runtime builds")
}

fn dealt_feed(leaf: &Arc<dyn ExecutionPlan>) -> &super::Dealt {
    leaf.downcast_ref::<FedRowsExec>()
        .and_then(|fed| fed.feed.inner())
        .expect("a leaf planned here holds its own feed")
}

/// The leaf as a stage carries it over a link.
fn declared(leaf: &Arc<dyn ExecutionPlan>) -> Vec<u8> {
    let mut buf = Vec::new();
    Codec
        .try_encode(Arc::clone(leaf), &mut buf, &DefaultPhysicalProtoConverter {})
        .expect("a fed leaf encodes");
    buf
}

fn units(batches: &[RecordBatch], end: u64) -> Vec<Result<Unit, DataFusionError>> {
    batches
        .iter()
        .map(|batch| {
            Ok(Unit {
                body: Some(Body::Batch(write(batch).expect("a batch writes"))),
            })
        })
        .chain([Ok(Unit {
            body: Some(Body::End(end)),
        })])
        .collect()
}

fn arrived(units: Vec<Result<Unit, DataFusionError>>) -> Result<Vec<RecordBatch>, DataFusionError> {
    runtime().block_on(received(schema(), stream::iter(units).boxed()).try_collect())
}

fn failed(cause: DataFusionError) -> CombineError {
    relayed(CombineError::Execute { cause })
}

#[test]
fn a_dealt_legs_declaration_has_the_same_size_for_ten_rows_and_a_hundred_thousand() {
    use datafusion_proto::physical_plan::{AsExecutionPlan as _, DefaultPhysicalExtensionCodec};
    use datafusion_proto::protobuf::PhysicalPlanNode;
    use prost::Message as _;

    // The meter must see rows: a scan the plan carries grows with them.
    let carried = |rows| {
        PhysicalPlanNode::try_from_physical_plan(scan(&leg(2, 5, rows)), &DefaultPhysicalExtensionCodec {})
            .expect("a memory scan encodes")
            .encoded_len()
    };
    assert!(carried(10_000) > 100 * carried(1), "the meter no longer sees rows in a plan");

    let small = declared(&leaf(&leg(2, 5, 1), 3));
    let large = declared(&leaf(&leg(2, 5, 10_000), 3));
    assert_eq!(
        small.len(),
        large.len(),
        "a leaf's declaration grew with the rows it will be fed"
    );
    assert!(
        large.len() < 256,
        "a declaration of {} bytes carries more than a schema and a feed",
        large.len()
    );
}

#[test]
fn each_task_is_fed_every_nth_batch_of_each_partition_and_the_shares_are_the_leg() {
    let rows = leg(2, 7, 3);
    let leaf = leaf(&rows, 3);
    let provider = dealt_feed(&leaf);
    let mut shares: Vec<Vec<RecordBatch>> = Vec::new();
    for global in 0..6 {
        let fed = provider
            .feed(global, context(3))
            .expect("the stage runs the tasks the leg was dealt across");
        let batches = runtime()
            .block_on(received(schema(), fed).try_collect::<Vec<_>>())
            .expect("a complete share arrives");
        shares.push(batches);
    }
    for (global, share) in shares.iter().enumerate() {
        let (task, partition) = (global.div_euclid(2), global.rem_euclid(2));
        let expected: Vec<RecordBatch> = rows[partition].iter().skip(task).step_by(3).cloned().collect();
        assert_eq!(share, &expected, "task {task} was not fed its share of partition {partition}");
    }
    let fed: usize = shares.iter().flatten().map(RecordBatch::num_rows).sum();
    assert_eq!(fed, 2 * 7 * 3, "the shares are not the leg's rows");
}

#[test]
fn a_leg_planned_as_one_task_is_not_dealt() {
    assert!(dealt_over(&scan(&leg(2, 5, 3)), 1).is_none());
}

#[test]
fn a_feed_that_ends_after_all_its_batches_arrives_whole() {
    let rows = leg(1, 3, 2).remove(0);
    assert_eq!(arrived(units(&rows, 3)).expect("a whole feed arrives"), rows);
}

#[test]
fn a_feed_that_stops_before_its_end_is_a_lost_node_and_no_partial_answer() {
    let rows = leg(1, 3, 2).remove(0);
    let mut cut = units(&rows, 3);
    cut.pop();
    let refused = arrived(cut).expect_err("a feed with no end is not a share");
    let relayed = failed(refused);
    assert!(matches!(relayed, CombineError::NodeLost { .. }), "{relayed:?}");
}

#[test]
fn a_feed_ended_at_a_count_that_is_not_what_arrived_is_a_lost_node() {
    let rows = leg(1, 3, 2).remove(0);
    for announced in [2, 4] {
        let refused = arrived(units(&rows, announced)).expect_err("a count that disagrees with the batches is no share");
        let relayed = failed(refused);
        assert!(matches!(relayed, CombineError::NodeLost { .. }), "{announced}: {relayed:?}");
    }
}

#[test]
fn a_feed_that_failed_under_the_planning_node_fails_the_task() {
    let rows = leg(1, 2, 2).remove(0);
    let mut broken = units(&rows, 2);
    broken.insert(
        1,
        Err(DataFusionError::Execution(String::from("the planning node's read failed"))),
    );
    arrived(broken).expect_err("a failed feed is not a share");
}

#[test]
fn a_message_that_is_not_a_batch_of_the_leg_fails_the_task_and_is_no_lost_node() {
    let foreign = RecordBatch::try_new(
        Arc::new(Schema::new(vec![Field::new("k", DataType::Int64, false)])),
        vec![Arc::new(Int64Array::from(vec![1]))],
    )
    .expect("a batch of its own schema");
    let undecodable = Unit {
        body: Some(Body::Batch(vec![1, 2, 3])),
    };
    let empty = Unit { body: None };
    let two = {
        let mut bytes = write(&batch(0, 1)).expect("a batch writes");
        bytes.extend(write(&batch(1, 1)).expect("a batch writes"));
        Unit {
            body: Some(Body::Batch(bytes)),
        }
    };
    let refusals = [
        (units(&[foreign], 1), "NotTheLeg"),
        (vec![Ok(undecodable)], "Undecodable"),
        (vec![Ok(empty)], "Empty"),
        (vec![Ok(two)], "Undecodable"),
    ];
    for (units, what) in refusals {
        let refused = arrived(units).expect_err("a message that is no batch of the leg is refused");
        let relayed = failed(refused);
        assert!(matches!(relayed, CombineError::Execute { .. }), "{what}: {relayed:?}");
    }
}

#[test]
fn rows_dealt_across_another_count_of_tasks_than_the_stage_runs_are_refused() {
    let leaf = leaf(&leg(2, 5, 3), 3);
    let refused = dealt_feed(&leaf)
        .feed(0, context(2))
        .err()
        .expect("a stage of two tasks was fed a leg dealt across three");
    assert!(
        matches!(&refused, DataFusionError::External(cause)
            if matches!(cause.downcast_ref::<Unfed>(), Some(Unfed::Mistasked { dealt: 3, running: 2 }))),
        "{refused:?}"
    );
}

#[test]
fn a_message_holds_about_one_unit_for_each_partition_rather_than_the_whole_share() {
    let leaf = leaf(&leg(2, 200, 1), 2);
    let provider = dealt_feed(&leaf);
    let feeds: Vec<_> = (0..2)
        .map(|global| provider.feed(global, context(2)).expect("a share is fed"))
        .collect();
    let chunk = runtime().block_on(async {
        stream::select_all(feeds)
            .ready_chunks(256)
            .next()
            .await
            .expect("a feed has messages")
    });
    assert!(chunk.len() <= 8, "{} units were ready at once", chunk.len());
}

#[test]
fn a_fed_leaf_crosses_a_link_as_the_leaf_it_was_and_reads_its_rows_off_the_feed() {
    let leaf = leaf(&leg(2, 5, 3), 3);
    let on_the_wire = declared(&leaf);
    let (&tag, _) = on_the_wire.split_first().expect("a declaration is not empty");
    assert_eq!(tag, FED);
    let decoded = Codec
        .try_decode(&on_the_wire, &[], &TaskContext::default(), &DefaultPhysicalProtoConverter {})
        .expect("a declared leaf decodes");
    let read = decoded.downcast_ref::<FedRowsExec>().expect("it decodes as a fed leaf");
    let sent = leaf.downcast_ref::<FedRowsExec>().expect("the leaf was fed");
    assert_eq!(decoded.schema(), leaf.schema());
    assert_eq!(
        decoded.properties().partitioning.partition_count(),
        leaf.properties().partitioning.partition_count()
    );
    assert_eq!(read.feed.to_proto(), sent.feed.to_proto());
    assert!(
        read.feed.inner().is_none(),
        "a decoded leaf holds the rows it would read: it reads a feed"
    );
    Codec
        .try_decode(&[FED], &[], &TaskContext::default(), &DefaultPhysicalProtoConverter {})
        .expect_err("a declaration with no feed is no leaf");
}

#[test]
fn a_node_reads_no_source_for_a_fed_leaf() {
    assert!(carried(&leaf(&leg(2, 5, 3), 3)));
}

/// The leaves of the plan a grouping over `rows` rows planned across three nodes, as the planner
/// builds it.
fn planned_leaves(rows: usize) -> Vec<Arc<dyn ExecutionPlan>> {
    let identity = NodeIdentity::signed_for_localhost();
    let working_set = WorkingSet::of_bytes(NonZeroUsize::new(1 << 30).expect("a nonzero size"));
    let own = Nodes::worker(working_set, identity.clone()).expect("a worker builds");
    let at = |port: u16| url::Url::parse(&format!("https://127.0.0.1:{port}")).expect("a node is a URL");
    let session = Nodes::declared(own, at(1), [at(2), at(3)], identity)
        .session(working_set)
        .expect("a session builds");
    let table = MemTable::try_new(schema(), leg(2, 5, rows)).expect("a leg is a table");
    drop(session.register_table("t", Arc::new(table)).expect("a table registers"));
    let plan = runtime().block_on(async {
        session
            .sql("SELECT k, sum(v) FROM t GROUP BY k")
            .await
            .expect("a grouping plans")
            .create_physical_plan()
            .await
            .expect("a grouping plans across the nodes")
    });
    let mut leaves = Vec::new();
    plan.apply(|node| {
        if node.children().is_empty() {
            leaves.push(Arc::clone(node));
        }
        Ok(TreeNodeRecursion::Continue)
    })
    .expect("a plan is walked");
    leaves
}

#[test]
fn a_leg_planned_across_three_nodes_is_planned_fed_and_its_stage_declares_the_same_bytes_whatever_its_rows() {
    let declarations = |rows| {
        let leaves = planned_leaves(rows);
        assert!(
            leaves.iter().all(|leaf| !leaf.is::<DataSourceExec>()),
            "a stage still carries its leg's rows: {leaves:?}"
        );
        let fed: Vec<_> = leaves.iter().filter(|leaf| leaf.is::<FedRowsExec>()).collect();
        assert_eq!(fed.len(), 1, "the grouped leg was not planned fed: {leaves:?}");
        declared(fed[0]).len()
    };
    assert_eq!(declarations(1), declarations(10_000));
}
