//! A combine whose lookup leg its source granted rather than read here, planned across localhost
//! nodes - this planning node and two readers of the leg's own, with a reader of another source
//! beside them in the cells built by `granted_across`, and no other node in one cell - the leg's
//! partitions checked for a repeated link and joined only by tasks on the nodes declared as reading
//! its source. Cells granting two legs instead read one source by a pair of nodes and the other by
//! none, each source by its own pair (five nodes), or both by one shared pair.
//!
//! # What this does not exercise
//!
//! The grant is built here, not by a source: `sutura_app`'s leg runner and a PostgreSQL split are
//! held apart from these paths, and none of these cells reads a refusal through `sutura_app`. The
//! readers answer from memory rather than a source, every node shares one authority and one
//! runtime, and no cell reaches the byte bound a grant carries, which no worker enforces.
//!
//! That a granted leg is never read on this node is held by `SplitLeafExec::execute` and
//! `reads_no_source`, not by these cells: `read_here` counts only a runner asked to run a lookup leg.

use core::convert::Infallible;
use core::num::NonZeroU32;
use core::time::Duration;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use datafusion::arrow::array::{ArrayRef, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::catalog::TableProvider;
use datafusion::error::DataFusionError;
use datafusion::execution::SendableRecordBatchStream;
use datafusion::physical_plan::stream::RecordBatchStreamAdapter;
use datafusion::prelude::col;
use futures_util::stream;
use parking_lot::Mutex;
use sutura_domain::catalog::TIME_BUCKET_LABEL;
use sutura_domain::model::SourceName;
use sutura_domain::nonempty::NonEmpty;
use sutura_domain::plan::leg::LegPlan;
use sutura_domain::plan::{
    ExecuteError, FederationCombiner as _, Granted, InternalLabel, LegRunner, LinkCast, Looked,
};
use sutura_domain::warehouse::estimate::EstimatedBytes;
use sutura_domain::warehouse::execution::{Execution, PlanStage};
use sutura_domain::warehouse::{ResultBatches, Value};
use tokio::runtime::Runtime;
use url::Url;

use super::{Authority, listening, roomy, served, servers};
use crate::combine::across::{lost, relayed};
use crate::combine::tests::{UNBOUNDED, batches_of, combined, combiner, fact_linked, fan_in_plan, leaf, link, sum_plan, typed};
use crate::combine::{CombineError, DataFusionCombiner, Nodes, PartitionReader, SplitScan, StreamBound};

/// Each partition's link value, by its index: partition `j` holds the one row of `c{links[j]}`.
type Links = [u8; 6];

/// More partitions than the leg has readers, none of their link values repeated.
const DISTINCT: Links = [0, 1, 2, 3, 4, 5];

/// [`DISTINCT`], but partitions 1 and 2 each hold `c1` once.
const REPEATED: Links = [0, 1, 1, 3, 4, 5];

fn source(name: &str) -> SourceName {
    SourceName::parse(name).expect("a test source is a source")
}

fn caller() -> Subject {
    Subject::verified("user@example.com").expect("a test caller is a subject")
}

fn label(link: u8) -> String {
    format!("c{link}")
}

const fn region(link: u8) -> &'static str {
    if link.is_multiple_of(2) { "north" } else { "south" }
}

/// One descriptor per partition: its index, so no two are the same partition, and its link value.
fn partitions(links: Links) -> Vec<Vec<u8>> {
    (0..).zip(links).map(|(index, link)| vec![index, link]).collect()
}

/// A fact of `A` under each link value `c0` to `c5`, 100 each.
fn fact() -> ResultBatches {
    fact_linked(DataType::Utf8, Arc::new(StringArray::from_iter_values(DISTINCT.map(label))))
}

/// The lookup leg `links` stands for, read whole: one row per partition, each link value in the
/// north when even and the south when odd.
fn whole(links: Links) -> ResultBatches {
    let link = link();
    typed(
        vec![(link.as_str(), DataType::Utf8), ("region", DataType::Utf8)],
        vec![
            Arc::new(StringArray::from_iter_values(links.map(label))),
            Arc::new(StringArray::from_iter_values(links.map(region))),
        ],
        links.len(),
    )
}

/// Every partition a reader was asked for, in the order it was asked.
type Asked = Arc<Mutex<Vec<Vec<u8>>>>;

/// The fake source's in-memory partition operation records its own fixture submission.
struct FixtureReader<F>(F);

