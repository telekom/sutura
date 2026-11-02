//! `answer_sql` over a combiner that runs the statement at every source it was built over, as a
//! planned statement's legs run: one mint for all of them, each leg checked before the port.

use sutura_domain::model::SourceName;
use sutura_domain::plan::federated::{FederatedAnswerRefusal, FederatedPlan, Legs};
use sutura_domain::plan::{
    FederationCombiner, MAX_ROWS, StatementAnswer, StatementLegs, StatementQuestion, StatementStopped, WorkersShort,
};
use sutura_domain::raw::{RawOutcome, RawRefusalReason, RawStatement};
use sutura_domain::warehouse::ResultBatches;
use sutura_domain::warehouse::deadline::{Budget, Deadline};

use super::{RunSqlError, RunSqlScope, RunningRaw, answer_sql};
use crate::Warehouses;
use crate::tests::{asked_by_a_person, shared, test_deadline};
use crate::tests_support::{CountingBroker, RawCapableWarehouse, bundle_over};
use crate::verify_and_validate;

/// Plans every statement over `planned`, in order, and answers with the last leg's rows. Over no
/// source it plans nothing; `late`, it stops at the deadline before any leg.
struct EverySource {
    planned: Vec<SourceName>,
    late: bool,
}

impl FederationCombiner for EverySource {
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
        if self.planned.is_empty() {
            return None;
        }
        if self.late {
            return Some(Err(StatementStopped::DeadlineExceeded));
        }
        if self.planned.len() > 1 && !question.federation_allowed() {
            return Some(Err(StatementStopped::SpansMoreThanOneSource));
        }
        let mut last = None;
        let ran = legs.open(&self.planned.iter().cloned().collect()).and_then(|()| {
            for source in &self.planned {
                last = Some(legs.run(source, question.statement())?);
            }
            Ok(())
        });
        Some(
            ran.map_err(StatementStopped::Leg)
                .and_then(|()| last.ok_or(StatementStopped::NotPlanned)),
        )
    }
}

fn named(source: &str) -> SourceName {
    SourceName::parse(source).expect("a test source is a source")
}

fn named_each(sources: &[&str]) -> Vec<SourceName> {
    sources.iter().map(|&source| named(source)).collect()
}

/// [`listing`] with every open source listed and a combiner that plans in time.
fn answered(
    open: &[(&str, usize)],
    planned: &[&str],
    federation_allowed: bool,
    deadline: Deadline,
) -> (RunningRaw<CountingBroker>, usize) {
    let listed: Vec<_> = open.iter().map(|&(source, _)| source).collect();
    let combiner = EverySource {
        planned: named_each(planned),
        late: false,
    };
    listing(open, &listed, &combiner, federation_allowed, deadline)
}

/// `select n from t` through `answer_sql`, over one shared-posture source per `(name, rows)`, those
/// in `listed` listed, planned by `combiner`: the answer, and how many mints it took.
fn listing(
    open: &[(&str, usize)],
    listed: &[&str],
    combiner: &EverySource,
    federation_allowed: bool,
    deadline: Deadline,
) -> (RunningRaw<CountingBroker>, usize) {
    let mut each = open
        .iter()
        .map(|&(source, rows)| RawCapableWarehouse::answering_rows(named(source), shared(), rows));
    let first = each.next().expect("a test opens a source");
    let warehouses = each
        .try_fold(Warehouses::of(first), Warehouses::and)
        .expect("the test sources are distinct");
    let pinned = verify_and_validate(bundle_over(&[]), &warehouses).expect("a bundle with no models validates");
    let scope = RunSqlScope::new(crate::tests_support::run_sql_sources(named_each(listed)), federation_allowed);
    let broker = CountingBroker::default();
    let statement = RawStatement::parse("select n from t").expect("a test statement is a statement");
    let ran = answer_sql(
        &pinned,
        &asked_by_a_person(),
        &statement,
        &broker,
        &warehouses,
        combiner,
        &scope,
        1 << 20,
        deadline,
    );
    (ran, broker.asked())
}

fn refused(ran: RunningRaw<CountingBroker>) -> RawRefusalReason {
    match ran.expect("a refusal is an answer, not a failure").into_outcome() {
        RawOutcome::Refusal { reason } => reason,
        rows @ RawOutcome::Rows { .. } => panic!("expected a refusal, not {rows:?}"),
    }
}

