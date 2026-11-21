//! A non-switching kind refuses the granted identity before its descriptor or driver is read.

use sutura_domain::identity::{PrincipalName, Secret};
use sutura_domain::source::SourcePosture;
use sutura_exec_datafusion::{TokenUnread, Unread};
use sutura_exec_postgres::PostgresError;
use sutura_exec_postgres::adbc::{AdbcPostgres, Channel, Conninfo, PostgresDriver};
use sutura_exec_postgres::connection::ConnectionTarget;

use crate::serve::kind::split::{PartitionReads, SplitKind as _};

fn postgres() -> AdbcPostgres {
    let conninfo = Conninfo::new(
        &super::geo(),
        ConnectionTarget::Host("localhost"),
        5432,
        "database",
        "user",
        &Secret::new(String::from("p")),
        Channel::Plaintext,
    )
    .expect("plaintext builds");
    AdbcPostgres::new(
        super::geo(),
        SourcePosture::ImpersonationAtSource,
        PostgresDriver::parse("/nonexistent/libadbc_driver_postgresql.so").expect("an absolute driver path"),
        conninfo,
    )
    .expect("the default timeout ceiling")
}

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

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the cell asks the real non-switching kind's identity guard before its descriptor or driver"
)]
fn a_kind_that_cannot_switch_refuses_a_switched_principal_and_names_the_source() {
    let postgres = postgres();
    let switched = PartitionReads::SwitchedPrincipal(PrincipalName::parse("alice").expect("a principal"));
    let refused = postgres.read_granted(&super::lookup(), b"", &super::schema(), &switched, super::deadline());
    assert!(
        failed_with::<_, TokenUnread>(&refused, |cause| cause.to_string()
            == format!(
                "this reading worker reads no partition of `{}` as a caller's switched principal",
                super::geo()
            )),
        "the kind must refuse the switched identity and name its source: {refused:?}"
    );
    let service = postgres.read_granted(
        &super::lookup(),
        b"",
        &super::schema(),
        &PartitionReads::ServiceUser,
        super::deadline(),
    );
    assert!(
        failed_with::<_, PostgresError>(&service, |cause| matches!(cause, PostgresError::MalformedDescriptor { .. })),
        "a service-user read passes the identity guard and stops at its descriptor: {service:?}"
    );
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the cell asks the PostgreSQL kind's own read, which refuses before its descriptor or driver"
)]
fn a_postgres_partition_refuses_a_switched_principal_before_it_reads() {
    let switched = PartitionReads::SwitchedPrincipal(PrincipalName::parse("alice").expect("a principal"));
    let read = postgres().read_granted(&super::lookup(), b"", &super::schema(), &switched, super::deadline());
    assert!(
        failed_with::<_, TokenUnread>(&read, |cause| cause.to_string()
            == format!(
                "this reading worker reads no partition of `{}` as a caller's switched principal",
                super::geo()
            )),
        "the identity refusal must precede even the malformed descriptor: {read:?}"
    );
}