impl<F> PartitionReader for FixtureReader<F>
where
    F: Fn(&SourceName, &[u8], &SchemaRef) -> Result<SendableRecordBatchStream, DataFusionError> + Send + Sync,
{
    fn read(
        &self,
        _verifier: &sutura_domain::identity::VerifierId,
        _wire: &[u8],
        source: &SourceName,
        partition: &[u8],
        schema: &SchemaRef,
        capture: Option<crate::SubmissionCapture>,
    ) -> Result<SendableRecordBatchStream, crate::Unread> {
        let recording = capture.as_ref().map(crate::SubmissionCapture::begin).transpose()?;
        let stream = (self.0)(source, partition, schema).map_err(crate::Unread::Failed)?;
        if let Some(recording) = recording {
            let mut facts = ResultBatches::none_under(Arc::clone(schema)).with_execution(
                sutura_domain::warehouse::execution::Execution::DataFusion {
                    source: Some(source.clone()),
                    stage: sutura_domain::warehouse::execution::PlanStage::Files,
                    logical_plan: format!("fixture in-memory partition {partition:?}"),
                },
            );
            recording.complete(&mut facts)?;
        }
        Ok(stream)
    }
}

/// A reader answering a partition with its one row, each ask logged in `asked`.
fn reader(asked: Asked) -> Arc<dyn PartitionReader> {
    Arc::new(FixtureReader(
        move |_: &SourceName, partition: &[u8], schema: &SchemaRef| -> Result<SendableRecordBatchStream, DataFusionError> {
            asked.lock().push(partition.to_vec());
            let &[_, link] = partition else {
                return Err(DataFusionError::Plan(String::from(
                    "a partition here is its index and its link",
                )));
            };
            let columns: Vec<ArrayRef> = vec![
                Arc::new(StringArray::from(vec![label(link)])),
                Arc::new(StringArray::from(vec![region(link)])),
            ];
            let batch = RecordBatch::try_new(Arc::clone(schema), columns)?;
            Ok(Box::pin(RecordBatchStreamAdapter::new(
                Arc::clone(schema),
                stream::iter(vec![Ok::<_, DataFusionError>(batch)]),
            )))
        },
    ))
}

/// A reading node other than the planning one, with every request it was sent and every partition
/// it read.
struct Peer {
    url: Url,
    sent: Arc<AtomicUsize>,
    asked: Asked,
}

impl Peer {
    fn serve(servers: &Runtime, authority: &Authority) -> Self {
        let signed = authority.sign();
        let asked = Asked::default();
        let worker = Nodes::worker_reading(roomy(), authority.identity(&signed), Some(reader(Arc::clone(&asked))))
            .expect("a reading worker builds");
        let sent = Arc::new(AtomicUsize::new(0));
        let url = served(servers, listening(&signed, authority.peers()), worker, Arc::clone(&sent));
        Self { url, sent, asked }
    }

    fn sent(&self) -> usize {
        self.sent.load(Ordering::SeqCst)
    }

    /// Each partition this node read.
    fn read(&self) -> BTreeSet<Vec<u8>> {
        self.asked.lock().iter().cloned().collect()
    }
}

/// `partitions` of a leg answering `schema`, granted to `caller` for an hour.
fn grant(schema: SchemaRef, partitions: Vec<Vec<u8>>, caller: &Subject) -> Looked<'static> {
    let now = SystemTime::now();
    let granted = Granted::new(
        b"granted-leg".to_vec(),
        schema,
        NonEmpty::parse(partitions).expect("a test grant has a partition"),
        caller.clone(),
        now + Duration::from_secs(3600),
        now,
        NonZeroU32::new(1000).expect("a literal row bound is not zero"),
        EstimatedBytes::parse(1 << 10),
        (),
    );
    Looked::Granted(granted.expect("a grant ending in an hour is not past"))
}

/// A runner reading the fact leg here and granting the lookup leg, counting each time it was asked
/// to read a lookup leg here instead.
struct Granting {
    schema: SchemaRef,
    partitions: Vec<Vec<u8>>,
    caller: Subject,
    read_here: AtomicUsize,
}

impl LegRunner for Granting {
    type Error = Infallible;

    fn run(&self, leg: &LegPlan) -> Result<ResultBatches, Infallible> {
        if matches!(leg, LegPlan::Lookup { .. }) {
            self.read_here.fetch_add(1, Ordering::SeqCst);
        }
        Ok(fact())
    }

    fn lookup(&self, _leg: &LegPlan, _link: LinkCast) -> Result<Looked<'_>, Infallible> {
        Ok(grant(Arc::clone(&self.schema), self.partitions.clone(), &self.caller))
    }
}

type Answer = Result<ResultBatches, ExecuteError<Infallible, CombineError>>;

/// `sum_plan`'s question through `combiner`, its lookup leg of `geo` granted as one partition per
/// entry of `links`, and how many times its runner was asked to read that leg here.
fn granting(combiner: &DataFusionCombiner, links: Links) -> (Answer, usize) {
    let plan = sum_plan(true);
    let runner = Granting {
        schema: Arc::clone(whole(links).schema()),
        partitions: partitions(links),
        caller: caller(),
        read_here: AtomicUsize::new(0),
    };
    let answer = combiner.execute(combiner.plan(&plan), &runner, UNBOUNDED);
    (answer, runner.read_here.into_inner())
}

