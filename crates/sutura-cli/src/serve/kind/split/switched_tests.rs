//! Whom a granted partition is read as when its grant names a switched principal: `ClickHouse`
//! reads it as that principal, every other kind refuses. No driver is loaded: each cell ends at the
//! identity guard or at the first step after it, which fails on the bucket descriptor.
//!
//! **Limit:** Oracle's refusal has no cell here, because an Oracle source opens its connection when
//! it is built; its call to [`refuse_switched`] is held by review and the shared cell below.

use core::num::NonZeroUsize;
use core::time::Duration;
use std::sync::Arc;
use std::time::Instant;

use sutura_domain::definitions::DefinitionDigest;
use sutura_domain::identity::{PrincipalName, Secret};
use sutura_domain::model::{ColumnName, DimensionName, MetricName, SourceName, TableName};
use sutura_domain::pinned::MetricDigest;
use sutura_domain::plan::{LegPlan, PlanBindings, PlanColumn, PlanKey, ResultLabel};
use sutura_domain::source::{AcknowledgementReason, HashBucketsDeclared, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::ResultBudget;
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_exec_bigquery::adbc::DriverLocation;
use sutura_exec_clickhouse::adbc::AdbcError;
use sutura_exec_datafusion::{SchemaRef, TokenUnread, Unread};

use super::{PartitionReads, SplitKind as _, read_switched, refuse_switched};
use crate::serve::ClickHouseSource;

const SERVICE_USER: &str = "sutura_worker_service";

fn ledger() -> SourceName {
    SourceName::parse("ledger").expect("a test source is a source")
}

fn lookup() -> LegPlan {
    let table = TableName::parse("accounts").expect("a test name is a name");
    LegPlan::Lookup {
        source: ledger(),
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

fn deadline() -> Deadline {
    Deadline::opened_at(Instant::now(), Budget::parse(Duration::from_secs(30)).expect("a budget"))
}

fn granted() -> SchemaRef {
    Arc::new(arrow_schema::Schema::empty())
}

fn alice() -> PrincipalName {
    PrincipalName::parse("alice").expect("a test principal is a principal")
}

fn shared() -> SourcePosture {
    SourcePosture::SharedServiceUser {
        declared: SharedIdentityDeclared::of(AcknowledgementReason::parse("a test source").expect("a test reason is a reason")),
    }
}

fn clickhouse(posture: SourcePosture) -> ClickHouseSource {
    ClickHouseSource::new(
        ledger(),
        posture,
        DriverLocation::parse("/nonexistent/libadbc_driver_clickhouse.so").expect("an absolute driver path"),
        String::from("http://127.0.0.1:1"),
        String::from(SERVICE_USER),
        Secret::new(String::from("unused")),
        None,
        ResultBudget::of_bytes(NonZeroUsize::MIN),
    )
    .with_hash_buckets(&HashBucketsDeclared::of(
        AcknowledgementReason::written_under("hash_buckets", "the fixture is immutable").expect("a reason"),
    ))
}

/// Whether `read` failed with an `E` that `is` the one asked for, which is how a cell tells which
/// guard answered.
fn failed_with<T, E>(read: &Result<T, Unread>, is: impl FnOnce(&E) -> bool) -> bool
where
    E: core::error::Error + 'static,
{
    let Err(Unread::Failed(error)) = read else {
        return false;
    };
    core::error::Error::source(error)
        .and_then(|cause| cause.downcast_ref::<E>())
        .is_some_and(is)
}

const fn is_descriptor(error: &AdbcError) -> bool {
    matches!(error, AdbcError::BucketDescriptor(_))
}

const fn is_disagreement(error: &AdbcError) -> bool {
    matches!(error, AdbcError::PartitionIdentityDisagrees)
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the cell asks the kind's own read, which never reaches a driver"
)]
fn a_switched_grant_is_read_as_the_principal_it_names_at_an_impersonating_source() {
    let source = clickhouse(SourcePosture::ImpersonationAtSource);
    let read = source.read_granted(
        &lookup(),
        b"",
        &granted(),
        &PartitionReads::SwitchedPrincipal(alice()),
        deadline(),
    );
    assert!(
        failed_with::<_, AdbcError>(&read, is_descriptor),
        "the identity guard passed and the read stopped at the descriptor: {read:?}"
    );
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the cell asks the kind's own read, which never reaches a driver"
)]
fn a_switched_grant_at_a_shared_service_source_is_refused_as_disagreeing_with_its_posture() {
    let source = clickhouse(shared());
    let read = source.read_granted(
        &lookup(),
        b"",
        &granted(),
        &PartitionReads::SwitchedPrincipal(alice()),
        deadline(),
    );
    assert!(failed_with::<_, AdbcError>(&read, is_disagreement), "{read:?}");
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the cell asks the kind's own read, which never reaches a driver"
)]
fn a_service_user_grant_at_an_impersonating_source_is_refused_as_disagreeing_with_its_posture() {
    let source = clickhouse(SourcePosture::ImpersonationAtSource);
    let read = source.read_granted(&lookup(), b"", &granted(), &PartitionReads::ServiceUser, deadline());
    assert!(failed_with::<_, AdbcError>(&read, is_disagreement), "{read:?}");
}

