//! An `impersonation-at-source` Foundry source: which token each request carries, and that none is
//! the deployment's. Over a recording fake of the transport port; no tenant is involved.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use arrow_array::{ArrayRef, Int64Array, RecordBatch};
use arrow_ipc::writer::StreamWriter;
use arrow_schema::{DataType, Field, Schema};
use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::identity::{Presented, Secret};
use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, SourceName, TableName};
use sutura_domain::plan::{Executable, PlanBindings, PlanBucket, PlanColumn, PlanMeasure, PlanTerm, QueryPlan, ResultLabel};
use sutura_domain::source::{AcknowledgementReason, ImpersonationCapability, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::{ResultBudget, Warehouse};

use super::{FoundryError, FoundryTransport, FoundryWarehouse, OntologyIdentifier, SqlRequest};

/// What the fake answers every request with.
#[derive(Debug, Clone, Copy)]
enum Answer {
    Rows,
    Status(u16),
}

/// A non-success status, as the fake reports it.
#[derive(Debug, thiserror::Error)]
#[error("the fake Foundry endpoint answered {0}")]
struct Status(u16);

/// Sends the bearer of every request it is handed to the cell, then answers as scripted.
#[derive(Debug)]
struct Recording {
    answer: Answer,
    seen: Sender<String>,
}

impl FoundryTransport for Recording {
    type Error = Status;

    fn execute(&self, _request: &SqlRequest, token: &Secret, _deadline: Deadline) -> Result<Vec<u8>, Self::Error> {
        #[expect(clippy::disallowed_methods, reason = "the cell asserts which token reached the transport")]
        let token = String::from(token.expose_secret());
        drop(self.seen.send(token));
        match self.answer {
            Answer::Rows => Ok(one_row()),
            Answer::Status(status) => Err(Status(status)),
        }
    }

    /// The real transport's rule: an authentication or authorization refusal is the source's.
    fn source_refused(&self, error: &Self::Error) -> bool {
        matches!(error, Status(401 | 403))
    }

    fn deadline_exceeded(&self, _error: &Self::Error) -> bool {
        false
    }
}

fn one_row() -> Vec<u8> {
    let schema = Arc::new(Schema::new(vec![Field::new("revenue", DataType::Int64, false)]));
    let values: ArrayRef = Arc::new(Int64Array::from(vec![7_i64]));
    let batch = RecordBatch::try_new(Arc::clone(&schema), vec![values]).expect("the fixture batch is rectangular");
    let mut bytes = Vec::new();
    let mut writer = StreamWriter::try_new(&mut bytes, &schema).expect("the Arrow writer opens");
    writer.write(&batch).expect("the fixture batch writes");
    writer.finish().expect("the Arrow stream finishes");
    bytes
}

fn source() -> SourceName {
    SourceName::parse("foundry").expect("the source parses")
}

/// A per-caller source over the fake, and every bearer the fake was handed.
struct Fixture {
    warehouse: FoundryWarehouse<Recording>,
    seen: Receiver<String>,
}

fn each_caller(answer: Answer) -> Fixture {
    let (seen, received) = channel();
    let warehouse = FoundryWarehouse::each_caller(
        source(),
        OntologyIdentifier::parse("sales_ontology").expect("the ontology parses"),
        Recording { answer, seen },
        ResultBudget::of_bytes(core::num::NonZeroUsize::MAX),
    );
    Fixture {
        warehouse,
        seen: received,
    }
}

fn plan() -> QueryPlan {
    let table = TableName::parse("orders").expect("the table parses");
    let column = |name: &str| PlanColumn::new(table.clone(), ColumnName::parse(name).expect("the column parses"));
    let metric = MetricName::parse("revenue").expect("the metric parses");
    let june = TimeRange::new(
        Date::new(2026, 6, 1).expect("the date parses"),
        Date::new(2026, 7, 1).expect("the date parses"),
    )
    .expect("June is a range");
    QueryPlan::new(
        source(),
        metric.clone(),
        sutura_domain::plan::StatementTables::only(table.clone()),
        PlanBucket::new(ResultLabel::bucket(), Grain::Month, column("order_date")),
        Vec::new(),
        PlanMeasure::Simple {
            term: PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: column("amount_cents"),
            },
        },
        ResultLabel::measure(&metric),
        PlanBindings::none(),
        june,
    )
}

fn deadline() -> Deadline {
    Deadline::opened_at(
        std::time::Instant::now(),
        Budget::parse(core::time::Duration::from_secs(30)).expect("the fixture budget is non-zero"),
    )
}

fn callers_token(raw: &str) -> Presented {
    Presented::SubjectToken {
        material: Secret::new(raw),
    }
}

#[test]
fn each_request_carries_the_token_the_broker_presented_for_its_caller_unchanged() {
    let Fixture { warehouse, seen } = each_caller(Answer::Rows);
    let plan = plan();
    for token in ["caller-a.foundry-token", "caller-b.foundry-token"] {
        warehouse
            .dry_run(Executable::Query(&plan), &callers_token(token), deadline())
            .expect("the parse-only request is accepted");
        let answered = warehouse
            .execute(Executable::Query(&plan), &callers_token(token), deadline())
            .expect("the fake answer is accepted");
        assert_eq!(answered.rows(), 1);
        assert_eq!(
            seen.try_iter().collect::<Vec<String>>(),
            [token, token],
            "the parse-only and the executing request must both carry this caller's own token, byte for byte"
        );
    }
}

#[test]
fn a_per_caller_source_sends_no_request_for_the_deployments_own_identity() {
    let Fixture { warehouse, seen } = each_caller(Answer::Rows);
    let shared = Presented::SharedServiceUser {
        declared: SharedIdentityDeclared::of(AcknowledgementReason::parse("one Foundry service identity").expect("reason")),
    };
    let refused = warehouse
        .execute(Executable::Query(&plan()), &shared, deadline())
        .expect_err("a per-caller source has no deployment token to send");
    assert!(matches!(refused, FoundryError::Undeliverable { .. }), "{refused:?}");
    assert_eq!(seen.try_iter().count(), 0, "nothing may reach Foundry under no caller");
}

#[test]
fn a_foundry_refusal_of_the_callers_token_is_a_source_refusal_and_no_answer() {
    for status in [401, 403] {
        let Fixture { warehouse, seen } = each_caller(Answer::Status(status));
        let refused = warehouse
            .execute(
                Executable::Query(&plan()),
                &callers_token("caller-a.foundry-token"),
                deadline(),
            )
            .expect_err("a refused token is no answer");
        assert!(
            warehouse.source_refused(&refused),
            "{status} under the caller's own token must be the source's refusal: {refused:?}"
        );
        assert_eq!(seen.try_iter().count(), 1);
    }
}

#[test]
fn a_per_caller_source_is_declared_impersonating_and_can_carry_a_caller() {
    let Fixture { warehouse, .. } = each_caller(Answer::Rows);
    assert_eq!(*warehouse.posture(), SourcePosture::ImpersonationAtSource);
    assert_eq!(
        <FoundryWarehouse<Recording> as Warehouse>::IMPERSONATION,
        ImpersonationCapability::PerSubjectCredential
    );
}