/// What a combine with a granted lookup leg answered, and what each other node saw.
struct Across {
    answer: Answer,
    read_here: usize,
    payments: Peer,
    geo: [Peer; 2],
}

/// [`granting`] across this planning node, a reader of `payments` and two readers of `geo`, the
/// leg's readers declared as `declared` names them.
fn granted_across(links: Links, declared: impl FnOnce(&[Peer; 2]) -> Vec<Url>) -> Across {
    let servers = servers();
    let authority = Authority::generate();
    let payments = Peer::serve(&servers, &authority);
    let geo = [Peer::serve(&servers, &authority), Peer::serve(&servers, &authority)];
    let others = core::iter::once(&payments).chain(&geo).map(|peer| peer.url.clone());
    let nodes = planning(&servers, &authority, others)
        .reading(source("geo"), declared(&geo))
        .reading(source("payments"), [payments.url.clone()]);
    let combiner = DataFusionCombiner::across(nodes).expect("a combiner builds");
    let (answer, read_here) = granting(&combiner, links);
    Across {
        answer,
        read_here,
        payments,
        geo,
    }
}

fn both_readers(geo: &[Peer; 2]) -> Vec<Url> {
    geo.iter().map(|peer| peer.url.clone()).collect()
}

/// A partition is counted once here however often it was read, which a cell below counts.
#[test]
fn a_granted_lookup_is_read_only_by_its_source_s_readers_and_answers_what_the_whole_leg_answers() {
    let across = granted_across(DISTINCT, both_readers);

    let answer = across
        .answer
        .expect("a granted lookup read across its readers answered")
        .to_rows()
        .expect("a combined answer decodes");
    let read_whole = combined(&sum_plan(true), &fact(), &whole(DISTINCT), UNBOUNDED);
    assert_eq!(answer.rows(), read_whole.rows());
    assert_eq!(across.read_here, 0, "execute ran the lookup instead of granting it");
    assert_eq!(
        across.payments.read(),
        BTreeSet::new(),
        "a node reading another source read a granted leg's partition"
    );
    let read: BTreeSet<Vec<u8>> = across.geo.iter().flat_map(Peer::read).collect();
    assert_eq!(
        read,
        partitions(DISTINCT).into_iter().collect(),
        "a partition was read by other than one of the leg's readers"
    );
}

/// One declared reader of `geo`: its stage is still scaled into reading tasks, both dealt to that
/// reader, rather than planned as one task whose leaf refuses to read.
#[test]
fn a_granted_lookup_one_node_reads_is_read_there_and_answers_what_the_whole_leg_answers() {
    let across = granted_across(DISTINCT, |geo| vec![geo[0].url.clone()]);

    let answer = across
        .answer
        .expect("a granted lookup with one reader answered")
        .to_rows()
        .expect("a combined answer decodes");
    let read_whole = combined(&sum_plan(true), &fact(), &whole(DISTINCT), UNBOUNDED);
    assert_eq!(answer.rows(), read_whole.rows());
    assert_eq!(across.read_here, 0, "execute ran the lookup instead of granting it");
    let [ref reader, ref undeclared] = across.geo;
    assert_eq!(
        reader.read(),
        partitions(DISTINCT).into_iter().collect(),
        "the one declared reader read every partition"
    );
    assert_eq!(
        undeclared.read(),
        BTreeSet::new(),
        "a node not declared as reading read a partition"
    );
}

/// Partitions 1 and 2 are read by different tasks on different readers, and each holds `c1` once.
#[test]
fn a_granted_lookup_whose_link_repeats_across_two_partitions_is_refused_as_ambiguous() {
    let across = granted_across(REPEATED, both_readers);

    assert!(
        matches!(across.answer, Err(ExecuteError::Combine(CombineError::AmbiguousLink))),
        "a link value two granted partitions each held once was not refused: {:?}",
        across.answer
    );
}

/// The link check is part of the plan that joins the leg, so no partition is read a second time.
#[test]
fn a_granted_lookup_reads_each_of_its_partitions_once() {
    let across = granted_across(DISTINCT, both_readers);

    assert!(
        across.answer.is_ok(),
        "a granted lookup read across its readers did not answer: {:?}",
        across.answer
    );
    let mut asked: Vec<Vec<u8>> = across.geo.iter().flat_map(|peer| peer.asked.lock().clone()).collect();
    asked.sort();
    assert_eq!(asked, partitions(DISTINCT), "a granted partition was read other than once");
}

/// A runner whose fact leg holds no row, its lookup leg of `geo` granted as [`REPEATED`]'s partitions.
struct NoFactRow;

impl LegRunner for NoFactRow {
    type Error = Infallible;

    fn run(&self, _leg: &LegPlan) -> Result<ResultBatches, Infallible> {
        Ok(fact_linked(DataType::Utf8, Arc::new(StringArray::from(Vec::<&str>::new()))))
    }

