//! A split lookup leg's ambiguous-link check planned across four localhost nodes - this planning
//! node, a reader of another source and two readers of the leg's own - each partition read by a task
//! on a node reading the leg's source, every link value counted after a repartition on it.
//!
//! # What this does not exercise
//!
//! The leg is a `SplitScan` built here and checked by a direct call: no served question builds one or
//! calls the check. Its readers answer from memory rather than a source, and every node shares one
//! authority and one runtime.

use core::time::Duration;
use std::sync::Arc;
use std::time::SystemTime;

use datafusion::arrow::array::Int64Array;
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::error::DataFusionError;
use datafusion::execution::SendableRecordBatchStream;
use datafusion::physical_plan::stream::RecordBatchStreamAdapter;
use futures_util::stream;
use parking_lot::Mutex;
use sutura_domain::identity::SubjectKey;
use sutura_domain::model::SourceName;
use tokio::runtime::Runtime;
use url::Url;

use super::{Authority, listening, roomy, served, servers};
use crate::combine::{CombineError, Nodes, PartitionReader, SplitScan, StreamBound, refuse_ambiguous_split};

/// More partitions than there are nodes, so the engine repartitions them by link value rather than
/// counting each where it was read.
const PARTITIONS: u8 = 6;

/// A ceiling no check here comes near.
const CEILING: u64 = 1 << 30;

fn schema() -> SchemaRef {
    Arc::new(Schema::new(vec![Field::new("link", DataType::Int64, false)]))
}

fn source(name: &str) -> SourceName {
    SourceName::parse(name).expect("a test source is a source")
}

fn partitions() -> Vec<Vec<u8>> {
    (0..PARTITIONS).map(|index| vec![index]).collect()
}

/// The link values a partition holds, by its index.
type Values = fn(u8) -> Vec<i64>;

/// Partition `j`'s link values: `10j + 1` and `10j + 2`, none repeated anywhere in the leg.
fn distinct(partition: u8) -> Vec<i64> {
    let first = i64::from(partition) * 10;
    vec![first + 1, first + 2]
}

/// [`distinct`], but partition 1 holds `1`, which partition 0 holds too: each holds it once.
fn across(partition: u8) -> Vec<i64> {
    if partition == 1 { vec![11, 1] } else { distinct(partition) }
}

/// [`distinct`], but partition 2 holds `21` twice.
fn inside(partition: u8) -> Vec<i64> {
    if partition == 2 { vec![21, 21] } else { distinct(partition) }
}

/// Every partition a reader was asked for, in the order it was asked.
type Asked = Arc<Mutex<Vec<Vec<u8>>>>;

/// A reader answering every partition it is asked for with its `values`, each ask logged in `asked`.
fn reader(values: Values, asked: Asked) -> Arc<dyn PartitionReader> {
    Arc::new(
        move |_: &SourceName, partition: &[u8], schema: &SchemaRef| -> Result<SendableRecordBatchStream, DataFusionError> {
            asked.lock().push(partition.to_vec());
            let links: Vec<i64> = partition.iter().flat_map(|&index| values(index)).collect();
            let batch = RecordBatch::try_new(Arc::clone(schema), vec![Arc::new(Int64Array::from(links))])?;
            Ok(Box::pin(RecordBatchStreamAdapter::new(
                Arc::clone(schema),
                stream::iter(vec![Ok::<_, DataFusionError>(batch)]),
            )))
        },
    )
}

/// A reading node signed by `authority`, served on `servers`, every partition it read logged in
/// `asked`.
fn reading(servers: &Runtime, authority: &Authority, values: Values, asked: &Asked) -> Url {
    let signed = authority.sign();
    let worker = Nodes::worker_reading(roomy(), authority.identity(&signed), Some(reader(values, Arc::clone(asked))))
        .expect("a worker builds");
    served(servers, listening(&signed, authority.peers()), worker, Arc::default())
}

/// What a split leg of `orders` was checked to, and every partition each other node read.
struct Checked {
    answer: Result<(), CombineError>,
    elsewhere: Asked,
    orders: [Asked; 2],
}

/// A split leg of `orders` whose partitions hold `values`, checked across this planning node, a
/// reader of `payments` and two readers of `orders`.
fn checked(values: Values) -> Checked {
    let servers = servers();
    let authority = Authority::generate();
    let node = authority.sign();
    let own = Nodes::worker(roomy(), authority.identity(&node)).expect("a worker builds");
    let own_url = served(&servers, listening(&node, authority.peers()), own.clone(), Arc::default());
    let elsewhere = Asked::default();
    let orders = [Asked::default(), Asked::default()];
    let elsewhere_url = reading(&servers, &authority, values, &elsewhere);
    let orders_urls = orders.each_ref().map(|asked| reading(&servers, &authority, values, asked));
    let others = orders_urls.iter().cloned().chain([elsewhere_url.clone()]);
    let nodes = Nodes::declared(own, own_url, others, authority.identity(&node))
        .reading(source("orders"), orders_urls)
        .reading(source("payments"), [elsewhere_url]);
    let leg = SplitScan::granted(
        b"orders-leg".to_vec(),
        source("orders"),
        schema(),
        partitions(),
        &SubjectKey::parse("user@example.com").expect("a test caller is a subject"),
        SystemTime::now() + Duration::from_secs(3600),
        StreamBound::new(1000, 1 << 20),
    )
    .expect("a split leg is granted");
    let answer = servers.block_on(refuse_ambiguous_split(&nodes, leg, &["link"], CEILING));
    Checked {
        answer,
        elsewhere,
        orders,
    }
}

/// Partitions 0 and 1 are read by different tasks on different readers, and each holds `1` once.
/// Counted per partition, or per task, neither holds it twice.
#[test]
fn a_link_value_held_once_by_each_of_two_partitions_of_a_split_leg_is_refused_as_ambiguous() {
    let checked = checked(across);

    assert!(
        matches!(checked.answer, Err(CombineError::AmbiguousLink)),
        "a link value two partitions each held once was not refused: {:?}",
        checked.answer
    );
}

#[test]
fn a_link_value_held_twice_by_one_partition_of_a_split_leg_is_refused_as_ambiguous() {
    let checked = checked(inside);

    assert!(
        matches!(checked.answer, Err(CombineError::AmbiguousLink)),
        "a link value one partition held twice was not refused: {:?}",
        checked.answer
    );
}

#[test]
fn a_split_leg_with_no_repeated_link_value_passes_read_only_on_its_source_s_readers() {
    let checked = checked(distinct);

    assert_eq!(
        checked.elsewhere.lock().clone(),
        Vec::<Vec<u8>>::new(),
        "a node reading another source read a split leg's partition"
    );
    let mut asked: Vec<Vec<u8>> = checked.orders.iter().flat_map(|asked| asked.lock().clone()).collect();
    asked.sort();
    assert_eq!(
        asked,
        partitions(),
        "a partition was read other than once by the leg's readers"
    );
    checked.answer.expect("a split leg with no repeated link value was refused");
}
