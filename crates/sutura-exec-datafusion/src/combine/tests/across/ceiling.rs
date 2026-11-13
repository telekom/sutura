//! A worker counts the rows a task of a question's final stage emits against the whole federated row
//! ceiling the stage carries, and fails at the batch that takes it past (Decision 4).
#![cfg(test)]

use std::sync::Arc;

use datafusion::arrow::array::{Int64Array, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::datasource::memory::MemorySourceConfig;
use datafusion::error::DataFusionError;
use datafusion::physical_plan::ExecutionPlan;
use datafusion_distributed::{
    ExecuteTaskRequest, MaybeEncoded, NetworkBroadcastExec, NetworkCoalesceExec, NetworkShuffleExec, ProducerHead,
    SetPlanRequest, TaskKey,
};
use futures_util::{StreamExt as _, stream};
use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair};
use sutura_domain::plan::FederatedRowCeiling;
use tokio_rustls::rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use tonic::codegen::http::HeaderMap;
use url::Url;

use super::ceiling::CeilingExec;
use super::{NodeIdentity, Nodes, WorkerRefusal};
use crate::pool::WorkingSet;

/// A node signed for 127.0.0.1 by an authority of its own: no cell here dials a link.
fn identity() -> NodeIdentity {
    let mut params = CertificateParams::new(Vec::<String>::new()).expect("no names are valid parameters");
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let authority =
        CertifiedIssuer::self_signed(params, KeyPair::generate().expect("a key generates")).expect("an authority signs itself");
    let key = KeyPair::generate().expect("a key generates");
    let certificate = CertificateParams::new(vec![String::from("127.0.0.1")])
        .expect("an address is valid parameters")
        .signed_by(&key, &authority)
        .expect("the authority signs a node");
    NodeIdentity::new(
        vec![certificate.der().clone()],
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
        [authority.der().clone()],
    )
    .expect("a signed node is an identity")
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

/// Each batch's row count, or the failure, of one task of a final stage holding three rows in two
/// batches, marked with `ceiling` by the planning node and run by another node's worker.
fn ran(ceiling: FederatedRowCeiling) -> Vec<Result<usize, DataFusionError>> {
    let worker = Nodes::worker(roomy(), identity()).expect("a worker builds");
    let at = Url::parse("https://127.0.0.1:1").expect("an address is a URL");
    let planning = Nodes::declared(worker.clone(), at.clone(), [], identity())
        .session(roomy())
        .expect("a session builds");
    let schema = Arc::new(Schema::new(vec![Field::new("n", DataType::Int64, false)]));
    let batch = |values: Vec<i64>| {
        RecordBatch::try_new(Arc::clone(&schema), vec![Arc::new(Int64Array::from(values))]).expect("a batch builds")
    };
    let rows = MemorySourceConfig::try_new_exec(&[vec![batch(vec![1, 2]), batch(vec![3])]], Arc::clone(&schema), None)
        .expect("rows the plan carries are a leaf");
    let stage = MaybeEncoded::Decoded(Arc::new(CeilingExec::new(rows, ceiling)) as Arc<dyn ExecutionPlan>)
        .encode(&planning.task_ctx())
        .expect("a marked stage encodes");
    #[expect(
        clippy::default_trait_access,
        reason = "the question id is a `uuid`, which this crate does not name; the count reads no id"
    )]
    let task_key = TaskKey {
        query_id: Default::default(),
        stage_id: 1,
        task_number: 0,
    };
    servers().block_on(async {
        let set = SetPlanRequest {
            completion: None,
            task_key,
            task_count: 1,
            plan: MaybeEncoded::Encoded(stage),
            work_unit_feed_declarations: vec![],
            target_worker_url: at,
            query_start_time_ns: 0,
        };
        // The coordinator's end of the channel stays open: a task whose channel ends is dropped.
        drop(
            worker
                .coordinator_channel(HeaderMap::new(), set, Box::pin(stream::pending()))
                .await
                .expect("a worker installs a marked stage"),
        );
        let (streams, _) = worker
            .execute_task(ExecuteTaskRequest {
                task_key,
                target_partition_start: 0,
                target_partition_end: 1,
                producer_head: ProducerHead::None,
            })
            .await
            .expect("an installed task runs");
        stream::iter(streams)
            .flatten()
            .map(|batch| batch.map(|batch| batch.num_rows()))
            .collect()
            .await
    })
}

