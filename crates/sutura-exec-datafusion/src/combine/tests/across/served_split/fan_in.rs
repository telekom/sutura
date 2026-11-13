//! Shared runner and partition capture for the parent fan-in cells.

use core::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use datafusion::arrow::array::{ArrayRef, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::error::DataFusionError;
use datafusion::execution::SendableRecordBatchStream;
use datafusion::physical_plan::stream::RecordBatchStreamAdapter;
use futures_util::stream;
use sutura_domain::model::SourceName;
use sutura_domain::plan::{ExecuteError, FederatedPlan, FederationCombiner as _, LegPlan, LegRunner, LinkCast, Looked};
use sutura_domain::warehouse::execution::{Execution, PlanStage};
use sutura_domain::warehouse::{ResultBatches, Value};

use super::{Answer, Asked, Authority, FixtureReader, NULL, Peer, UNBOUNDED, TIME_BUCKET_LABEL, batches_of, both_readers, caller,
    grant, label, leaf, linked, listening, planning, roomy, served, servers, source, text};
use tokio::runtime::Runtime;
use crate::combine::{DataFusionCombiner, Nodes, PartitionReader};

/// [`super::fan_in_plan`]'s question: a fact row per entry of `fact`, under its link value for both `geo`
/// and `ops`, `geo` and `crm` granted as their partitions, and `ops` read here holding `c1`.
pub(super) struct GrantingGeoAndCrm {
    pub(super) fact: Vec<(&'static str, i64)>,
    pub(super) geo: Vec<Vec<u8>>,
    pub(super) crm: Vec<Vec<u8>>,
}

impl LegRunner for GrantingGeoAndCrm {
    type Error = Infallible;

    fn run(&self, leg: &LegPlan) -> Result<ResultBatches, Infallible> {
        Ok(batches_of(
            vec![linked(0), linked(2), String::from(TIME_BUCKET_LABEL), leaf(0)],
            self.fact
                .iter()
                .map(|&(link, cents)| vec![text(link), text(link), text("2026-06"), Value::Integer(cents)])
                .collect(),
        ).with_execution(Execution::DataFusion {
            source: Some(leg.source().clone()), stage: PlanStage::Files,
            logical_plan: String::from("fixture fact submission"),
        }))
    }

    fn lookup(&self, leg: &LegPlan, _link: LinkCast) -> Result<Looked<'_>, Infallible> {
        if *leg.source() == source("ops") {
            return Ok(Looked::Rows(vec![batches_of(
                vec![linked(2), String::from("tier")],
                vec![vec![text("c1"), text("gold")]],
            ).with_execution(Execution::DataFusion {
                source: Some(leg.source().clone()), stage: PlanStage::Files,
                logical_plan: String::from("fixture ops submission"),
            })]));
        }
        let (columns, granted) = if *leg.source() == source("geo") {
            (vec![linked(0), linked(1), String::from("region")], &self.geo)
        } else {
            (vec![linked(1), String::from("area")], &self.crm)
        };
        let fields: Vec<Field> = columns
            .into_iter()
            .map(|name| Field::new(name, DataType::Utf8, true))
            .collect();
        let Looked::Granted(granted) = grant(Arc::new(Schema::new(fields)), granted.clone(), &caller()) else {
            unreachable!();
        };
        Ok(Looked::Granted(granted.with_executions(vec![Execution::DataFusion {
            source: Some(leg.source().clone()), stage: PlanStage::Files,
            logical_plan: format!("fixture {} grant creation", leg.source()),
        }])))
    }
}

/// Partition descriptors, one per partition.
pub(super) type Partitions = Vec<Vec<u8>>;

/// [`answered_filling`]'s combine, and every partition either reader was asked for, sorted.
pub(super) fn asked_filling(plan: &FederatedPlan, runner: &GrantingGeoAndCrm) -> (Answer, Partitions) {
    let servers = servers();
    let authority = Authority::generate();
    let readers = [filling_peer(&servers, &authority), filling_peer(&servers, &authority)];
    let nodes = planning(&servers, &authority, both_readers(&readers))
        .reading(source("geo"), both_readers(&readers))
        .reading(source("crm"), both_readers(&readers));
    let combiner = DataFusionCombiner::across(nodes).expect("a combiner builds");
    let answer = combiner.execute(combiner.plan(plan), runner, UNBOUNDED);
    let mut asked: Partitions = readers.iter().flat_map(|peer| peer.asked.lock().clone()).collect();
    asked.sort();
    (answer, asked)
}

/// One row per entry of `links`, each column of `schema` holding that entry's label, or null for
/// [`NULL`].
pub(super) fn filled(schema: &SchemaRef, links: &[u8]) -> Result<RecordBatch, DataFusionError> {
    let column = || -> ArrayRef {
        Arc::new(StringArray::from_iter(
            links.iter().map(|&link| (link != NULL).then(|| label(link))),
        ))
    };
    Ok(RecordBatch::try_new(
        Arc::clone(schema),
        schema.fields().iter().map(|_| column()).collect(),
    )?)
}

/// A reader answering a partition with [`filled`]'s one row for its link, each ask logged in `asked`.
fn filling(asked: Asked) -> Arc<dyn PartitionReader> {
    Arc::new(FixtureReader(
        move |_: &SourceName, partition: &[u8], schema: &SchemaRef| -> Result<SendableRecordBatchStream, DataFusionError> {
            asked.lock().push(partition.to_vec());
            let batch = filled(schema, partition.get(1..).unwrap_or_default())?;
            Ok(Box::pin(RecordBatchStreamAdapter::new(
                Arc::clone(schema),
                stream::iter(vec![Ok::<_, DataFusionError>(batch)]),
            )))
        },
    ))
}

/// [`Peer::serve`], each partition read by [`filling`].
pub(super) fn filling_peer(servers: &Runtime, authority: &Authority) -> Peer {
    let signed = authority.sign();
    let asked = Asked::default();
    let worker = Nodes::worker_reading(roomy(), authority.identity(&signed), Some(filling(Arc::clone(&asked))))
        .expect("a reading worker builds");
    let sent = Arc::new(AtomicUsize::new(0));
    let url = served(servers, listening(&signed, authority.peers()), worker, Arc::clone(&sent));
    Peer { url, sent, asked }
}

/// `plan` answered for `runner` across this planning node and two nodes reading `geo` and `crm`,
/// each partition read by [`filling`].
pub(super) fn answered_filling<R>(plan: &FederatedPlan, runner: &R) -> Answer
where
    R: LegRunner<Error = Infallible> + Sync,
{
    let servers = servers();
    let authority = Authority::generate();
    let readers = [filling_peer(&servers, &authority), filling_peer(&servers, &authority)];
    let nodes = planning(&servers, &authority, both_readers(&readers))
        .reading(source("geo"), both_readers(&readers))
        .reading(source("crm"), both_readers(&readers));
    let combiner = DataFusionCombiner::across(nodes).map_err(ExecuteError::Combine)?;
    combiner.execute(combiner.plan(plan), runner, UNBOUNDED)
}

