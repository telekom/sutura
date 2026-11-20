use sutura_domain::identity::Secret;
use sutura_domain::model::SourceName;
use sutura_exec_postgres::adbc::{AdbcPostgres, Channel, Conninfo, PostgresDriver};
use sutura_exec_postgres::connection::ConnectionTarget;

use super::super::AnyWarehouse;

pub(super) fn warehouse() -> AnyWarehouse {
    let source = SourceName::parse("fixture").expect("fixture source");
    let posture = sutura_domain::source::SourcePosture::SharedServiceUser {
        declared: sutura_domain::source::SharedIdentityDeclared::of(
            sutura_domain::source::AcknowledgementReason::parse("fixture shared source").expect("fixture reason"),
        ),
    };
    let conninfo = Conninfo::new(
        &source,
        ConnectionTarget::Host("127.0.0.1"),
        1,
        "fixture",
        "fixture",
        &Secret::new("unused"),
        Channel::Plaintext,
    )
    .expect("fixture connection");
    let driver = PostgresDriver::parse("/nonexistent/libadbc_driver_postgresql.so").expect("absolute fixture path");
    AnyWarehouse::Postgres(Box::new(
        AdbcPostgres::new(source, posture, driver, conninfo).expect("adapter builds"),
    ))
}