/// A federated statement mints once for every source it reads, and the outcome names each of them
/// with the posture it ran under.
#[test]
fn a_federated_statement_mints_once_and_records_every_source_it_read() {
    let (ran, asked) = answered(&[("local", 2), ("other", 3)], &["local", "other"], true, test_deadline());
    let RawOutcome::Rows { rows, executed_as, .. } = ran.expect("both legs answer").into_outcome() else {
        panic!("a federated statement over two answering sources is answered");
    };
    assert_eq!(rows.len(), 3, "the fake answers with its last leg's rows");
    let record = executed_as.expect("a federated answer records where it ran");
    let read: Vec<_> = record
        .legs()
        .map(|(source, posture)| (source.as_str(), posture.clone()))
        .collect();
    assert_eq!(read, [("local", shared()), ("other", shared())]);
    assert_eq!(asked, 1, "one mint covers every leg");
}

/// With federation off, an answer read one source and records none, as a passed-through one does.
#[test]
fn a_planned_statement_without_federation_records_no_source() {
    let (ran, _) = answered(&[("local", 2)], &["local"], false, test_deadline());
    let RawOutcome::Rows { executed_as, .. } = ran.expect("the one leg answers").into_outcome() else {
        panic!("a statement over one answering source is answered");
    };
    assert_eq!(executed_as, None);
}

/// With federation off, a plan over two sources is refused by its wire name before anything is minted.
#[test]
fn a_plan_over_two_sources_without_federation_is_refused_before_any_mint() {
    let (ran, asked) = answered(&[("local", 1), ("other", 1)], &["local", "other"], false, test_deadline());
    assert_eq!(refused(ran), RawRefusalReason::PlanSpansMoreThanOneSource);
    assert_eq!(asked, 0, "nothing is minted for a statement no leg runs");
}

/// Each leg is held to the row cap, not only the answer: the first leg is over it, the last fits.
#[test]
fn a_leg_over_the_row_cap_is_refused_though_the_answer_would_fit() {
    let over_cap = usize::try_from(MAX_ROWS).expect("the cap fits a usize") + 1;
    let (ran, _) = answered(
        &[("local", over_cap), ("other", 1)],
        &["local", "other"],
        true,
        test_deadline(),
    );
    assert_eq!(refused(ran), RawRefusalReason::TooManyRows { limit: MAX_ROWS });
}

/// A budget spent before the first leg refuses it before the port is asked.
#[test]
fn a_spent_budget_refuses_a_federated_leg_before_the_port() {
    let spent = Deadline::opened_at(
        std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(10))
            .expect("ten seconds before now does not underflow the monotonic clock"),
        Budget::parse(std::time::Duration::from_secs(1)).expect("one second is a budget"),
    );
    let (ran, _) = answered(&[("local", 1), ("other", 1)], &["local", "other"], true, spent);
    assert_eq!(
        refused(ran),
        RawRefusalReason::DeadlineExceeded {
            budget_seconds: spent.budget().seconds()
        }
    );
}

/// Two listed sources and a combiner that plans nothing: a wiring failure, never one source picked.
#[test]
fn two_listed_sources_and_no_plan_is_a_wiring_failure() {
    let (ran, asked) = answered(&[("local", 1), ("other", 1)], &[], true, test_deadline());
    assert!(matches!(ran, Err(RunSqlError::Unplanned)), "{ran:?}");
    assert_eq!(asked, 0, "nothing is minted for a statement nothing planned");
}

/// A combiner stopped at the deadline is the deadline refusal with the request's budget, and nothing
/// is minted for it.
#[test]
fn a_statement_the_combiner_stopped_at_the_deadline_is_a_deadline_refusal() {
    let combiner = EverySource {
        planned: named_each(&["local"]),
        late: true,
    };
    let (ran, asked) = listing(&[("local", 1)], &["local"], &combiner, true, test_deadline());
    assert_eq!(
        refused(ran),
        RawRefusalReason::DeadlineExceeded {
            budget_seconds: test_deadline().budget().seconds()
        }
    );
    assert_eq!(
        asked, 0,
        "nothing is minted for a statement that ran out of time while planning"
    );
}

/// A plan with a leg at an open source the tool does not list fails before anything is minted.
#[test]
fn a_leg_at_an_unlisted_source_fails_before_any_mint() {
    let combiner = EverySource {
        planned: named_each(&["local", "other"]),
        late: false,
    };
    let (ran, asked) = listing(&[("local", 1), ("other", 1)], &["local"], &combiner, true, test_deadline());
    assert!(matches!(ran, Err(RunSqlError::Unlisted)), "{ran:?}");
    assert_eq!(asked, 0, "nothing is minted for a source the tool does not run over");
}