#[test]
fn the_principal_a_worker_is_told_to_switch_to_reaches_the_adapter_and_is_never_dropped() {
    let shared = clickhouse(shared());
    let read = read_switched(&shared, &lookup(), b"", &granted(), alice(), deadline());
    assert!(
        failed_with::<_, AdbcError>(&read, is_disagreement),
        "a switched read at a shared-service source must not become the service user's: {read:?}"
    );
    let impersonating = clickhouse(SourcePosture::ImpersonationAtSource);
    let read = read_switched(&impersonating, &lookup(), b"", &granted(), alice(), deadline());
    assert!(failed_with::<_, AdbcError>(&read, is_descriptor), "{read:?}");
}

#[test]
fn a_kind_that_cannot_switch_refuses_a_switched_principal_and_names_the_source() {
    let refused = refuse_switched(&lookup(), &PartitionReads::SwitchedPrincipal(alice()));
    assert!(
        failed_with::<(), TokenUnread>(&refused, |cause| matches!(cause, TokenUnread::NotSwitched { .. })),
        "{refused:?}"
    );
    assert!(format!("{refused:?}").contains("ledger"), "{refused:?}");
    assert!(matches!(refuse_switched(&lookup(), &PartitionReads::ServiceUser), Ok(())));
}

#[cfg(feature = "postgres")]
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the cell asks the kind's own read, which refuses before any driver"
)]
fn a_postgres_partition_refuses_a_switched_principal_before_it_reads() {
    use sutura_exec_postgres::adbc::{AdbcPostgres, Channel, Conninfo, PostgresDriver};
    use sutura_exec_postgres::connection::ConnectionTarget;

    let conninfo = Conninfo::new(
        &ledger(),
        ConnectionTarget::Host("localhost"),
        5432,
        "database",
        "user",
        &Secret::new(String::from("p")),
        Channel::Plaintext,
    )
    .expect("plaintext builds");
    let postgres = AdbcPostgres::new(
        ledger(),
        SourcePosture::ImpersonationAtSource,
        PostgresDriver::parse("/nonexistent/libadbc_driver_postgresql.so").expect("an absolute driver path"),
        conninfo,
    )
    .expect("the default timeout ceiling");
    let read = postgres.read_granted(
        &lookup(),
        b"",
        &granted(),
        &PartitionReads::SwitchedPrincipal(alice()),
        deadline(),
    );
    assert!(
        failed_with::<_, TokenUnread>(&read, |cause| matches!(cause, TokenUnread::NotSwitched { .. })),
        "{read:?}"
    );
}