#[test]
fn a_final_stage_task_fails_at_the_batch_that_passes_the_ceiling() {
    let ceiling = FederatedRowCeiling::parse(2).expect("a nonzero ceiling");
    let ran = ran(ceiling);

    let [Ok(first), Err(refused)] = ran.as_slice() else {
        panic!("the task did not fail at its second batch: {ran:?}");
    };
    assert_eq!(*first, 2, "the batch at the ceiling passes");
    assert!(
        matches!(
            refused.find_root(),
            DataFusionError::External(cause)
                if cause.downcast_ref::<WorkerRefusal>() == Some(&WorkerRefusal::TaskOverCeiling { ceiling })
        ),
        "{refused}"
    );
}

#[test]
fn a_final_stage_task_at_exactly_the_ceiling_passes() {
    let ran = ran(FederatedRowCeiling::parse(3).expect("a nonzero ceiling"));

    let rows: Vec<usize> = ran
        .into_iter()
        .map(|batch| batch.expect("a task at its ceiling runs"))
        .collect();
    assert_eq!(rows.iter().sum::<usize>(), 3, "{rows:?}");
}

/// Where each [`CeilingExec`] in a plan sits: directly under the final gather, or below a shuffle or
/// broadcast, whose stage another stage regroups.
#[derive(Default)]
struct Marks {
    under_the_gather: usize,
    below_a_regrouping: usize,
    shuffles: usize,
}

fn marks(plan: &Arc<dyn ExecutionPlan>, gathered: bool, regrouped: bool, seen: &mut Marks) {
    if plan.is::<CeilingExec>() {
        if regrouped {
            seen.below_a_regrouping += 1;
        } else if gathered {
            seen.under_the_gather += 1;
        }
    }
    let shuffled = plan.is::<NetworkShuffleExec>();
    if shuffled {
        seen.shuffles += 1;
    }
    let regrouped = regrouped || shuffled || plan.is::<NetworkBroadcastExec>();
    for child in plan.children() {
        marks(child, plan.is::<NetworkCoalesceExec>(), regrouped, seen);
    }
}

#[test]
fn only_the_stage_under_the_final_gather_is_marked() {
    let own = Nodes::worker(roomy(), identity()).expect("a worker builds");
    let at = |port: u16| Url::parse(&format!("https://127.0.0.1:{port}")).expect("a node is a URL");
    let session = Nodes::declared(own, at(1), [at(2), at(3)], identity())
        .row_ceiling(FederatedRowCeiling::parse(2).expect("a nonzero ceiling"))
        .session(roomy())
        .expect("a session builds");
    let rows = RecordBatch::try_new(
        Arc::new(Schema::new(vec![
            Field::new("k", DataType::Utf8, false),
            Field::new("n", DataType::Int64, false),
        ])),
        vec![
            Arc::new(StringArray::from(vec!["north", "south", "north", "east", "south", "west"])),
            Arc::new(Int64Array::from(vec![1_i64, 2, 3, 4, 5, 6])),
        ],
    )
    .expect("a batch builds");
    drop(session.register_batch("t", rows).expect("an in-memory batch registers"));

    let plan = servers().block_on(async {
        session
            .sql("SELECT k, sum(n) FROM t GROUP BY k")
            .await
            .expect("a grouping plans")
            .create_physical_plan()
            .await
            .expect("a grouping plans across the nodes")
    });
    let mut seen = Marks::default();
    marks(&plan, false, false, &mut seen);

    assert!(seen.shuffles > 0, "the grouping was not planned across the nodes");
    assert_eq!(seen.under_the_gather, 1, "the final stage is not marked once");
    assert_eq!(seen.below_a_regrouping, 0, "a stage another stage regroups is marked");
}
