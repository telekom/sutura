//! What `answer_sql` makes of a leg that ended in a refusal, by whether the combiner says it read a
//! column under a text policy and where that policy runs.

use std::collections::BTreeSet;

use sutura_domain::identity::Presented;
use sutura_domain::model::{QualifiedTable, SourceName};
use sutura_domain::plan::federated::{FederatedAnswerRefusal, FederatedPlan, Legs};
use sutura_domain::plan::{
    AnchorPlan, Executable, FederationCombiner, MAX_ROWS, PolicyRead, StatementAnswer, StatementLegs, StatementQuestion,
    StatementStopped, WorkersShort,
};
use sutura_domain::raw::{RawOutcome, RawRefusalReason, RawStatement};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, RawExecution, RawRows, ResultBatches, Value, Warehouse};

use super::{RunSqlScope, answer_sql};
use crate::tests::{asked_by_a_person, shared, source, test_deadline};
use crate::tests_support::{AdapterFailure, CountingBroker, RawCapableWarehouse, bundle_over, run_sql_sources};
use crate::{Warehouses, verify_and_validate};

/// How the one planned source ends its statement.
#[derive(Clone, Copy)]
enum Ends {
    Rows(usize),
    TooMuchData,
    TimedOut,
}

struct Planned {
    inner: RawCapableWarehouse,
    ends: Ends,
}

impl Warehouse for Planned {
    type Error = AdapterFailure;
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn source(&self) -> &SourceName {
        self.inner.source()
    }

    fn posture(&self) -> &SourcePosture {
        self.inner.posture()
    }

    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        self.inner.execute(executable, presented, deadline)
    }

    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(AdapterFailure::RefusedBySource)
    }

    fn accepts_planned_statements(&self) -> bool {
        true
    }

    fn execute_planned_statement(
        &self,
        _statement: &RawStatement,
        _tables: &BTreeSet<QualifiedTable>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> RawExecution<Self::Error> {
        Some(match self.ends {
            Ends::Rows(rows) => Ok(RawRows::of(vec![String::from("n")], vec![vec![Value::Integer(0)]; rows])),
            Ends::TooMuchData => Err(AdapterFailure::TooMuchData),
            Ends::TimedOut => Err(AdapterFailure::TimedOut),
        })
    }

    fn result_did_not_fit(&self, error: &Self::Error) -> bool {
        matches!(error, AdapterFailure::TooMuchData)
    }

    fn deadline_exceeded(&self, error: &Self::Error) -> bool {
        matches!(error, AdapterFailure::TimedOut)
    }
}

/// What the combiner says of the one leg it plans: whether it reads a policy column. With `late`, the
/// leg answers and the local part then runs past the deadline.
struct Combiner {
    read: PolicyRead,
    late: bool,
}

impl FederationCombiner for Combiner {
    type Error = core::fmt::Error;

    fn combine(&self, _plan: &FederatedPlan, _legs: Legs<'_>, _working_set_bytes: u64) -> Result<ResultBatches, Self::Error> {
        Err(core::fmt::Error)
    }

    fn working_set_exhausted(&self, _error: &Self::Error) -> Option<u64> {
        None
    }

    fn answer_not_well_formed(&self, _error: &Self::Error) -> Option<FederatedAnswerRefusal> {
        None
    }

    fn workers_not_ready(&self, _error: &Self::Error) -> Option<WorkersShort> {
        None
    }

    fn node_lost(&self, _error: &Self::Error) -> bool {
        false
    }

    fn answer_statement<L>(&self, question: StatementQuestion<'_>, legs: &mut L) -> StatementAnswer<L::Stop>
    where
        L: StatementLegs,
    {
        let ran = legs
            .open(&BTreeSet::from([source()]))
            .and_then(|()| legs.run_planned(&source(), question.statement(), &BTreeSet::new(), self.read));
        if self.late {
            return Some(Err(StatementStopped::PolicySearchDeadlineExceeded));
        }
        Some(ran.map_err(StatementStopped::Leg))
    }
}

fn refused(read: PolicyRead, ends: Ends) -> RawRefusalReason {
    refused_by(&Combiner { read, late: false }, ends)
}

fn refused_by(combiner: &Combiner, ends: Ends) -> RawRefusalReason {
    let warehouses = Warehouses::of(Planned {
        inner: RawCapableWarehouse::answering_rows(source(), shared(), 0),
        ends,
    });
    let pinned = verify_and_validate(bundle_over(&[]), &warehouses).expect("a bundle with no models validates");
    let scope = RunSqlScope::new(run_sql_sources(vec![source()]), false);
    let statement = RawStatement::parse("select n from t").expect("a test statement is a statement");
    let ran = answer_sql(
        &pinned,
        &asked_by_a_person(),
        &statement,
        &CountingBroker::default(),
        &warehouses,
        combiner,
        &scope,
        1 << 20,
        test_deadline(),
    )
    .expect("a part that ended in a refusal is an answer, not a failure");
    match ran.into_outcome() {
        RawOutcome::Refusal { reason } => reason,
        rows @ RawOutcome::Rows { .. } => panic!("expected a refusal, not {rows:?}"),
    }
}

fn past_the_cap() -> usize {
    usize::try_from(MAX_ROWS).expect("the cap fits a usize") + 1
}

fn deadline_exceeded() -> RawRefusalReason {
    RawRefusalReason::DeadlineExceeded {
        budget_seconds: test_deadline().budget().seconds(),
    }
}

/// Named wherever a policy column is read, so a filter on another column narrows the scan: at the
/// source that applies the policy, and at Cube, where a filter on a column without a policy reaches it.
#[test]
fn a_leg_past_its_deadline_is_named_wherever_it_read_a_policy_column() {
    for read in [PolicyRead::AtSource, PolicyRead::Locally] {
        assert_eq!(
            refused(read, Ends::TimedOut),
            RawRefusalReason::PolicySearchNeedsNarrowing,
            "{read:?}"
        );
    }
    assert_eq!(refused(PolicyRead::Unread, Ends::TimedOut), deadline_exceeded());
}

/// A size refusal is renamed only at Cube, where the leg's row cap counts rows before the policy and
/// a filter that reaches Cube narrows them. A source that applies the policy answers rows already
/// masked and filtered, so its size refusals are what they were.
#[test]
fn a_size_refusal_is_named_only_where_the_policy_runs_after_the_leg() {
    assert_eq!(
        refused(PolicyRead::Locally, Ends::TooMuchData),
        RawRefusalReason::PolicySearchNeedsNarrowing
    );
    assert_eq!(
        refused(PolicyRead::Locally, Ends::Rows(past_the_cap())),
        RawRefusalReason::PolicySearchNeedsNarrowing
    );
    for read in [PolicyRead::Unread, PolicyRead::AtSource] {
        assert_eq!(refused(read, Ends::TooMuchData), RawRefusalReason::ResultTooLarge, "{read:?}");
        assert_eq!(
            refused(read, Ends::Rows(past_the_cap())),
            RawRefusalReason::TooManyRows { limit: MAX_ROWS },
            "{read:?}"
        );
    }
}

/// A local part over rows a Cube leg answered, past the deadline, is the same refusal as a Cube leg past it.
#[test]
fn a_local_part_past_the_deadline_over_a_policy_that_ran_here_is_named() {
    let late = Combiner {
        read: PolicyRead::Locally,
        late: true,
    };
    assert_eq!(refused_by(&late, Ends::Rows(1)), RawRefusalReason::PolicySearchNeedsNarrowing);
}
