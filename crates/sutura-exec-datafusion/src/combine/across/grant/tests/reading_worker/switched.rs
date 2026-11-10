//! A reading worker's read of a partition granted as the caller's principal: only the principal its
//! grant names, masked as its grant's caller, reaches the reader, and only once.

use std::sync::Arc;

use datafusion::arrow::datatypes::SchemaRef;
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::error::DataFusionError;
use datafusion::execution::SendableRecordBatchStream;
use datafusion::physical_plan::stream::RecordBatchStreamAdapter;
use futures_util::stream;
use sutura_domain::model::SourceName;

use super::super::super::super::partition::{PartitionReader, PartitionScanExec, SwitchedPrincipal, Unread};
use super::super::super::reading::{
    Expiry, LegDigest, MaskedCaller, Partition, ReadingGrant, ReadingKey, StreamBound, encode, mint,
};
use super::{executed, granted, installed_and_run, refused_as_not_naming, schema, source};

fn switched(name: &str) -> SwitchedPrincipal {
    SwitchedPrincipal::new(sutura_domain::identity::PrincipalName::parse(name).expect("a test principal is a principal"))
}

/// A grant naming `wire`/`source()`/`partition` for the caller `caller`, read as `principal`.
pub(super) fn granted_switching(key: &ReadingKey, wire: &[u8], partition: &[u8], caller: &str, principal: &str) -> Vec<u8> {
    let built = ReadingGrant::new(
        LegDigest::of(wire),
        source(),
        Partition::of(partition.to_vec()),
        Expiry::at(core::time::Duration::from_secs(4_000_000_000)),
        MaskedCaller::of(&sutura_domain::identity::SubjectKey::parse(caller).expect("a test caller is a subject")),
        StreamBound {
            rows: 1000,
            bytes: 1 << 20,
        },
    )
    .switching(&switched(principal));
    let (built, signature) = mint(key, built);
    encode(&built, &signature)
}

/// Each principal a [`Switches`] reader was handed.
type Switched = Arc<parking_lot::Mutex<Vec<String>>>;

/// A reader that reads only as a principal, each one it was handed logged in its [`Switched`].
struct Switches(Switched);

impl PartitionReader for Switches {
    fn read(
        &self,
        _wire: &[u8],
        _source: &SourceName,
        _partition: &[u8],
        _schema: &SchemaRef,
    ) -> Result<SendableRecordBatchStream, Unread> {
        Err(Unread::Failed(DataFusionError::Execution(String::from(
            "read as no principal",
        ))))
    }

    fn read_switched(
        &self,
        _wire: &[u8],
        _source: &SourceName,
        _partition: &[u8],
        schema: &SchemaRef,
        switched: SwitchedPrincipal,
    ) -> Result<SendableRecordBatchStream, Unread> {
        self.0.lock().push(String::from(switched.name().as_str()));
        Ok(Box::pin(RecordBatchStreamAdapter::new(
            Arc::clone(schema),
            stream::empty::<Result<RecordBatch, DataFusionError>>(),
        )))
    }
}

#[test]
fn a_granted_partition_read_hands_its_reader_the_principal_its_grant_names() {
    let (wire, partition, key_bytes) = (b"wire-switched".to_vec(), b"partition-switched".to_vec(), [18_u8; 32]);
    let grant = granted_switching(
        &ReadingKey::of(key_bytes),
        &wire,
        &partition,
        "user@example.com",
        "user@example.com",
    );
    let handed = Switched::default();
    let leaf = PartitionScanExec::new(wire, source(), partition, key_bytes, Some(grant), schema())
        .switching(switched("user@example.com"));

    let failures = installed_and_run(
        Some(Arc::new(Switches(Arc::clone(&handed)))),
        leaf,
        "eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee",
    );

    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(*handed.lock(), vec![String::from("user@example.com")]);
}

#[test]
fn a_partition_read_carrying_a_principal_its_grant_does_not_name_is_refused() {
    let (wire, partition, key_bytes) = (b"wire-reswitched".to_vec(), b"partition-reswitched".to_vec(), [19_u8; 32]);
    let grant = granted_switching(
        &ReadingKey::of(key_bytes),
        &wire,
        &partition,
        "user@example.com",
        "user@example.com",
    );
    let handed = Switched::default();
    let leaf = PartitionScanExec::new(wire, source(), partition, key_bytes, Some(grant), schema())
        .switching(switched("other@example.com"));

    let failures = installed_and_run(
        Some(Arc::new(Switches(Arc::clone(&handed)))),
        leaf,
        "ffffffff-ffff-ffff-ffff-ffffffffffff",
    );

    assert!(refused_as_not_naming(&failures), "{failures:?}");
    assert!(
        handed.lock().is_empty(),
        "a reader was handed a principal its grant does not name"
    );
}

