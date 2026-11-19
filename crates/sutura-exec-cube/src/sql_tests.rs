#![forbid(unsafe_code)]
//! Planned SQL uses the question's credential and refuses incomplete or oversized answers.

use core::cell::RefCell;
use std::collections::{BTreeSet, VecDeque};
use std::time::{Duration, Instant};

use crate::{Answer, CubeError, CubeTransport, CubeWarehouse, Loaded};
use sutura_domain::identity::{Presented, Secret};
use sutura_domain::model::{QualifiedTable, SourceName};
use sutura_domain::plan::MAX_ROWS;
use sutura_domain::raw::RawStatement;
use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::{RawRows, Real, Value, Warehouse as _};

#[derive(Debug, thiserror::Error)]
enum Unavailable {
    #[error("no answer was scripted")]
    Down,
    #[error("the answer exceeded the transport byte cap")]
    TooLarge,
}

type Sent = (String, String, Deadline);

struct Scripted {
    answer: RefCell<VecDeque<Answer<Unavailable>>>,
    sent: RefCell<Vec<Sent>>,
}

impl Scripted {
    fn answering(body: &[u8]) -> Self {
        Self::with(Ok(body.to_vec()))
    }

    fn with(answer: Answer<Unavailable>) -> Self {
        Self {
            answer: RefCell::new([answer].into()),
            sent: RefCell::new(Vec::new()),
        }
    }
}

impl CubeTransport for Scripted {
    type Error = Unavailable;

    fn meta(&self, _token: &Secret, _deadline: Deadline) -> Answer<Self::Error> {
        Err(Unavailable::Down)
    }

    fn load(&self, _query: &serde_json::Value, _token: &Secret, _deadline: Deadline) -> Result<Loaded, Self::Error> {
        Err(Unavailable::Down)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the fake records the credential the SQL port received"
    )]
    fn sql(&self, query: &str, token: &Secret, deadline: Deadline) -> Answer<Self::Error> {
        self.sent
            .borrow_mut()
            .push((String::from(query), String::from(token.expose_secret()), deadline));
        self.answer.borrow_mut().pop_front().ok_or(Unavailable::Down)?
    }

    fn source_refused(&self, _error: &Self::Error) -> bool {
        false
    }

    fn deadline_exceeded(&self, _error: &Self::Error) -> bool {
        false
    }

    fn result_did_not_fit(&self, error: &Self::Error) -> bool {
        matches!(*error, Unavailable::TooLarge)
    }
}

fn declared() -> SharedIdentityDeclared {
    SharedIdentityDeclared::of(AcknowledgementReason::parse("one fixture credential").expect("a reason"))
}

fn cube(transport: Scripted) -> CubeWarehouse<Scripted> {
    CubeWarehouse::of(
        SourceName::parse("metrics").expect("a source"),
        SourcePosture::ImpersonationAtSource,
        Secret::new(String::from("deployment-token-canary-68e1")),
        transport,
    )
}

fn statement() -> RawStatement {
    RawStatement::parse("SELECT subscription_key FROM fct_usage_daily").expect("a statement")
}

fn tables() -> BTreeSet<QualifiedTable> {
    [QualifiedTable::parse("fct_usage_daily").expect("a table")].into()
}

fn subject() -> Presented {
    Presented::SubjectToken {
        material: Secret::new(String::from("caller-token-canary-4c17")),
    }
}

fn deadline() -> Deadline {
    Deadline::opened_at(Instant::now(), Budget::parse(Duration::from_secs(10)).expect("a budget"))
}

fn run(warehouse: &CubeWarehouse<Scripted>) -> Result<RawRows, CubeError<Unavailable>> {
    warehouse
        .execute_planned_statement(&statement(), &tables(), &subject(), deadline())
        .expect("planned SQL is supported")
}

const ONE: &[u8] = b"{\"schema\":[{\"name\":\"subscription_key\",\"column_type\":\"Int64\"}]}\n{\"data\":[[\"1002\"]]}\n";

