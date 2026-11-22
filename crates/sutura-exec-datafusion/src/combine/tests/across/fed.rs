//! A single lookup batch plans as one `FedRowsExec` naming a feed, and dealt across three tasks
//! gives each task a contiguous row range rather than a whole-batch share.

use std::sync::Arc;

use datafusion::arrow::array::{ArrayRef, Int64Array};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::common::tree_node::{TreeNode as _, TreeNodeRecursion};
use datafusion::functions_aggregate::expr_fn::sum;
use datafusion::logical_expr::col;
use datafusion::physical_plan::ExecutionPlan;
use url::Url;

use super::{Authority, roomy, servers};
use crate::combine::Nodes;
use crate::combine::across::fed::{FedRowsExec, shares_for_test};

/// A single lookup batch plans as one `FedRowsExec` naming a feed, rather than carrying any rows
/// itself - the content of a task's share is asserted end to end by
/// `a_single_lookup_batch_is_shared_by_rows_across_three_tasks` below.
#[test]
fn a_single_lookup_batch_plans_as_one_fed_leaf_naming_a_feed() {
    let authority = Authority::generate();
    let node = authority.sign();
    let own = Nodes::worker(roomy(), authority.identity(&node)).expect("a worker builds");
    let at = |port: u16| Url::parse(&format!("https://127.0.0.1:{port}")).expect("a node is a URL");
    let session = Nodes::declared(own, at(1), [at(2), at(3)], authority.identity(&node))
        .session(roomy())
        .expect("a session builds");
    let rows = RecordBatch::try_from_iter([
        ("k", Arc::new(Int64Array::from_iter_values(0..7)) as ArrayRef),
        ("n", Arc::new(Int64Array::from_iter_values(10..17)) as ArrayRef),
    ])
    .expect("the source returned one batch");
    let grouped = session
        .read_batch(rows)
        .expect("the lookup reads")
        .aggregate(vec![col("k")], vec![sum(col("n"))])
        .expect("the grouping builds");
    let plan = servers()
        .block_on(grouped.create_physical_plan())
        .expect("the grouping plans across the nodes");
    let mut fed_leaves = Vec::new();
    plan.apply(|plan| {
        if let Some(leaf) = plan.downcast_ref::<FedRowsExec>() {
            fed_leaves.push(leaf.schema());
        }
        Ok(TreeNodeRecursion::Continue)
    })
    .expect("the plan is traversed");
    assert_eq!(fed_leaves.len(), 1, "one feed names the lookup's single batch");
    let schema = &fed_leaves[0];
    assert!(
        schema.index_of("k").is_ok(),
        "the grouping key is still the fed leaf's own schema"
    );
}

/// A single lookup batch of 7 rows, dealt across 3 tasks, gives each task a contiguous row range -
/// not a whole-batch share, which would give task 0 all 7 rows and leave tasks 1 and 2 with none.
///
/// Moved here from `combine/tests/across.rs`, under its original name and asserting the same
/// `[[0,1,2],[3,4],[5,6]]` split; it now reads the single fed leaf's own feed rather than per-task
/// leaves, since `fed::dealt`'s leaf no longer carries rows itself.
#[test]
fn a_single_lookup_batch_is_shared_by_rows_across_three_tasks() {
    let authority = Authority::generate();
    let node = authority.sign();
    let own = Nodes::worker(roomy(), authority.identity(&node)).expect("a worker builds");
    let at = |port: u16| Url::parse(&format!("https://127.0.0.1:{port}")).expect("a node is a URL");
    let session = Nodes::declared(own, at(1), [at(2), at(3)], authority.identity(&node))
        .session(roomy())
        .expect("a session builds");
    let rows = RecordBatch::try_from_iter([
        ("k", Arc::new(Int64Array::from_iter_values(0..7)) as ArrayRef),
        ("n", Arc::new(Int64Array::from_iter_values(10..17)) as ArrayRef),
    ])
    .expect("the source returned one batch");
    let grouped = session
        .read_batch(rows)
        .expect("the lookup reads")
        .aggregate(vec![col("k")], vec![sum(col("n"))])
        .expect("the grouping builds");
    let plan = servers()
        .block_on(grouped.create_physical_plan())
        .expect("the grouping plans across the nodes");
    let mut leaf: Option<Arc<dyn ExecutionPlan>> = None;
    plan.apply(|plan| {
        if plan.downcast_ref::<FedRowsExec>().is_some() {
            leaf = Some(Arc::clone(plan));
            return Ok(TreeNodeRecursion::Stop);
        }
        Ok(TreeNodeRecursion::Continue)
    })
    .expect("the plan is traversed");
    let leaf = leaf.expect("the lookup's single batch is dealt as one fed leaf");
    let leaf = leaf
        .downcast_ref::<FedRowsExec>()
        .expect("the leaf found above is a fed leaf");
    let shares = shares_for_test(leaf, 3, &servers())
        .into_iter()
        .map(|batches| {
            batches
                .iter()
                .flat_map(|batch| {
                    let index = batch.schema().index_of("k").expect("the grouping key is present");
                    let key = batch
                        .column(index)
                        .as_any()
                        .downcast_ref::<Int64Array>()
                        .expect("the key is an integer");
                    key.values().iter().copied().collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(shares, vec![vec![0, 1, 2], vec![3, 4], vec![5, 6]]);
}
