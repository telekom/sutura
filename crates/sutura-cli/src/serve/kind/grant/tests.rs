//! [`granted_or_read`] over a fake split: what it grants, what it reads here, and how long what the
//! split holds open lives.

use core::convert::Infallible;
use core::num::NonZeroU32;
use core::time::Duration;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Instant, SystemTime};

use sutura_domain::definitions::DefinitionDigest;
use sutura_domain::identity::{Agreed, Expiry, LegCredentials, Minted, Presented, Secret, SourceSet, Subject};
use sutura_domain::model::{ColumnName, DimensionName, MetricName, SourceName, TableName};
use sutura_domain::pinned::MetricDigest;
use sutura_domain::plan::{Grantee, LegPlan, NotGranted, Partitioned, PlanBindings, PlanColumn, PlanKey, ResultLabel};
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::estimate::EstimatedBytes;
use sutura_domain::warehouse::{ResultBatches, Value};
use sutura_exec_datafusion::SchemaRef;

use super::{HeldSplit, granted_or_read};

/// A split of `partitions`, each portable or not, recording each describe and read, and `open`
/// for as long as it lives.
struct Fake {
    partitions: usize,
    portable: bool,
    schema: Option<SchemaRef>,
    described: Arc<AtomicUsize>,
    read: Arc<AtomicUsize>,
    open: Arc<AtomicBool>,
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.open.store(false, Ordering::SeqCst);
    }
}

impl HeldSplit for Fake {
    type Error = Infallible;

    fn descriptors(&self) -> Vec<Option<Vec<u8>>> {
        (0..self.partitions)
            .map(|index| {
                self.portable
                    .then(|| vec![u8::try_from(index).expect("a fake has few partitions")])
            })
            .collect()
    }

    fn columns(&mut self, _deadline: Deadline) -> Result<Option<SchemaRef>, Infallible> {
        self.described.fetch_add(1, Ordering::SeqCst);
        Ok(self.schema.clone())
    }

    fn estimate(&self) -> EstimatedBytes {
        EstimatedBytes::parse(64)
    }

    fn read(&self, index: usize, _deadline: Deadline) -> Option<Result<ResultBatches, Infallible>> {
        (index < self.partitions).then(|| {
            self.read.fetch_add(1, Ordering::SeqCst);
            Ok(rows())
        })
    }
}

/// What a test reads off a [`Fake`] after handing it over.
struct Seen {
    described: Arc<AtomicUsize>,
    read: Arc<AtomicUsize>,
    open: Arc<AtomicBool>,
}

fn rows() -> ResultBatches {
    sutura_domain::warehouse::arrow::of_rows(&[String::from("k")], &[vec![Value::Integer(1)]]).expect("one row is a result")
}

fn fake(partitions: usize, portable: bool, schema: Option<SchemaRef>) -> (Fake, Seen) {
    let seen = Seen {
        described: Arc::default(),
        read: Arc::default(),
        open: Arc::new(AtomicBool::new(true)),
    };
    let split = Fake {
        partitions,
        portable,
        schema,
        described: Arc::clone(&seen.described),
        read: Arc::clone(&seen.read),
        open: Arc::clone(&seen.open),
    };
    (split, seen)
}

fn schema() -> SchemaRef {
    Arc::clone(rows().schema())
}

fn geo() -> SourceName {
    SourceName::parse("geo").expect("a test source is a source")
}

fn lookup() -> LegPlan {
    let table = TableName::parse("paged").expect("a test name is a name");
    LegPlan::Lookup {
        source: geo(),
        metric: MetricDigest::new(
            MetricName::parse("amount_total").expect("a test name is a name"),
            DefinitionDigest::parse("a".repeat(64)).expect("a digest"),
        ),
        table: table.clone().into(),
        keys: vec![PlanKey::new(
            ResultLabel::dimension(&DimensionName::parse("k").expect("a test name is a name")),
            PlanColumn::new(table, ColumnName::parse("k").expect("a test name is a name")),
        )],
        bindings: PlanBindings::none(),
    }
}

/// A grantee for a verified caller, its grant ending `ends` from now.
fn grantee(ends: Duration) -> Grantee {
    let subject = Subject::verified("user@example.com").expect("a test caller is a subject");
    let sources = SourceSet::of(geo());
    let presented = BTreeMap::from([(
        geo(),
        Presented::SubjectToken {
            material: Secret::new("a-test-token"),
        },
    )]);
    let credentials = LegCredentials::minted(subject.clone(), Expiry::NothingExpires, &sources, presented)
        .expect("one leg covers its one source");
    let Ok(Agreed::Granted { credentials }) = (Minted::Granted { credentials }).agreeing_with(&subject, &sources, 0) else {
        panic!("a fixture grant agrees with its own request");
    };
    Grantee::new(&lookup(), &credentials, SystemTime::now() + ends).expect("a verified caller's leg is offered")
}