#[test]
fn planned_sql_sends_the_callers_bearer_and_original_deadline_without_enabling_raw_sql() {
    let warehouse = cube(Scripted::answering(ONE));
    let deadline = deadline();
    assert!(warehouse.accepts_planned_statements());
    let rows = warehouse
        .execute_planned_statement(&statement(), &tables(), &subject(), deadline)
        .expect("planned SQL is supported")
        .expect("a complete answer");
    assert_eq!(rows.columns(), ["subscription_key"]);
    assert_eq!(rows.rows(), [vec![Value::Integer(1002)]]);
    assert!(!warehouse.accepts_raw_statements());
    assert!(warehouse.execute_raw(&statement(), &subject(), deadline).is_none());
    assert_eq!(
        *warehouse.transport.sent.borrow(),
        vec![(
            String::from(statement().as_str()),
            String::from("caller-token-canary-4c17"),
            deadline
        )]
    );
}

#[test]
fn planned_sql_uses_each_questions_own_bearer_on_one_shared_adapter() {
    let warehouse = cube(Scripted::answering(ONE));
    warehouse.transport.answer.borrow_mut().push_back(Ok(ONE.to_vec()));
    for token in ["caller-token-canary-a-731b", "caller-token-canary-b-812c"] {
        let presented = Presented::SubjectToken {
            material: Secret::new(String::from(token)),
        };
        warehouse
            .execute_planned_statement(&statement(), &tables(), &presented, deadline())
            .expect("supported")
            .expect("answer");
    }
    let sent = warehouse.transport.sent.borrow();
    assert_eq!(
        sent.iter().map(|(_, token, _)| token.as_str()).collect::<Vec<_>>(),
        ["caller-token-canary-a-731b", "caller-token-canary-b-812c"]
    );
}

#[test]
fn planned_sql_refuses_a_mismatched_posture_and_an_empty_scan_set() {
    let warehouse = cube(Scripted::answering(ONE));
    let shared = Presented::SharedServiceUser { declared: declared() };
    let wrong = warehouse
        .execute_planned_statement(&statement(), &tables(), &shared, deadline())
        .expect("supported")
        .unwrap_err();
    assert!(matches!(wrong, CubeError::PresentedDisagreesWithPosture { .. }));
    let empty = warehouse
        .execute_planned_statement(&statement(), &BTreeSet::new(), &subject(), deadline())
        .expect("supported")
        .unwrap_err();
    assert!(matches!(empty, CubeError::NoPlannedTables));
    run(&warehouse).expect("the refused calls consumed no response");
}

#[test]
fn planned_sql_refuses_a_spent_deadline_before_calling_the_transport() {
    let warehouse = cube(Scripted::answering(ONE));
    let expired = Deadline::opened_at(
        Instant::now()
            .checked_sub(Duration::from_secs(2))
            .expect("an earlier instant"),
        Budget::parse(Duration::from_secs(1)).expect("a budget"),
    );
    let error = warehouse
        .execute_planned_statement(&statement(), &tables(), &subject(), expired)
        .expect("supported")
        .unwrap_err();
    assert!(warehouse.deadline_exceeded(&error));
    run(&warehouse).expect("the expired call consumed no response");
}

#[test]
fn planned_sql_discards_rows_when_cube_appends_an_error_without_exposing_its_text() {
    let mut body = ONE.to_vec();
    body.extend_from_slice(b"{\"error\":\"server-secret-canary-dc91\"}\n");
    let warehouse = cube(Scripted::answering(&body));
    let error = run(&warehouse).unwrap_err();
    assert!(matches!(error, CubeError::StatementFailed));
    assert!(!format!("{error} {error:?}").contains("server-secret-canary-dc91"));
}

#[test]
fn planned_sql_refuses_ragged_out_of_order_or_unsupported_data() {
    for body in [
        b"{\"data\":[[\"1\"]]}\n".as_slice(),
        b"{\"schema\":[{\"name\":\"x\",\"column_type\":\"Int64\"}]}\n{\"data\":[[]]}\n",
        b"{\"schema\":[{\"name\":\"x\",\"column_type\":\"Decimal\"}]}\n{\"data\":[]}\n",
        b"{\"schema\":[]}\n{\"schema\":[]}\n{\"data\":[]}\n",
        b"{\"schema\":[]}\n",
    ] {
        let error = run(&cube(Scripted::answering(body))).unwrap_err();
        assert!(matches!(error, CubeError::MalformedAnswer { .. }), "{error:?}");
    }
}

