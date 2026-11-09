//! What this adapter decides before it touches the driver. The driver path names a file that does
//! not exist, so a cell that reached the load would answer [`AdbcError::Load`] and fail on the
//! variant it asserts.

use core::num::NonZeroUsize;
use std::time::{Duration, Instant};

use sutura_adbc::DriverLocation;
use sutura_conformance::corpus::{self, Case};
use sutura_domain::calendar::Date;
use sutura_domain::identity::{Presented, PrincipalName, Secret};
use sutura_domain::plan::Executable;
use sutura_domain::source::SourcePosture;
use sutura_domain::warehouse::ParamValue;
use sutura_domain::warehouse::Warehouse as _;
use sutura_domain::warehouse::deadline::{Budget, Deadline};

use super::{AdbcClickHouseWarehouse, AdbcError, ResultBudget, rewrite_placeholders};

fn warehouse(posture: SourcePosture) -> AdbcClickHouseWarehouse {
    AdbcClickHouseWarehouse::new(
        corpus::source(),
        posture,
        DriverLocation::parse("/nonexistent/libadbc_driver_clickhouse.so").expect("an absolute path is a location"),
        String::from("http://127.0.0.1:1"),
        String::from("sutura_service"),
        Secret::new(String::from("deployment-secret")),
        None,
        ResultBudget::of_bytes(NonZeroUsize::new(1 << 20).expect("a test budget is positive")),
    )
}

fn first_case() -> Case {
    corpus::cases().into_iter().next().expect("the corpus has a case")
}

fn spent_deadline() -> Deadline {
    let budget = Budget::parse(Duration::from_millis(1)).expect("a non-zero budget");
    let opened = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .expect("one second ago is representable");
    Deadline::opened_at(opened, budget)
}

#[test]
fn a_spent_deadline_is_refused_before_the_driver_is_loaded() {
    let warehouse = warehouse(corpus::posture());
    let case = first_case();
    let error = warehouse
        .execute(Executable::Query(case.plan()), &corpus::presented(), spent_deadline())
        .expect_err("a spent deadline is refused before any round trip");
    assert!(matches!(error, AdbcError::DeadlineSpent), "{error}");
    assert!(warehouse.deadline_exceeded(&error), "the port classifies a spent deadline");
}

#[test]
fn a_subject_credential_is_refused_as_no_place_to_arrive() {
    let presented = Presented::SubjectToken {
        material: Secret::new(String::from("unused")),
    };
    let case = first_case();
    let error = warehouse(corpus::posture())
        .execute(Executable::Query(case.plan()), &presented, corpus::deadline())
        .expect_err("a subject credential has nowhere to arrive on this adapter");
    assert!(matches!(error, AdbcError::NoPlaceForASubject), "{error}");
}

/// A principal at a SHARED source disagrees with its declaration and is refused, never run as the
/// service user under that name.
#[test]
fn a_subject_principal_at_a_shared_source_is_refused() {
    let presented = Presented::SubjectPrincipal {
        name: PrincipalName::parse("analyst_role").expect("a test name is a name"),
    };
    let case = first_case();
    let outcome = warehouse(corpus::posture()).execute(Executable::Query(case.plan()), &presented, corpus::deadline());
    assert!(
        matches!(outcome, Err(AdbcError::PresentedDisagreesWithPosture(_))),
        "a principal at a shared source disagrees with its declaration: {outcome:?}"
    );
}

#[test]
fn a_principal_that_is_not_a_clickhouse_user_is_refused_before_the_driver_is_loaded() {
    let presented = Presented::SubjectPrincipal {
        name: PrincipalName::parse("a\" SELECT 1 --").expect("a test name is a name"),
    };
    let case = first_case();
    let outcome =
        warehouse(SourcePosture::ImpersonationAtSource).execute(Executable::Query(case.plan()), &presented, corpus::deadline());
    assert!(matches!(outcome, Err(AdbcError::NotAClickHouseUser(_))), "{outcome:?}");
}

#[test]
#[expect(
    clippy::literal_string_with_formatting_args,
    reason = "the braces are ClickHouse's own named-parameter syntax in a plain expected literal, not an \
              unformatted `format!` call"
)]
fn every_question_mark_becomes_a_named_typed_placeholder() {
    let params = [
        ParamValue::Text(String::from("north")),
        ParamValue::Date(Date::from_days_since_epoch(0).expect("the epoch is a date")),
    ];
    let rewritten = rewrite_placeholders("SELECT * FROM t WHERE region = ? AND day >= ?", &params).expect("two bound");
    assert_eq!(rewritten, "SELECT * FROM t WHERE region = {p0:String} AND day >= {p1:Date}");
}

#[test]
fn a_placeholder_count_mismatch_is_refused() {
    let params = [ParamValue::Text(String::from("north"))];
    let refused = rewrite_placeholders("SELECT ? , ?", &params).expect_err("two placeholders, one value");
    assert!(
        matches!(&refused, AdbcError::Driver(message) if message.to_string().contains("2 placeholders")),
        "{refused}"
    );
}
