//! What `answer_sql` makes of a leg that ended in a refusal, by whether the combiner says it read a
//! column under a text policy and where that policy runs.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use sutura_domain::identity::Presented;
use sutura_domain::model::{QualifiedTable, SourceName};
use sutura_domain::plan::federated::{FederatedAnswerRefusal, FederatedPlan, Legs};
use sutura_domain::plan::{
    AnchorPlan, Executable, FederationCombiner, MAX_ROWS, PolicyRead, StatementAnswer, StatementLegs, StatementQuestion,
    StatementStopped, WorkersShort,
};
use sutura_domain::raw::{RawOutcome, RawRefusalReason, RawStatement};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::{Budget, Deadline};
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

/// What the combiner says of the one leg it plans: whether it reads a policy column.
struct Combiner(PolicyRead);

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
            .and_then(|()| legs.run_planned(&source(), question.statement(), &BTreeSet::new(), self.0));
        Some(ran.map_err(StatementStopped::Leg))
    }
}

fn refused(read: PolicyRead, ends: Ends) -> RawRefusalReason {
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
        &Combiner(read),
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

/// A supported filter on another column narrows source-side masking and Cube's local-policy input.
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

/// Only Cube's local policy names a size refusal: the cap counts input rows before local filtering.
#[test]
fn a_size_refusal_is_named_only_where_the_policy_runs_after_the_leg() {
    for ends in [Ends::TooMuchData, Ends::Rows(past_the_cap())] {
        assert_eq!(
            refused(PolicyRead::Locally, ends),
            RawRefusalReason::PolicySearchNeedsNarrowing
        );
    }
    for read in [PolicyRead::Unread, PolicyRead::AtSource] {
        assert_eq!(refused(read, Ends::TooMuchData), RawRefusalReason::ResultTooLarge, "{read:?}");
        assert_eq!(
            refused(read, Ends::Rows(past_the_cap())),
            RawRefusalReason::TooManyRows { limit: MAX_ROWS },
            "{read:?}"
        );
    }
}

/// `refused`, but under a deadline already spent before the leg's own call is even made - the
/// pre-dispatch check `run_at` makes ahead of every leg, which used to answer a bare
/// `DeadlineExceeded` with no regard for whether the leg was about to read a policy column.
fn refused_already_out_of_time(read: PolicyRead) -> RawRefusalReason {
    let warehouses = Warehouses::of(Planned {
        inner: RawCapableWarehouse::answering_rows(source(), shared(), 0),
        ends: Ends::Rows(0),
    });
    let pinned = verify_and_validate(bundle_over(&[]), &warehouses).expect("a bundle with no models validates");
    let scope = RunSqlScope::new(run_sql_sources(vec![source()]), false);
    let statement = RawStatement::parse("select n from t").expect("a test statement is a statement");
    let spent = Deadline::opened_at(
        Instant::now()
            .checked_sub(Duration::from_secs(1000))
            .expect("the test clock has run for over 1000s"),
        Budget::parse(Duration::from_secs(1)).expect("a nonzero test budget"),
    );
    let ran = answer_sql(
        &pinned,
        &asked_by_a_person(),
        &statement,
        &CountingBroker::default(),
        &warehouses,
        &Combiner(read),
        &scope,
        1 << 20,
        spent,
    )
    .expect("a leg refused before it ran is still an answer");
    match ran.into_outcome() {
        RawOutcome::Refusal { reason } => reason,
        rows @ RawOutcome::Rows { .. } => panic!("expected a refusal, not {rows:?}"),
    }
}

/// A policy-column search that spent its whole budget before its leg ever reached the data system
/// is still named, not read back as a bare timeout: the pre-dispatch check knows what it is about
/// to run.
#[test]
fn a_leg_already_out_of_time_before_it_runs_is_named_wherever_it_reads_a_policy_column() {
    for read in [PolicyRead::AtSource, PolicyRead::Locally] {
        assert_eq!(
            refused_already_out_of_time(read),
            RawRefusalReason::PolicySearchNeedsNarrowing,
            "{read:?}"
        );
    }
    assert_eq!(
        refused_already_out_of_time(PolicyRead::Unread),
        RawRefusalReason::DeadlineExceeded { budget_seconds: 1 }
    );
}

/// A source-side refusal this port does not classify as a deadline or a size limit stays
/// `source_failed` even where the leg read a column under a text policy - the named refusal is for
/// an exhausted search, never a stand-in for every failure on a protected column.
struct GenuineFailure {
    source: SourceName,
    posture: SourcePosture,
}

impl Warehouse for GenuineFailure {
    type Error = AdapterFailure;
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn execute(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        Err(AdapterFailure::RefusedBySource)
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
        Some(Err(AdapterFailure::RefusedBySource))
    }
}

/// Pinned behaviour, not this branch's own: killed by sweeping every `policy.is_read()` leg into
/// the named refusal regardless of cause (`Claim-Cell`, `devco/claim-mutations/<this fn's name>`).
#[test]
fn a_genuine_source_refusal_over_a_policy_column_stays_source_failed() {
    let warehouses = Warehouses::of(GenuineFailure {
        source: source(),
        posture: shared(),
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
        &Combiner(PolicyRead::AtSource),
        &scope,
        1 << 20,
        test_deadline(),
    )
    .expect("a part that ended in a refusal is an answer, not a failure");
    match ran.into_outcome() {
        RawOutcome::Refusal { reason } => {
            assert_eq!(
                reason,
                RawRefusalReason::SourceFailed,
                "a source refusal this port does not classify as a deadline or size limit must stay generic, \
                 not be swept into the named policy-search refusal: got {reason:?}"
            );
        }
        rows @ RawOutcome::Rows { .. } => panic!("expected a refusal, not {rows:?}"),
    }
}