    fn lookup(&self, _leg: &LegPlan, _link: LinkCast) -> Result<Looked<'_>, Infallible> {
        Ok(grant(Arc::clone(whole(REPEATED).schema()), partitions(REPEATED), &caller()))
    }
}

/// No fact row reaches the granted leg, and its repeated link is refused all the same: the check is
/// the join's build side, which the join reads whole before the fact leg.
#[test]
fn a_granted_lookup_whose_link_repeats_is_refused_though_no_fact_row_joins_it() {
    let servers = servers();
    let authority = Authority::generate();
    let geo = [Peer::serve(&servers, &authority), Peer::serve(&servers, &authority)];
    let nodes = planning(&servers, &authority, both_readers(&geo)).reading(source("geo"), both_readers(&geo));
    let combiner = DataFusionCombiner::across(nodes).expect("a combiner builds");

    let answer = combiner.execute(combiner.plan(&sum_plan(true)), &NoFactRow, UNBOUNDED);

    assert!(
        matches!(answer, Err(ExecuteError::Combine(CombineError::AmbiguousLink))),
        "a repeated granted link no fact row joins was not refused: {answer:?}"
    );
}

#[test]
fn a_granted_lookup_on_a_combiner_with_no_other_node_is_refused_and_never_read_here() {
    let (answer, read_here) = granting(&combiner(), DISTINCT);

    assert!(
        matches!(
            &answer,
            Err(ExecuteError::Combine(CombineError::NoReadingNode { source_name: refused })) if *refused == source("geo")
        ),
        "a granted lookup with no node to read it was not refused for it: {answer:?}"
    );
    assert_eq!(read_here, 0, "execute ran the lookup instead of granting it");
}

#[test]
fn a_granted_lookup_no_resolved_node_reads_is_refused_as_no_reading_node_and_sent_nowhere() {
    let across = granted_across(DISTINCT, |_| {
        vec![Url::parse("https://127.0.0.1:1").expect("an address is a URL")]
    });

    let refused = match across.answer {
        Err(ExecuteError::Combine(refused)) => refused,
        other => panic!("a granted lookup no resolved node reads was not refused by the combine: {other:?}"),
    };
    assert_eq!(combiner().no_reading_node(&refused), Some(&source("geo")), "{refused:?}");
    assert_eq!(across.read_here, 0, "execute ran the lookup instead of granting it");
    let sent: usize = core::iter::once(&across.payments).chain(&across.geo).map(Peer::sent).sum();
    assert_eq!(sent, 0, "a granted lookup no node reads sent a task");
}

fn text(value: &str) -> Value {
    Value::Text(value.into())
}

fn linked(leg: usize) -> String {
    InternalLabel::link(leg).label()
}

/// A runner for [`fan_in_plan`]'s question: its fact and its `geo` leg read here, its `crm` and
/// `ops` legs each granted as [`DISTINCT`]'s partitions.
struct GrantingTwo;

impl LegRunner for GrantingTwo {
    type Error = Infallible;

    fn run(&self, _leg: &LegPlan) -> Result<ResultBatches, Infallible> {
        Ok(batches_of(
            vec![linked(0), linked(2), String::from(TIME_BUCKET_LABEL), leaf(0)],
            vec![vec![text("c1"), text("c1"), text("2026-06"), Value::Integer(10)]],
        ))
    }

    fn lookup(&self, leg: &LegPlan, _link: LinkCast) -> Result<Looked<'_>, Infallible> {
        if *leg.source() == source("geo") {
            return Ok(Looked::Rows(vec![batches_of(
                vec![linked(0), linked(1), String::from("region")],
                vec![vec![text("c1"), text("r1"), text("north")]],
            )]));
        }
        let (on, named) = if *leg.source() == source("crm") {
            (1, "area")
        } else {
            (2, "tier")
        };
        let schema = Arc::new(Schema::new(vec![
            Field::new(linked(on), DataType::Utf8, true),
            Field::new(named, DataType::Utf8, true),
        ]));
        Ok(grant(schema, partitions(DISTINCT), &caller()))
    }
}

/// This planning node, served on `servers` and signed by `authority`, its other nodes `others`.
fn planning(servers: &Runtime, authority: &Authority, others: impl IntoIterator<Item = Url>) -> Nodes {
    let node = authority.sign();
    let own = Nodes::worker(roomy(), authority.identity(&node)).expect("a worker builds");
    let own_url = served(servers, listening(&node, authority.peers()), own.clone(), Arc::default());
    Nodes::declared(own, own_url, others, authority.identity(&node))
}

