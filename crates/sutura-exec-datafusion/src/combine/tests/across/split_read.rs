//! A split leg planned across five localhost nodes - this planning node, a combining-only worker, a
//! reader of another source and two readers of the leg's own - its partitions read only by tasks on
//! the nodes declared as reading its source.
//!
//! # What this does not exercise
//!
//! The leg is a `SplitScan` built here, not by a granted lookup leg. Its readers answer from
//! memory rather than a source, and every node shares one authority and one runtime.

use core::time::Duration;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use datafusion::arrow::array::Int64Array;
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use datafusion::arrow::error::ArrowError;
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::catalog::TableProvider;
use datafusion::datasource::MemTable;
use datafusion::error::DataFusionError;
use datafusion::execution::SendableRecordBatchStream;
use datafusion::physical_plan::stream::RecordBatchStreamAdapter;
use datafusion::prelude::{SessionContext, col};
use futures_util::stream;
use parking_lot::Mutex;
use sutura_domain::identity::Subject;
use sutura_domain::model::SourceName;
use tokio::runtime::Runtime;
use url::Url;

use super::{Authority, listening, roomy, served, servers};
use crate::combine::across::NoReadingNode;
use crate::combine::{Nodes, PartitionReader, SplitScan, StreamBound};

/// More partitions than there are nodes, so each task reads more than one.
const PARTITIONS: u8 = 6;

fn schema() -> SchemaRef {
    Arc::new(Schema::new(vec![Field::new("n", DataType::Int64, false)]))
}

fn source(name: &str) -> SourceName {
    SourceName::parse(name).expect("a test source is a source")
}

fn partitions() -> Vec<Vec<u8>> {
    (0..PARTITIONS).map(|index| vec![index]).collect()
}

/// Partition `[j]`'s rows: `10j` and `10j + 1`.
fn rows(partition: &[u8]) -> Result<RecordBatch, ArrowError> {
    let values: Vec<i64> = partition
        .iter()
        .flat_map(|&index| {
            let first = i64::from(index) * 10;
            [first, first + 1]
        })
        .collect();
    RecordBatch::try_new(schema(), vec![Arc::new(Int64Array::from(values))])
}

/// Every partition a reader was asked for, in the order it was asked.
type Asked = Arc<Mutex<Vec<Vec<u8>>>>;

/// A reader answering every partition it is asked for with its rows, each ask logged in `asked`.
fn reader(asked: Asked) -> Arc<dyn PartitionReader> {
    Arc::new(
        move |_: &SourceName, partition: &[u8], schema: &SchemaRef| -> Result<SendableRecordBatchStream, DataFusionError> {
            asked.lock().push(partition.to_vec());
            let batch = rows(partition)?;
            Ok(Box::pin(RecordBatchStreamAdapter::new(
                Arc::clone(schema),
                stream::iter(vec![Ok::<_, DataFusionError>(batch)]),
            )))
        },
    )
}

/// A node other than the planning one, with every request it was sent and every partition it read.
struct Peer {
    url: Url,
    sent: Arc<AtomicUsize>,
    asked: Asked,
}

impl Peer {
    /// A node signed by `authority`, served on `servers`: a reading worker when `reads`, and a
    /// combining-only one otherwise.
    fn serve(servers: &Runtime, authority: &Authority, reads: bool) -> Self {
        let signed = authority.sign();
        let identity = authority.identity(&signed);
        let asked = Arc::new(Mutex::new(Vec::new()));
        let worker = if reads {
            Nodes::worker_reading(roomy(), identity, Some(reader(Arc::clone(&asked))))
        } else {
            Nodes::worker(roomy(), identity)
        }
        .expect("a worker builds");
        let sent = Arc::new(AtomicUsize::new(0));
        let url = served(servers, listening(&signed, authority.peers()), worker, Arc::clone(&sent));
        Self { url, sent, asked }
    }

    fn sent(&self) -> usize {
        self.sent.load(Ordering::SeqCst)
    }

    fn asked(&self) -> Vec<Vec<u8>> {
        self.asked.lock().clone()
    }
}

/// What a split leg of `orders` answered when read across its nodes, and what each other node saw.
struct Across {
    answer: Result<Vec<RecordBatch>, DataFusionError>,
    combining: Peer,
    elsewhere: Peer,
    orders: [Peer; 2],
}

/// `table`'s rows ordered by `n`, read through `planning` on `servers`.
fn sorted(
    servers: &Runtime,
    planning: &SessionContext,
    table: Arc<dyn TableProvider>,
) -> Result<Vec<RecordBatch>, DataFusionError> {
    servers.block_on(async {
        planning
            .read_table(table)?
            .sort(vec![col("n").sort(true, false)])?
            .collect()
            .await
    })
}