#[test]
fn a_partition_read_carrying_a_principal_where_its_grant_names_none_is_refused() {
    let (wire, partition, key_bytes) = (
        b"wire-added-principal".to_vec(),
        b"partition-added-principal".to_vec(),
        [21_u8; 32],
    );
    let grant = granted(&ReadingKey::of(key_bytes), &wire, &partition);
    let handed = Switched::default();
    let leaf = PartitionScanExec::new(wire, source(), partition, key_bytes, Some(grant), schema())
        .switching(switched("user@example.com"));

    let failures = installed_and_run(
        Some(Arc::new(Switches(Arc::clone(&handed)))),
        leaf,
        "13131313-1313-1313-1313-131313131313",
    );

    assert!(refused_as_not_naming(&failures), "{failures:?}");
    assert!(handed.lock().is_empty(), "a reader was handed a principal no grant names");
}

#[test]
fn a_partition_read_as_a_principal_masked_unlike_its_grants_caller_is_refused() {
    let (wire, partition, key_bytes) = (b"wire-unmasked".to_vec(), b"partition-unmasked".to_vec(), [22_u8; 32]);
    let grant = granted_switching(
        &ReadingKey::of(key_bytes),
        &wire,
        &partition,
        "someone-else@example.org",
        "user@example.com",
    );
    let handed = Switched::default();
    let leaf = PartitionScanExec::new(wire, source(), partition, key_bytes, Some(grant), schema())
        .switching(switched("user@example.com"));

    let failures = installed_and_run(
        Some(Arc::new(Switches(Arc::clone(&handed)))),
        leaf,
        "14141414-1414-1414-1414-141414141414",
    );

    assert!(refused_as_not_naming(&failures), "{failures:?}");
    assert!(
        handed.lock().is_empty(),
        "a reader was handed a principal that is not its grant's caller"
    );
}

#[test]
fn a_reader_whose_sources_switch_to_no_principal_refuses_a_partition_granted_as_one() {
    let (wire, partition, key_bytes) = (b"wire-noswitch".to_vec(), b"partition-noswitch".to_vec(), [23_u8; 32]);
    let grant = granted_switching(
        &ReadingKey::of(key_bytes),
        &wire,
        &partition,
        "user@example.com",
        "user@example.com",
    );
    let reader: Arc<dyn PartitionReader> = Arc::new(|_: &SourceName, _: &[u8], schema: &SchemaRef| {
        Ok(Box::pin(RecordBatchStreamAdapter::new(
            Arc::clone(schema),
            stream::empty::<Result<RecordBatch, DataFusionError>>(),
        )) as SendableRecordBatchStream)
    });
    let leaf = PartitionScanExec::new(wire, source(), partition, key_bytes, Some(grant), schema())
        .switching(switched("user@example.com"));

    let failures = installed_and_run(Some(reader), leaf, "15151515-1515-1515-1515-151515151515");

    assert!(
        failures.iter().any(|failure| failure
            .to_string()
            .contains("reads no partition of `orders` as a caller's principal")),
        "{failures:?}"
    );
}

#[test]
fn a_partition_scan_hands_out_its_principal_once() {
    let handed = Switched::default();
    let leaf = PartitionScanExec::new(
        b"wire-principal-once".to_vec(),
        source(),
        b"partition-principal-once".to_vec(),
        [24_u8; 32],
        None,
        schema(),
    )
    .switching(switched("user@example.com"));

    let answered = executed(&leaf, Arc::new(Switches(Arc::clone(&handed))), 2);

    assert_eq!(*handed.lock(), vec![String::from("user@example.com")]);
    assert!(answered.first().is_some_and(Result::is_ok), "{answered:?}");
    assert!(
        matches!(
            answered.get(1),
            Some(Err(DataFusionError::Execution(refused))) if refused.contains("hands out its caller's principal once")
        ),
        "a second read of one scan was not refused for its principal being taken: {answered:?}"
    );
}

#[test]
fn a_partition_scan_shows_whether_it_holds_a_principal_and_never_the_name() {
    let leaf = PartitionScanExec::new(
        b"wire-principal-shown".to_vec(),
        source(),
        b"partition-principal-shown".to_vec(),
        [25_u8; 32],
        None,
        schema(),
    )
    .switching(switched("user@example.com"));

    let debug = format!("{leaf:?}");
    let display = datafusion::physical_plan::displayable(&leaf).one_line().to_string();

    assert!(
        debug.contains(r#"principal: "held""#) && display.contains("principal=held"),
        "{debug} {display}"
    );
    assert!(
        !debug.contains("user@example.com") && !display.contains("user@example.com"),
        "{debug} {display}"
    );
}