/// `crm` is read by two nodes and `ops` by none: the combine is refused for `ops` before `crm`'s
/// check asks its readers for anything.
#[test]
fn a_granted_lookup_no_node_is_declared_as_reading_is_refused_before_an_earlier_one_is_read() {
    let servers = servers();
    let authority = Authority::generate();
    let crm = [Peer::serve(&servers, &authority), Peer::serve(&servers, &authority)];
    let nodes = planning(&servers, &authority, both_readers(&crm)).reading(source("crm"), both_readers(&crm));
    let combiner = DataFusionCombiner::across(nodes).expect("a combiner builds");

    let answer = combiner.execute(combiner.plan(&fan_in_plan()), &GrantingTwo, UNBOUNDED);

    assert!(
        matches!(
            &answer,
            Err(ExecuteError::Combine(CombineError::NoReadingNode { source_name: refused })) if *refused == source("ops")
        ),
        "a granted lookup no node is declared as reading was not refused for it: {answer:?}"
    );
    let read: BTreeSet<Vec<u8>> = crm.iter().flat_map(Peer::read).collect();
    assert_eq!(read, BTreeSet::new(), "an earlier granted lookup was read before the refusal");
}

/// `geo` and `ops` granted in one stage, each read by its own two nodes: each source has a reader,
/// so the stage is refused for no source being read together, not for one being unread, and sent
/// nowhere.
#[test]
fn a_stage_whose_granted_sources_each_have_readers_but_none_in_common_names_none_unread() {
    let servers = servers();
    let authority = Authority::generate();
    let geo = [Peer::serve(&servers, &authority), Peer::serve(&servers, &authority)];
    let ops = [Peer::serve(&servers, &authority), Peer::serve(&servers, &authority)];
    let session = planning(&servers, &authority, both_readers(&geo).into_iter().chain(both_readers(&ops)))
        .reading(source("geo"), both_readers(&geo))
        .reading(source("ops"), both_readers(&ops))
        .session(roomy())
        .expect("a session builds");
    let scan = |on: &str| -> Arc<dyn TableProvider> {
        let leg = SplitScan::granted(
            b"granted-leg".to_vec(),
            source(on),
            Arc::clone(whole(DISTINCT).schema()),
            partitions(DISTINCT),
            &caller(),
            SystemTime::now() + Duration::from_secs(3600),
            StreamBound::new(1000, 1 << 20),
        );
        Arc::new(leg.expect("a split leg is granted"))
    };

    let answer: Result<Vec<RecordBatch>, DataFusionError> = servers.block_on(async {
        session
            .read_table(scan("geo"))?
            .union(session.read_table(scan("ops"))?)?
            .sort(vec![col("region").sort(true, false)])?
            .collect()
            .await
    });

    let cause = answer.expect_err("a stage no node reads every source of was answered");
    let refused = relayed(lost(CombineError::Execute { cause }));
    assert!(
        matches!(refused, CombineError::Execute { .. }),
        "a stage whose sources each have a reader was refused as one unread: {refused:?}"
    );
    let read: BTreeSet<Vec<u8>> = geo.iter().chain(&ops).flat_map(Peer::read).collect();
    assert_eq!(read, BTreeSet::new(), "a stage no node reads every source of was read");
}

/// [`DISTINCT`], but partition 5 holds `c9`, which no fact row carries, and no partition holds `c5`,
/// which a fact row does.
const UNMATCHED: Links = [0, 1, 2, 3, 4, 9];

/// The fact row the granted leg has no row for is kept and the leg's row no fact row carries is
/// dropped, as when the leg is read whole.
#[test]
fn a_granted_lookup_with_a_link_unmatched_on_each_side_answers_what_the_whole_leg_answers() {
    let across = granted_across(UNMATCHED, both_readers);

    let answer = across
        .answer
        .expect("a granted lookup with unmatched links answered")
        .to_rows()
        .expect("a combined answer decodes");
    let read_whole = combined(&sum_plan(true), &fact(), &whole(UNMATCHED), UNBOUNDED);
    assert_eq!(answer.rows(), read_whole.rows());
}

/// The byte a partition's descriptor carries for a null link.
const NULL: u8 = u8::MAX;

/// [`DISTINCT`]'s first four links, and two partitions each holding a null link.
const NULLS: Links = [0, 1, 2, 3, NULL, NULL];

/// `sum_plan`'s question, its lookup leg of `geo` granted as [`NULLS`]'s partitions.
struct GrantingNulls;

impl LegRunner for GrantingNulls {
    type Error = Infallible;

    fn run(&self, _leg: &LegPlan) -> Result<ResultBatches, Infallible> {
        Ok(fact())
    }

    fn lookup(&self, _leg: &LegPlan, _link: LinkCast) -> Result<Looked<'_>, Infallible> {
        Ok(grant(Arc::clone(whole(NULLS).schema()), partitions(NULLS), &caller()))
    }
}

/// Two rows of a granted leg whose link is null are not ambiguous: a null link joins nothing.
#[test]
fn a_granted_lookup_with_two_null_linked_rows_answers_what_the_whole_leg_answers() {
    let answer = answered_filling(&sum_plan(true), &GrantingNulls)
        .expect("a granted lookup with two null-linked rows answered")
        .to_rows()
        .expect("a combined answer decodes");

    let schema = Arc::clone(whole(NULLS).schema());
    let rows = filled(&schema, &NULLS).expect("a test batch is well formed");
    let link = link();
    let read = typed(
        vec![(link.as_str(), DataType::Utf8), ("region", DataType::Utf8)],
        rows.columns().to_vec(),
        NULLS.len(),
    );
    assert_eq!(answer.rows(), combined(&sum_plan(true), &fact(), &read, UNBOUNDED).rows());
}

