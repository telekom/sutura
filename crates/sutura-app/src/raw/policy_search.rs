//! What `answer_sql` makes of a leg that ended in a refusal, by whether the combiner says it read a
//! column under a text policy and where that policy runs.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use sutura_domain::catalog::PrefilterIneligible;
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

use super::{POLICY_LEG_MIN_BUDGET, POLICY_LEG_TEARDOWN_MARGIN, RunSqlScope, answer_sql};
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
    /// What the leg's own deadline left, as the port saw it - for the margin-narrowing tests below,
    /// which read this rather than time anything.
    handed: std::cell::RefCell<Option<Deadline>>,
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
        deadline: Deadline,
    ) -> RawExecution<Self::Error> {
        *self.handed.borrow_mut() = Some(deadline);
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

/// What the combiner says of the one leg it plans: whether it reads a policy column, and why the
/// raw prefilter could not push the search down (if it ever tried).
struct Combiner(PolicyRead, Option<PrefilterIneligible>);

impl Combiner {
    fn new(read: PolicyRead) -> Self {
        Self(read, None)
    }

    fn with_ineligible(read: PolicyRead, ineligible: PrefilterIneligible) -> Self {
        Self(read, Some(ineligible))
    }
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
            .and_then(|()| legs.run_planned(&source(), question.statement(), &BTreeSet::new(), self.0, self.1));
        Some(ran.map_err(StatementStopped::Leg))
    }
}

fn refused(read: PolicyRead, ends: Ends) -> RawRefusalReason {
    refused_with(&Combiner::new(read), ends)
}

