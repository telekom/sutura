//! Missing submissions fail an answer; every retained submission spends the response budget.

use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::query::Query;
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, ResultBatches, Warehouse};

use super::{FixedBroker, FixedWarehouse, Grain, Warehouses, answer, asked_by_a_person, bundle, certified, june, metric, shared, source, verify_and_validate};

struct MissingCapture { source: SourceName, posture: SourcePosture }

impl Warehouse for MissingCapture {
    type Error = core::convert::Infallible;
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    fn source(&self) -> &SourceName { &self.source }
    fn posture(&self) -> &SourcePosture { &self.posture }
    fn execute(&self, _: Executable<'_>, _: &Presented, _: Deadline) -> Result<ResultBatches, Self::Error> {
        Ok(sutura_domain::warehouse::arrow::of_row_set(&certified()).expect("fixture rows"))
    }
    fn verify_anchor(&self, _: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> { Ok(AnchorRows::of(certified())) }
}

#[test]
fn a_successful_whole_answer_without_execution_capture_is_a_failure() {
    let registry = Warehouses::of(FixedWarehouse::answering(source(), shared(), certified()));
    let validated = verify_and_validate(bundle(), &registry).expect("validated fixture");
    let absent = Warehouses::of(MissingCapture { source: source(), posture: shared() });
    let query = Query::single(metric(), Grain::Month, june(), Vec::new(), Vec::new());
    let result = answer(&validated, &query, &asked_by_a_person(), &FixedBroker::GrantsShared, &absent);
    assert!(result.is_err(), "a successful result without capture cannot become a certified answer");
}

#[test]
fn the_response_budget_counts_every_execution_including_identical_submissions() {
    use sutura_domain::warehouse::execution::{Execution, PlanStage};
    let bytes = usize::try_from(sutura_domain::query::ResponseByteLimit::DEFAULT.bytes()).expect("the limit fits");
    let record = || Execution::DataFusion { source: None, stage: PlanStage::Combine, logical_plan: "x".repeat(bytes / 2) };
    let rows = certified();
    assert!(crate::bounds::exceeds_response_bound(&rows, &[record()]).is_none());
    assert!(crate::bounds::exceeds_response_bound(&rows, &[record(), record()]).is_some(), "duplicates still spend bytes");
}