mod fan_in;
use fan_in::{GrantingGeoAndCrm, Partitions, answered_filling, asked_filling, filled, filling_peer};

/// `crm`, granted and joined INNER onto `geo`, has no row its join can match: each partition holds
/// a null link, or no row at all. An INNER join of it would end the plan before `geo`'s count ran;
/// its RIGHT join reads `geo`'s, so the repeated link is refused all the same.
#[test]
fn a_granted_lookup_whose_link_repeats_is_refused_beside_a_later_granted_inner_leg_with_no_joinable_row() {
    for crm in [partitions([NULL; 6]), vec![vec![6], vec![7]]] {
        let runner = GrantingGeoAndCrm {
            fact: vec![("c1", 10)],
            geo: partitions(REPEATED),
            crm: crm.clone(),
        };

        let answer = answered_filling(&fan_in_plan(), &runner);

        assert!(
            matches!(answer, Err(ExecuteError::Combine(CombineError::AmbiguousLink))),
            "a repeated link beside a later granted leg with no joinable row, partitions {crm:?}, was not \
             refused: {answer:?}"
        );
    }
}

/// Two granted legs on one shared pair of readers, `crm` joined INNER onto `geo`, each holding a null
/// link: the fact's `c2` reaches `geo` but no `crm` row, its `c5` no `geo` row, and `crm`'s `c3` and
/// `c9` meet no joined `geo` row. Each leg is counted in the plan that joins it, so every partition
/// of both is read once, and INNER drops what it does not match.
#[test]
fn two_granted_lookups_read_each_partition_once_and_drop_what_an_inner_leg_does_not_match() {
    let geo = partitions([0, 1, 2, 3, 4, NULL]);
    let mut crm: Partitions = (6..)
        .zip([0, 1, 3, 9, NULL, NULL])
        .map(|(index, link)| vec![index, link])
        .collect();
    crm[5] = crm[4].clone();
    let runner = GrantingGeoAndCrm {
        fact: vec![("c0", 10), ("c1", 20), ("c2", 30), ("c5", 40)],
        geo: geo.clone(),
        crm: crm.clone(),
    };

    let (answer, asked) = asked_filling(&fan_in_plan(), &runner);

    let mut batches = answer.expect("two granted lookups answered");
    let records = batches.take_executions();
    let answer = batches.to_rows()
        .expect("a combined answer decodes");
    assert_eq!(
        answer.rows(),
        &[
            vec![text("c0"), text("c0"), Value::Null, text("2026-06"), Value::Integer(10)],
            vec![text("c1"), text("c1"), text("gold"), text("2026-06"), Value::Integer(20)],
        ]
    );
    let mut every: Partitions = geo.iter().chain(&crm).cloned().collect();
    every.sort();
    assert_eq!(asked, every, "a granted partition was read other than once");
    assert_eq!(records.len(), 18, "the complete worker/input/probe/combine receipt census differs");
    let inputs: Vec<_> = [("facts", "fixture fact submission"), ("geo", "fixture geo grant creation"),
        ("crm", "fixture crm grant creation"), ("ops", "fixture ops submission")]
        .into_iter().map(|(at, text)| Execution::DataFusion {
            source: Some(source(at)), stage: PlanStage::Files, logical_plan: String::from(text),
        }).collect();
    assert_eq!(&records[..4], inputs.as_slice(), "a contributing input or grant receipt was lost");
    let mut expected: Vec<_> = geo.iter().map(|part| ("geo", part)).chain(crm.iter().map(|part| ("crm", part)))
        .map(|(at, part)| Execution::DataFusion {
            source: Some(source(at)), stage: PlanStage::Files,
            logical_plan: format!("fixture in-memory partition {part:?}"),
        }).collect();
    let mut plans = Vec::new();
    for record in records.iter().skip(4) {
        if matches!(record, Execution::DataFusion { source: None, stage: PlanStage::Combine, .. }) {
            plans.push(record);
        } else {
            let at = expected.iter().position(|wanted| wanted == record)
                .expect("a worker supplied an unexpected or duplicate partition receipt");
            expected.remove(at);
        }
    }
    assert!(expected.is_empty(), "a worker dropped a partition receipt, including a repeated descriptor");
    assert_eq!(plans.len(), 2, "the ops ambiguity probe and final combine must both be retained");
    let mut partitions: Vec<_> = records.iter().skip(4).filter(|record| matches!(record,
        Execution::DataFusion { source: Some(_), stage: PlanStage::Files, .. })).collect();
    partitions.sort_by_key(|record| format!("{record:?}"));
    let mut partitions = partitions.into_iter();
    let mut canonical: Vec<_> = records.iter().collect();
    for record in canonical.iter_mut().skip(4) {
        if matches!(*record, Execution::DataFusion { source: Some(_), stage: PlanStage::Files, .. }) {
            *record = partitions.next().expect("each partition slot has one sorted receipt");
        }
    }
    insta::with_settings!({prepend_module_to_snapshot => false, snapshot_path => "../snapshots"}, {
        insta::assert_debug_snapshot!("worker-input-receipts", canonical);
    });
}

