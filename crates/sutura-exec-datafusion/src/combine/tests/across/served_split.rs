//! A combine whose lookup leg its source granted rather than read here, planned across four
//! localhost nodes - this planning node, a reader of another source and two readers of the leg's
//! own - the leg's partitions checked for a repeated link and joined only by tasks on the nodes
//! declared as reading its source. Two cells grant two legs instead, each source read by its own
//! pair of nodes or by none.
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
use sutura_domain::identity::SubjectKey;
use sutura_domain::model::SourceName;
use sutura_domain::nonempty::NonEmpty;
use sutura_domain::plan::leg::LegPlan;
use sutura_domain::plan::{
    ExecuteError, FederatedPlan, FederationCombiner as _, Granted, InternalLabel, LegRunner, LinkCast, Looked,
};
use sutura_domain::warehouse::estimate::EstimatedBytes;
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

fn caller() -> SubjectKey {
    SubjectKey::parse("user@example.com").expect("a test caller is a subject")
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

/// A reader answering a partition with its one row, each ask logged in `asked`.
fn reader(asked: Asked) -> Arc<dyn PartitionReader> {
    Arc::new(
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
    )
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
fn grant(schema: SchemaRef, partitions: Vec<Vec<u8>>, caller: &SubjectKey) -> Looked<'static> {
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
    caller: SubjectKey,
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

/// One row per entry of `links`, each column of `schema` holding that entry's label, or null for
/// [`NULL`].
fn filled(schema: &SchemaRef, links: &[u8]) -> Result<RecordBatch, DataFusionError> {
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
    Arc::new(
        move |_: &SourceName, partition: &[u8], schema: &SchemaRef| -> Result<SendableRecordBatchStream, DataFusionError> {
            asked.lock().push(partition.to_vec());
            let batch = filled(schema, partition.get(1..).unwrap_or_default())?;
            Ok(Box::pin(RecordBatchStreamAdapter::new(
                Arc::clone(schema),
                stream::iter(vec![Ok::<_, DataFusionError>(batch)]),
            )))
        },
    )
}

/// [`Peer::serve`], each partition read by [`filling`].
fn filling_peer(servers: &Runtime, authority: &Authority) -> Peer {
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
fn answered_filling<R>(plan: &FederatedPlan, runner: &R) -> Answer
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

/// [`fan_in_plan`]'s question: `geo` granted as [`REPEATED`], `crm` granted with only null links,
/// and `ops` read here.
struct GrantingGeoAndCrm;

impl LegRunner for GrantingGeoAndCrm {
    type Error = Infallible;

    fn run(&self, _leg: &LegPlan) -> Result<ResultBatches, Infallible> {
        Ok(batches_of(
            vec![linked(0), linked(2), String::from(TIME_BUCKET_LABEL), leaf(0)],
            vec![vec![text("c1"), text("c1"), text("2026-06"), Value::Integer(10)]],
        ))
    }

    fn lookup(&self, leg: &LegPlan, _link: LinkCast) -> Result<Looked<'_>, Infallible> {
        if *leg.source() == source("ops") {
            return Ok(Looked::Rows(vec![batches_of(
                vec![linked(2), String::from("tier")],
                vec![vec![text("c1"), text("gold")]],
            )]));
        }
        let (columns, links) = if *leg.source() == source("geo") {
            (vec![linked(0), linked(1), String::from("region")], REPEATED)
        } else {
            (vec![linked(1), String::from("area")], [NULL; 6])
        };
        let fields: Vec<Field> = columns
            .into_iter()
            .map(|name| Field::new(name, DataType::Utf8, true))
            .collect();
        Ok(grant(Arc::new(Schema::new(fields)), partitions(links), &caller()))
    }
}

/// `crm`, granted and joined INNER onto `geo`, has no row its join can match, so that join reads
/// nothing below it; `geo`'s repeated link is refused all the same.
#[test]
fn a_granted_lookup_whose_link_repeats_is_refused_beside_a_later_granted_inner_leg_with_no_joinable_row() {
    let answer = answered_filling(&fan_in_plan(), &GrantingGeoAndCrm);

    assert!(
        matches!(answer, Err(ExecuteError::Combine(CombineError::AmbiguousLink))),
        "a repeated link beside a later granted leg with no joinable row was not refused: {answer:?}"
    );
}