/// A split leg of `orders` read across this planning node, a combining-only worker, a reader of
/// `payments` and two readers of `orders`, the leg's readers declared as `declared` names them.
fn split_across(declared: impl FnOnce(&[Peer; 2]) -> Vec<Url>) -> Across {
    let servers = servers();
    let authority = Authority::generate();
    let node = authority.sign();
    let own = Nodes::worker(roomy(), authority.identity(&node)).expect("a worker builds");
    let own_url = served(&servers, listening(&node, authority.peers()), own.clone(), Arc::default());
    let combining = Peer::serve(&servers, &authority, false);
    let elsewhere = Peer::serve(&servers, &authority, true);
    let orders = [
        Peer::serve(&servers, &authority, true),
        Peer::serve(&servers, &authority, true),
    ];
    let others = [&combining, &elsewhere, &orders[0], &orders[1]].map(|peer| peer.url.clone());
    let planning = Nodes::declared(own, own_url, others, authority.identity(&node))
        .reading(source("orders"), declared(&orders))
        .reading(source("payments"), [elsewhere.url.clone()])
        .session(roomy())
        .expect("a session builds");
    let leg = SplitScan::granted(
        b"orders-leg".to_vec(),
        source("orders"),
        schema(),
        partitions(),
        &Subject::verified("user@example.com").expect("a test caller is a subject"),
        SystemTime::now() + Duration::from_secs(3600),
        StreamBound::new(1000, 1 << 20),
    )
    .expect("a split leg is granted");
    let answer = sorted(&servers, &planning, Arc::new(leg));
    Across {
        answer,
        combining,
        elsewhere,
        orders,
    }
}

fn values(batches: &[RecordBatch]) -> Vec<i64> {
    batches
        .iter()
        .flat_map(|batch| {
            batch
                .column(0)
                .as_any()
                .downcast_ref::<Int64Array>()
                .expect("`n` is an Int64 column")
                .values()
                .to_vec()
        })
        .collect()
}

fn both_readers(orders: &[Peer; 2]) -> Vec<Url> {
    orders.iter().map(|peer| peer.url.clone()).collect()
}

#[test]
fn a_split_leg_read_on_the_nodes_reading_its_source_answers_what_the_whole_leg_answers() {
    let across = split_across(both_readers);
    let whole = MemTable::try_new(
        schema(),
        partitions()
            .iter()
            .map(|partition| vec![rows(partition).expect("a partition's rows build")])
            .collect(),
    )
    .expect("a leg's partitions are a table");
    let whole = sorted(&servers(), &SessionContext::new(), Arc::new(whole)).expect("the whole leg answers");

    let split = across.answer.expect("a split leg read across its readers answered");
    assert_eq!(values(&split), values(&whole));
}

#[test]
fn a_split_leg_s_partitions_are_sent_only_to_the_nodes_reading_its_source_each_once() {
    let across = split_across(both_readers);

    assert_eq!(
        across.combining.sent(),
        0,
        "a combining-only node was sent a split leg's task"
    );
    assert_eq!(
        across.elsewhere.sent(),
        0,
        "a node reading another source was sent a split leg's task"
    );
    assert_eq!(
        across.elsewhere.asked(),
        Vec::<Vec<u8>>::new(),
        "a node reading another source read a split leg's partition"
    );
    assert!(
        across.orders.iter().all(|peer| peer.sent() > 0),
        "the leg's tasks were not dealt across both of its readers"
    );
    let mut asked: Vec<Vec<u8>> = across.orders.iter().flat_map(Peer::asked).collect();
    asked.sort();
    assert_eq!(
        asked,
        partitions(),
        "a partition was read other than once by the leg's readers"
    );
    across.answer.expect("a split leg read across its readers answered");
}

/// Six partitions over two readers: as one task each, the first reads the even partitions and the
/// second the odd ones. Dealt a task per node instead, the first reader would also read partition 5.
#[test]
fn a_split_leg_with_more_partitions_than_readers_sends_each_reader_one_task() {
    let across = split_across(both_readers);

    let shares = across
        .orders
        .iter()
        .map(|peer| {
            let mut asked = peer.asked();
            asked.sort();
            asked
        })
        .collect::<Vec<_>>();
    assert_eq!(
        shares,
        [vec![vec![0], vec![2], vec![4]], vec![vec![1], vec![3], vec![5]]],
        "a reader ran more than one task of the leg's stage"
    );
    let whole: Vec<i64> = partitions()
        .iter()
        .flat_map(|partition| values(&[rows(partition).expect("a partition's rows build")]))
        .collect();
    let split = across.answer.expect("a split leg read across its readers answered");
    assert_eq!(values(&split), whole);
}

#[test]
fn a_split_leg_no_resolved_node_reads_is_refused_and_sent_nowhere() {
    let across = split_across(|_| vec![Url::parse("https://127.0.0.1:1").expect("an address is a URL")]);

    let refused = across.answer.expect_err("a split leg no resolved node reads was answered");
    assert!(
        matches!(
            refused.find_root(),
            DataFusionError::External(cause)
                if cause.downcast_ref::<NoReadingNode>().is_some_and(|refusal| refusal.aliases == [source("orders")])
        ),
        "{refused}"
    );
    let sent: usize = [&across.combining, &across.elsewhere, &across.orders[0], &across.orders[1]]
        .into_iter()
        .map(Peer::sent)
        .sum();
    assert_eq!(sent, 0, "a split leg no node reads sent a task");
}