use std::collections::BTreeMap;

use datafusion_distributed::Worker;
use sutura_domain::identity::{
    Agreed, Delegated, Expiry, LegCredentials, Minted, Presented, Secret, SourceSet, SourceToken, Subject, WorkloadPool,
};
use sutura_domain::plan::Grantee;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tonic::service::InterceptorLayer;
use tonic::transport::Server;

use super::Tls;

mod ends;
mod tokens;
use ends::EndPeer;
use tokens::{Handed, ReadsWithTokens};

/// The token a granted `geo` leg is read with.
const FORWARDED: &str = "a-forwarded-token-value";

/// Every header value a node was sent.
type Seen = Arc<Mutex<Vec<Vec<u8>>>>;

/// `worker` served over `tls` on a free localhost port, every header value it is sent logged in
/// `seen`.
fn served_seeing(servers: &Runtime, tls: Arc<ServerConfig>, worker: Worker, seen: Seen) -> Url {
    let listener = servers
        .block_on(tokio::net::TcpListener::bind(("127.0.0.1", 0)))
        .expect("a localhost port binds");
    let port = listener.local_addr().expect("a bound port has an address").port();
    let incoming = stream::unfold((listener, TlsAcceptor::from(tls)), |(listener, acceptor)| async move {
        let accepted = match listener.accept().await {
            Ok((tcp, _)) => acceptor.accept(tcp).await.map(Tls),
            Err(cause) => Err(cause),
        };
        Some((accepted, (listener, acceptor)))
    });
    let seeing = InterceptorLayer::new(move |request: tonic::Request<()>| {
        seen.lock().extend(
            request
                .metadata()
                .clone()
                .into_headers()
                .values()
                .map(|value| value.as_bytes().to_vec()),
        );
        Ok(request)
    });
    servers.spawn(
        Server::builder()
            .layer(seeing)
            .add_service(worker.into_worker_server())
            .serve_with_incoming(incoming),
    );
    Url::parse(&format!("https://127.0.0.1:{port}")).expect("a socket address is a URL's authority")
}

/// A reading node that reads only with a token, with each token it was handed and every header
/// value it was sent.
struct TokenPeer {
    url: Url,
    handed: Handed,
    seen: Seen,
}

impl TokenPeer {
    fn started(servers: &Runtime, authority: &Authority) -> Self {
        let signed = authority.sign();
        let (handed, seen) = (Handed::default(), Seen::default());
        let reading = ReadsWithTokens {
            rows: reader(Asked::default()),
            handed: Arc::clone(&handed),
        };
        let worker = Nodes::worker_reading(roomy(), authority.identity(&signed), Some(Arc::new(reading)))
            .expect("a reading worker builds");
        let url = served_seeing(servers, listening(&signed, authority.peers()), worker, Arc::clone(&seen));
        Self { url, handed, seen }
    }
}

/// [`DISTINCT`]'s partitions of `leg`, answering `schema`, granted through a [`Grantee`] for an hour
/// and read with [`FORWARDED`], exchanged from the subject token the request presented for `leg`'s
/// source.
fn granted_with_token(leg: &LegPlan, schema: SchemaRef) -> Looked<'static> {
    let now = SystemTime::now();
    let presented = Secret::new("the-callers-assertion");
    let subject = Subject::verified("user@example.com").expect("a test caller is a subject");
    let sources = SourceSet::of(leg.source().clone());
    let credentials = LegCredentials::minted(
        subject.clone(),
        Expiry::NothingExpires,
        &sources,
        BTreeMap::from([(
            leg.source().clone(),
            Presented::SubjectToken {
                material: presented.clone(),
            },
        )]),
    )
    .expect("one leg covers its one source");
    let Ok(Agreed::Granted { credentials }) = (Minted::Granted { credentials }).agreeing_with(&subject, &sources, 0) else {
        panic!("a fixture grant agrees with its own request");
    };
    let pool = WorkloadPool::parse("//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/p/providers/q")
        .expect("a test pool is a pool");
    let ends = (now + Duration::from_secs(3600))
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after the epoch")
        .as_secs();
    let token = SourceToken::exchanged(&presented, &pool, Delegated::new(Secret::new(FORWARDED), ends));
    let granted = Grantee::new(leg, &credentials, now + Duration::from_secs(3600))
        .expect("a verified caller's lookup leg is offered")
        .forwarding(token)
        .expect("a token exchanged from the caller's own credential is forwarded")
        .grant(
            schema,
            partitions(DISTINCT).into_iter().map(Some),
            NonZeroU32::new(1000).expect("a literal row bound is not zero"),
            EstimatedBytes::parse(1 << 10),
            (),
            now,
        );
    Looked::Granted(granted.expect("a grant ending in an hour is not past"))
}