fn deadline() -> Deadline {
    Deadline::opened_at(Instant::now(), Budget::parse(Duration::from_secs(30)).expect("a budget"))
}

fn rows_per_stream() -> NonZeroU32 {
    NonZeroU32::new(1000).expect("a literal bound is not zero")
}

#[test]
fn a_split_of_two_partitions_is_granted_and_its_source_stays_open_until_the_grant_is_dropped() {
    let (split, seen) = fake(2, true, Some(schema()));

    let answered = granted_or_read(
        split,
        Some(grantee(Duration::from_secs(30))),
        deadline(),
        rows_per_stream(),
        SystemTime::now,
    );

    let Ok(Partitioned::Granted(granted)) = answered else {
        panic!("a portable split of two partitions with one schema is granted");
    };
    assert_eq!(
        granted.partitions().iter().cloned().collect::<Vec<_>>(),
        vec![vec![0], vec![1]],
        "the grant carries each partition's own descriptor, in page order"
    );
    assert!(
        seen.open.load(Ordering::SeqCst),
        "the split is held open while its grant lives"
    );
    drop(granted);
    assert!(!seen.open.load(Ordering::SeqCst), "the split is released with its grant");
    assert_eq!(seen.read.load(Ordering::SeqCst), 0, "a granted split is not read here");
}

#[test]
fn a_split_of_one_partition_is_read_here_and_neither_described_nor_granted() {
    let (split, seen) = fake(1, true, Some(schema()));

    let answered = granted_or_read(
        split,
        Some(grantee(Duration::from_secs(30))),
        deadline(),
        rows_per_stream(),
        SystemTime::now,
    );

    let Ok(Partitioned::Split(parts)) = answered else {
        panic!("a split of one partition is read here, as before a leg could be granted");
    };
    assert_eq!(parts.count(), 1);
    assert_eq!(seen.read.load(Ordering::SeqCst), 1, "its one partition is read here");
    assert_eq!(seen.described.load(Ordering::SeqCst), 0, "a split read here is not described");
}

#[test]
fn a_split_whose_columns_have_no_one_schema_is_read_here() {
    let (split, seen) = fake(2, true, None);

    let answered = granted_or_read(
        split,
        Some(grantee(Duration::from_secs(30))),
        deadline(),
        rows_per_stream(),
        SystemTime::now,
    );

    let Ok(Partitioned::Split(parts)) = answered else {
        panic!("a split with no one schema is read here");
    };
    assert_eq!(parts.count(), 2);
    assert_eq!(seen.read.load(Ordering::SeqCst), 2);
}

#[test]
fn a_split_with_a_partition_no_other_node_can_read_is_refused_and_not_read_here() {
    let (split, seen) = fake(2, false, Some(schema()));

    let answered = granted_or_read(
        split,
        Some(grantee(Duration::from_secs(30))),
        deadline(),
        rows_per_stream(),
        SystemTime::now,
    );

    assert!(matches!(answered, Ok(Partitioned::NotGranted(NotGranted::NotPortable))));
    assert_eq!(seen.read.load(Ordering::SeqCst), 0);
    assert!(!seen.open.load(Ordering::SeqCst), "a refused split is released at once");
}

#[test]
fn a_grant_is_checked_against_the_instant_after_the_describe_not_the_one_it_was_offered_at() {
    let (split, _seen) = fake(2, true, Some(schema()));
    let offered = grantee(Duration::from_secs(1));

    let answered = granted_or_read(split, Some(offered), deadline(), rows_per_stream(), || {
        SystemTime::now() + Duration::from_secs(2)
    });

    assert!(matches!(answered, Ok(Partitioned::NotGranted(NotGranted::Expired(_)))));
}

#[test]
fn a_split_offered_no_grantee_is_read_here() {
    let (split, seen) = fake(2, true, Some(schema()));

    let answered = granted_or_read(split, None, deadline(), rows_per_stream(), SystemTime::now);

    let Ok(Partitioned::Split(parts)) = answered else {
        panic!("a split with no grantee is read here");
    };
    assert_eq!(parts.count(), 2);
    assert_eq!(
        seen.described.load(Ordering::SeqCst),
        0,
        "a split with no grantee is not described"
    );
}
