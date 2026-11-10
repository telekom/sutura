use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use arrow_array::{ArrayRef, Int64Array, RecordBatch};
use arrow_ipc::writer::StreamWriter;
use arrow_schema::{DataType, Field, Schema};
use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::definitions::DefinitionDigest;
use sutura_domain::identity::{Presented, Secret};
use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, SourceName, TableName};
use sutura_domain::pinned::MetricDigest;
use sutura_domain::plan::{
    Executable, LegPlan, PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanKey, PlanMeasure, PlanPredicate, PlanTerm,
    QueryPlan, ResultLabel, StatementTables,
};
use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::{ResultBudget, Warehouse as _};

use super::{FoundryTransport, FoundryWarehouse, OntologyIdentifier, SqlRequest};

#[derive(Debug, thiserror::Error)]
#[error("the fake Foundry transport failed")]
struct FakeError;

#[derive(Debug)]
struct RecordingTransport {
    answer: Vec<u8>,
    seen: Rc<RefCell<Vec<SqlRequest>>>,
}

impl FoundryTransport for RecordingTransport {
    type Error = FakeError;

    fn execute(&self, request: &SqlRequest, _token: &Secret, _deadline: Deadline) -> Result<Vec<u8>, Self::Error> {
        self.seen.borrow_mut().push(request.clone());
        Ok(self.answer.clone())
    }

    fn source_refused(&self, _error: &Self::Error) -> bool {
        false
    }

    fn deadline_exceeded(&self, _error: &Self::Error) -> bool {
        false
    }
}

fn deadline() -> Deadline {
    Deadline::opened_at(
        std::time::Instant::now(),
        Budget::parse(core::time::Duration::from_secs(30)).expect("the fixture budget is non-zero"),
    )
}

fn declared() -> SharedIdentityDeclared {
    SharedIdentityDeclared::of(AcknowledgementReason::parse("one Foundry service identity").expect("the reason parses"))
}

fn plan() -> QueryPlan {
    let table = TableName::parse("orders").expect("the table parses");
    let column = |name: &str| PlanColumn::new(table.clone(), ColumnName::parse(name).expect("the column parses"));
    let metric = MetricName::parse("revenue").expect("the metric parses");
    QueryPlan::new(
        SourceName::parse("foundry").expect("the source parses"),
        metric.clone(),
        StatementTables::only(table.clone()),
        PlanBucket::new(ResultLabel::bucket(), Grain::Month, column("order_date")),
        vec![PlanKey::new(
            ResultLabel::dimension(&sutura_domain::model::DimensionName::parse("region").expect("the dimension parses")),
            column("region"),
        )],
        PlanMeasure::Simple {
            term: PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: column("amount_cents"),
            },
        },
        ResultLabel::measure(&metric),
        PlanBindings::parse(
            vec![
                PlanFilter::new(
                    sutura_domain::plan::PredicateOrigin::Definition,
                    PlanPredicate::AtOrAfter {
                        column: column("order_date"),
                        param: 0,
                    },
                ),
                PlanFilter::new(
                    sutura_domain::plan::PredicateOrigin::Definition,
                    PlanPredicate::Before {
                        column: column("order_date"),
                        param: 1,
                    },
                ),
            ],
            vec![
                sutura_domain::warehouse::ParamValue::Date(Date::new(2026, 6, 1).expect("the date parses")),
                sutura_domain::warehouse::ParamValue::Date(Date::new(2026, 7, 1).expect("the date parses")),
            ],
        )
        .expect("the bindings parse"),
        TimeRange::new(
            Date::new(2026, 6, 1).expect("the date parses"),
            Date::new(2026, 7, 1).expect("the date parses"),
        )
        .expect("the range parses"),
    )
}

fn arrow_answer(rows: usize) -> Vec<u8> {
    let schema = Arc::new(Schema::new(vec![Field::new("revenue", DataType::Int64, false)]));
    let values: ArrayRef = Arc::new(Int64Array::from_iter_values(
        0..i64::try_from(rows).expect("fixture row count fits"),
    ));
    let batch = RecordBatch::try_new(Arc::clone(&schema), vec![values]).expect("the fixture batch is rectangular");
    let mut bytes = Vec::new();
    let mut writer = StreamWriter::try_new(&mut bytes, &schema).expect("the Arrow writer opens");
    writer.write(&batch).expect("the fixture batch writes");
    writer.finish().expect("the Arrow stream finishes");
    bytes
}

fn warehouse(answer: Vec<u8>, seen: Rc<RefCell<Vec<SqlRequest>>>) -> FoundryWarehouse<RecordingTransport> {
    FoundryWarehouse::new(
        SourceName::parse("foundry").expect("the source parses"),
        SourcePosture::SharedServiceUser { declared: declared() },
        OntologyIdentifier::parse("sales_ontology").expect("the ontology parses"),
        Secret::new("foundry-token"),
        RecordingTransport { answer, seen },
        ResultBudget::of_bytes(core::num::NonZeroUsize::MAX),
    )
}

fn shared() -> Presented {
    Presented::SharedServiceUser { declared: declared() }
}

#[test]
fn executes_a_semantic_plan_through_the_transport_fake() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let warehouse = warehouse(arrow_answer(1), Rc::clone(&seen));
    let result = warehouse
        .execute(Executable::Query(&plan()), &shared(), deadline())
        .expect("the fake Foundry answer is accepted");

    assert_eq!(result.rows(), 1);
    let requests = seen.borrow();
    let [request] = requests.as_slice() else {
        panic!("expected one request, got {requests:?}");
    };
    assert!(
        request.query.contains("SELECT"),
        "the semantic plan rendered SQL: {}",
        request.query
    );
    assert_eq!(request.ontology_identifier, "sales_ontology");
    assert_eq!(request.row_limit, super::FOUNDRY_MAX_ROW_LIMIT);
    assert!(!request.dry_run);
    assert!(request.parameters.is_some(), "the date predicates stayed bound");
}

#[test]
fn an_answer_at_the_service_marker_is_refused_as_too_large() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let warehouse = warehouse(arrow_answer(super::FOUNDRY_MAX_ROW_LIMIT), Rc::clone(&seen));
    let plan = plan();
    let leg = LegPlan::Lookup {
        source: plan.source().clone(),
        metric: MetricDigest::new(
            MetricName::parse("revenue").expect("the metric parses"),
            DefinitionDigest::parse("a".repeat(64)).expect("the digest parses"),
        ),
        table: plan.tables().clone(),
        keys: plan.keys().to_vec(),
        bindings: PlanBindings::none(),
    };
    let error = warehouse
        .execute(Executable::Leg(&leg), &shared(), deadline())
        .expect_err("the service marker must not be certified");

    assert!(
        warehouse.result_did_not_fit(&error),
        "the refusal must be classified as a size boundary: {error}"
    );
    assert_eq!(seen.borrow().len(), 1, "the plan reached the fake transport once");
    assert!(!seen.borrow()[0].query.contains("LIMIT"), "a whole leg has no SQL answer cap");
}