fn refused_with(combiner: &Combiner, ends: Ends) -> RawRefusalReason {
    let warehouses = Warehouses::of(Planned {
        inner: RawCapableWarehouse::answering_rows(source(), shared(), 0),
        ends,
        handed: std::cell::RefCell::new(None),
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

/// A supported filter on another column narrows source-side masking and Cube's local-policy input.
#[test]
fn a_leg_past_its_deadline_is_named_wherever_it_read_a_policy_column() {
    for read in [PolicyRead::AtSource, PolicyRead::Locally] {
        assert_eq!(
            refused(read, Ends::TimedOut),
            RawRefusalReason::PolicySearchNeedsNarrowing { ineligible: None },
            "{read:?}"
        );
    }
    assert_eq!(refused(PolicyRead::Unread, Ends::TimedOut), deadline_exceeded());
}

/// A policy search that timed out after dispatch carries WHY the raw prefilter could not push it
/// down - the same [`PrefilterIneligible`] value `catalog::eligible` computed over the needle and
/// the column's policy, not read back from how long the leg ran or what it returned.
#[test]
fn a_leg_past_its_deadline_names_why_the_prefilter_could_not_push_it_down() {
    for read in [PolicyRead::AtSource, PolicyRead::Locally] {
        assert_eq!(
            refused_with(
                &Combiner::with_ineligible(read, PrefilterIneligible::NotDeclared),
                Ends::TimedOut
            ),
            RawRefusalReason::PolicySearchNeedsNarrowing {
                ineligible: Some(PrefilterIneligible::NotDeclared)
            },
            "{read:?}"
        );
    }
}

/// Only Cube's local policy names a size refusal: the cap counts input rows before local filtering.
#[test]
fn a_size_refusal_is_named_only_where_the_policy_runs_after_the_leg() {
    for ends in [Ends::TooMuchData, Ends::Rows(past_the_cap())] {
        assert_eq!(
            refused(PolicyRead::Locally, ends),
            RawRefusalReason::PolicySearchNeedsNarrowing { ineligible: None }
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
        handed: std::cell::RefCell::new(None),
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
        &Combiner::new(read),
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
            RawRefusalReason::PolicySearchNeedsNarrowing { ineligible: None },
            "{read:?}"
        );
    }
    assert_eq!(
        refused_already_out_of_time(PolicyRead::Unread),
        RawRefusalReason::DeadlineExceeded { budget_seconds: 1 }
    );
}

/// `run_at`'s leg under `read`, with `budget` as the total deadline's own budget - returning the
/// remaining duration the port actually handed `Planned::execute_planned_statement`, so the margin
/// in `POLICY_LEG_TEARDOWN_MARGIN` is checked on what the leg SAW, not timed.
fn handed_remaining(read: PolicyRead, budget: Duration) -> Duration {
    let planned = Planned {
        inner: RawCapableWarehouse::answering_rows(source(), shared(), 0),
        ends: Ends::Rows(0),
        handed: std::cell::RefCell::new(None),
    };
    let warehouses = Warehouses::of(planned);
    let pinned = verify_and_validate(bundle_over(&[]), &warehouses).expect("a bundle with no models validates");
    let scope = RunSqlScope::new(run_sql_sources(vec![source()]), false);
    let statement = RawStatement::parse("select n from t").expect("a test statement is a statement");
    let opened = Instant::now();
    let deadline = Deadline::opened_at(opened, Budget::parse(budget).expect("a nonzero test budget"));
    let ran = answer_sql(
        &pinned,
        &asked_by_a_person(),
        &statement,
        &CountingBroker::default(),
        &warehouses,
        &Combiner::new(read),
        &scope,
        1 << 20,
        deadline,
    )
    .expect("an answered row is still an answer");
    assert!(
        matches!(ran.into_outcome(), RawOutcome::Rows { .. }),
        "this fixture's leg always succeeds; a refusal means the deadline computation panicked or \
         was itself exhausted before the fixture could record it"
    );
    let handed = warehouses
        .get(&source())
        .expect("the one fixture source is registered")
        .handed
        .borrow()
        .expect("execute_planned_statement must have run and recorded the deadline it was handed");
    handed.remaining_at(opened).unwrap_or_default()
}

/// Review item 2 on `#2079`: with a generous total budget, a policy-read leg gets the deadline
/// narrowed by the full `POLICY_LEG_TEARDOWN_MARGIN` - leaving the adapter's own teardown and local
/// masking room inside `docs/adr/0029`'s `REPLY_MARGIN`, the whole point of the narrowing.
#[test]
fn a_policy_leg_is_narrowed_by_the_full_margin_when_the_remaining_budget_affords_it() {
    let budget = Duration::from_secs(10);
    let narrowed_to = budget
        .checked_sub(POLICY_LEG_TEARDOWN_MARGIN)
        .expect("the fixture's budget exceeds the margin");
    let handed = handed_remaining(PolicyRead::AtSource, budget);
    assert!(
        handed <= narrowed_to && handed > narrowed_to.checked_sub(Duration::from_millis(500)).unwrap_or_default(),
        "a 10s budget must narrow to ~8s left, got {handed:?}"
    );
    // A leg that does not read a policy column is never narrowed.
    let unread = handed_remaining(PolicyRead::Unread, budget);
    assert!(
        unread > budget.checked_sub(Duration::from_millis(500)).unwrap_or_default(),
        "an unread leg keeps the full budget, got {unread:?}"
    );
}

/// Review item 2 on `#2079`: a deployment configuring a low total `request_timeout_seconds` (here,
/// less than the margin itself) must not have the narrowing refuse every policy-column read before
/// the data system is ever asked - the leg keeps the full remaining budget instead, the behaviour
/// before this margin existed.
#[test]
fn a_policy_leg_keeps_the_full_budget_when_narrowing_would_leave_too_little() {
    let budget = Duration::from_millis(1500);
    assert!(
        budget < POLICY_LEG_TEARDOWN_MARGIN,
        "the fixture must exercise the low-budget fallback"
    );
    let handed = handed_remaining(PolicyRead::AtSource, budget);
    assert!(
        handed > Duration::from_millis(1000),
        "narrowing a sub-margin budget must fall back to the full remaining budget, not refuse \
         pre-dispatch or leave almost nothing: got {handed:?}"
    );
}

/// Opus round-3 review on `#2079`: a budget that is below the full `POLICY_LEG_TEARDOWN_MARGIN` but
/// still above `POLICY_LEG_MIN_BUDGET` after subtracting it must still narrow - the floor checked
/// is `POLICY_LEG_MIN_BUDGET`, a smaller number than the margin itself, never the margin again. A
/// `request_timeout_seconds` of 3-4s is exactly the band the HTTP cell
/// `a_policy_search_leg_past_its_deadline_reaches_the_caller_by_name_ahead_of_the_408` exercises.
///
/// Kept off the floor itself (`3.5s`/`4s`, not `3s`): at `3s` the narrowed value is exactly
/// `POLICY_LEG_MIN_BUDGET`, and the sub-millisecond elapsed time `handed_remaining` actually spends
/// reading the clock can push the narrowed value a hair under the floor, flipping this fixture into
/// the "kept full" branch tested separately below - a real flake, not a hypothetical one.
#[test]
fn a_policy_leg_in_the_low_timeout_band_narrows_once_the_floor_still_allows_it() {
    for budget in [Duration::from_millis(3500), Duration::from_secs(4)] {
        let narrowed_to = budget
            .checked_sub(POLICY_LEG_TEARDOWN_MARGIN)
            .expect("each fixture budget here exceeds the margin");
        assert!(
            narrowed_to >= POLICY_LEG_MIN_BUDGET,
            "the fixture must land on or above the floor: {budget:?}"
        );
        let handed = handed_remaining(PolicyRead::AtSource, budget);
        assert!(
            handed <= narrowed_to && handed > narrowed_to.checked_sub(Duration::from_millis(500)).unwrap_or_default(),
            "a {budget:?} budget must narrow to ~{narrowed_to:?} left, got {handed:?}"
        );
    }
}

/// The floor itself: a budget whose narrowed value would fall below `POLICY_LEG_MIN_BUDGET` (but
/// above zero, unlike the sub-margin fixture above) keeps the full remaining budget rather than the
/// too-thin narrowed one.
#[test]
fn a_policy_leg_below_the_floor_keeps_the_full_budget_rather_than_a_too_thin_one() {
    let budget = Duration::from_millis(2500);
    let narrowed_to = budget
        .checked_sub(POLICY_LEG_TEARDOWN_MARGIN)
        .expect("the fixture's budget exceeds the margin");
    assert!(
        narrowed_to < POLICY_LEG_MIN_BUDGET && narrowed_to > Duration::ZERO,
        "the fixture must land strictly below the floor and above zero: {narrowed_to:?}"
    );
    let handed = handed_remaining(PolicyRead::AtSource, budget);
    assert!(
        handed > budget.checked_sub(Duration::from_millis(500)).unwrap_or_default(),
        "below the floor the leg must keep the full budget, not the too-thin narrowed one: got {handed:?}"
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
        &Combiner::new(PolicyRead::AtSource),
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