/// A runner reading the fact leg here and granting the lookup leg as [`granted_with_token`].
struct GrantingWithToken {
    schema: SchemaRef,
}

impl LegRunner for GrantingWithToken {
    type Error = Infallible;

    fn run(&self, _leg: &LegPlan) -> Result<ResultBatches, Infallible> {
        Ok(fact())
    }

    fn lookup(&self, leg: &LegPlan, _link: LinkCast) -> Result<Looked<'_>, Infallible> {
        Ok(granted_with_token(leg, Arc::clone(&self.schema)))
    }
}

#[test]
fn a_granted_lookup_read_with_the_callers_token_hands_it_to_each_partition_read_and_to_no_header() {
    let servers = servers();
    let authority = Authority::generate();
    let payments = TokenPeer::started(&servers, &authority);
    let geo = [
        TokenPeer::started(&servers, &authority),
        TokenPeer::started(&servers, &authority),
    ];
    let others = core::iter::once(&payments).chain(&geo).map(|peer| peer.url.clone());
    let nodes = planning(&servers, &authority, others)
        .reading(source("geo"), geo.iter().map(|peer| peer.url.clone()))
        .reading(source("payments"), [payments.url.clone()]);
    let combiner = DataFusionCombiner::across(nodes).expect("a combiner builds");
    let runner = GrantingWithToken {
        schema: Arc::clone(whole(DISTINCT).schema()),
    };

    let answer = combiner.execute(combiner.plan(&sum_plan(true)), &runner, UNBOUNDED);

    answer.expect("a granted lookup read with the caller's token answered");
    let handed: Vec<_> = geo.iter().flat_map(|peer| peer.handed.lock().clone()).collect();
    assert_eq!(
        handed.iter().map(|(partition, _)| partition.clone()).collect::<BTreeSet<_>>(),
        partitions(DISTINCT).into_iter().collect(),
        "a partition was read by other than one of the leg's readers"
    );
    assert!(handed.iter().all(|(_, token)| token == FORWARDED), "{handed:?}");
    assert!(
        payments.handed.lock().is_empty(),
        "a node reading another source was handed the token"
    );
    let hex: String = FORWARDED
        .bytes()
        .flat_map(|byte| [byte >> 4, byte & 0xf])
        .filter_map(|digit| char::from_digit(u32::from(digit), 16))
        .collect();
    let headers: Vec<Vec<u8>> = core::iter::once(&payments)
        .chain(&geo)
        .flat_map(|peer| peer.seen.lock().clone())
        .collect();
    assert!(!headers.is_empty(), "no node was sent a header to look in");
    assert!(
        headers.iter().all(|value| {
            let value = String::from_utf8_lossy(value);
            !value.contains(FORWARDED) && !value.contains(&hex)
        }),
        "a header carried the forwarded token"
    );
}

/// A reading worker hands its reader the end of the grant it admitted each token under, on its own
/// clock - no later than the token, which the grant was clamped to - and the time that grant had left.
#[test]
fn a_reading_worker_hands_its_reader_the_end_of_the_grant_it_admitted_the_token_under() {
    let asked = SystemTime::now();
    let servers = servers();
    let authority = Authority::generate();
    let payments = EndPeer::logging_ends(&servers, &authority);
    let geo = [
        EndPeer::logging_ends(&servers, &authority),
        EndPeer::logging_ends(&servers, &authority),
    ];
    let others = core::iter::once(&payments).chain(&geo).map(|peer| peer.url.clone());
    let nodes = planning(&servers, &authority, others)
        .reading(source("geo"), geo.iter().map(|peer| peer.url.clone()))
        .reading(source("payments"), [payments.url.clone()]);
    let combiner = DataFusionCombiner::across(nodes).expect("a combiner builds");
    let runner = GrantingWithToken {
        schema: Arc::clone(whole(DISTINCT).schema()),
    };

    let answer = combiner.execute(combiner.plan(&sum_plan(true)), &runner, UNBOUNDED);

    answer.expect("a granted lookup read with the caller's token answered");
    let an_hour = Duration::from_secs(3600);
    let latest = SystemTime::now() + an_hour;
    let ends: Vec<_> = geo.iter().flat_map(|peer| peer.ends.lock().clone()).collect();
    assert!(!ends.is_empty(), "no reader was handed a token");
    assert!(
        ends.iter()
            .all(|&(end, left)| end > asked && end <= latest && left.is_some_and(|left| left <= an_hour)),
        "a reader was handed other than its admitted grant's end, an hour at most: {ends:?}"
    );
}