#[test]
fn planned_sql_reads_the_declared_wire_types_and_empty_answers() {
    let body = b"{\"schema\":[{\"name\":\"real\",\"column_type\":\"Double\"},{\"name\":\"flag\",\"column_type\":\"Boolean\"},{\"name\":\"day\",\"column_type\":{\"Date\":false}},{\"name\":\"missing\",\"column_type\":\"String\"}]}\n{\"data\":[[\"1.5\",true,\"2026-06-01\",null]]}\n";
    let rows = run(&cube(Scripted::answering(body))).expect("typed values");
    assert_eq!(
        rows.rows(),
        [vec![
            Value::Real(Real::parse(1.5).expect("finite")),
            Value::Integer(1),
            Value::Text(String::from("2026-06-01")),
            Value::Null
        ]]
    );
    let empty = run(&cube(Scripted::answering(b"{\"schema\":[]}\n{\"data\":[]}\n"))).expect("an empty result");
    assert_eq!(empty.rows(), [] as [Vec<Value>; 0]);
}

#[test]
fn planned_sql_refuses_inexact_and_non_finite_double_answers() {
    for value in ["9007199254740993", "1e20", "NaN", "inf"] {
        let kind = "Double";
        let body = format!("{{\"schema\":[{{\"name\":\"x\",\"column_type\":\"{kind}\"}}]}}\n{{\"data\":[[\"{value}\"]]}}\n");
        let error = run(&cube(Scripted::answering(body.as_bytes()))).unwrap_err();
        assert!(
            matches!(error, CubeError::InexactInteger { .. } | CubeError::NotFinite { .. }),
            "{error:?}"
        );
    }
}

#[test]
fn planned_sql_keeps_signed_integer_precision_and_boolean_nulls() {
    let body = b"{\"schema\":[{\"name\":\"n\",\"column_type\":\"Int64\"},{\"name\":\"flag\",\"column_type\":\"Boolean\"}]}\n{\"data\":[[\"9223372036854775807\",true],[\"-9223372036854775808\",false],[\"9007199254740993\",null]]}\n";
    let rows = run(&cube(Scripted::answering(body))).expect("signed strings do not pass through a double");
    assert_eq!(
        rows.rows(),
        [
            vec![Value::Integer(i64::MAX), Value::Integer(1)],
            vec![Value::Integer(i64::MIN), Value::Integer(0)],
            vec![Value::Integer(0x0020_0000_0000_0001), Value::Null],
        ]
    );
}

#[test]
fn planned_sql_refuses_integer_overflow_and_non_integral_decimal_text() {
    for value in ["9223372036854775808", "1.25", "not-a-number-canary-731b"] {
        let body = format!("{{\"schema\":[{{\"name\":\"n\",\"column_type\":\"Int64\"}}]}}\n{{\"data\":[[\"{value}\"]]}}\n");
        let error = run(&cube(Scripted::answering(body.as_bytes()))).unwrap_err();
        assert!(matches!(error, CubeError::NotAnInteger { .. }));
        assert!(!format!("{error} {error:?}").contains(value));
    }
}

#[test]
fn planned_sql_classifies_row_and_transport_byte_caps_as_result_bounds() {
    let mut body = b"{\"schema\":[{\"name\":\"x\",\"column_type\":\"Int64\"}]}\n".to_vec();
    body.extend(serde_json::to_vec(&serde_json::json!({ "data": vec![vec!["1"]; (MAX_ROWS + 1) as usize] })).expect("JSON"));
    let warehouse = cube(Scripted::answering(&body));
    let error = run(&warehouse).unwrap_err();
    assert!(matches!(error, CubeError::TooManyRows { limit: MAX_ROWS }));
    assert!(warehouse.result_did_not_fit(&error));
    let warehouse = cube(Scripted::with(Err(Unavailable::TooLarge)));
    let error = run(&warehouse).unwrap_err();
    assert!(warehouse.result_did_not_fit(&error));
}
