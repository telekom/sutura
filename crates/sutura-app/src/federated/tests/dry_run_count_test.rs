//! The cell that pins `docs/adr/0008`'s federated row for an ACCEPTED two-leg answer: each leg's
//! own warehouse sees exactly one `dry_run`, because `answer_federated` calls `dry_run_leg` for
//! the fact leg and again for the lookup leg even when the preflight accepts. Split out of `super`
//! (the `federated::tests` module) for that file's own `max-lines` reason - not for thematic
//! tidiness, so `use super::*` reaches every fixture (`federated_plan`, `federated_fact_rows`,
//! `federated_lookup_rows`, `FEDERATED_BUDGET`, `answer_federated` itself) exactly as the other
//! split-out modules read them.
//!
//! **The fake lives HERE, in a `#[cfg(test)]` file, and not in `tests_support`.** The one
//! existing federated `dry_runs()` assertion pins 0 for a budget-refused lookup leg
//! (`a_federated_dry_run_that_spends_the_budget_refuses_both_legs_before_either_executes`), and
//! the fake behind it - `SlowDryRunLegsWarehouse` - already counts. What this cell needs is a
//! counter on the ACCEPTED path, and adding it to an existing shared fake in a non-test file is
//! reverted on base by `just causality` and breaks the base build, so the counter is a new fake
//! in this test-only file.

use super::*;
use sutura_domain::warehouse::PreFlight;

/// A leg-executing fake that counts how many times its `dry_run` was reached, and answers a
/// fixed result on `execute`.
///
/// The shape of [`crate::tests_support::LegsWarehouse`] - declares `EXECUTES_LEGS`, answers one
/// canned [`RowSet`] - with one addition: a `dry_runs` counter, so a test can prove an accepted
/// two-leg answer dry-ran each leg exactly once. `AtomicUsize` rather than `Cell` because
/// `answer_federated` runs the lookup leg's `execute` on a scoped OS thread, so the warehouse
/// must be `Sync`; the dry-run itself is sequential (both legs before either `execute`), but the
/// counter is read after the scope joins, so the ordering is the same either way.
struct CountingDryRunLegsWarehouse {
    source: SourceName,
    posture: SourcePosture,
    result: RowSet,
    dry_runs: std::sync::atomic::AtomicUsize,
}

impl CountingDryRunLegsWarehouse {
    fn answering(source: SourceName, posture: SourcePosture, result: RowSet) -> Self {
        Self {
            source,
            posture,
            result,
            dry_runs: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn dry_runs(&self) -> usize {
        self.dry_runs.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Warehouse for CountingDryRunLegsWarehouse {
    type Error = crate::tests_support::AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn dry_run(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<PreFlight, Self::Error> {
        self.dry_runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(PreFlight::NotAsked)
    }

    fn execute(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        Ok(crate::tests_support::canned(&self.result))
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(self.result.clone()))
    }
}

/// An accepted two-leg answer dry-runs EACH leg exactly once: `answer_federated` calls
/// `dry_run_leg` for the fact leg and again for the lookup leg, even when the preflight accepts,
/// so both legs' own warehouses see one `dry_run` before either `execute` runs.
#[test]
fn a_federated_answer_dry_runs_each_leg_exactly_once() {
    let fact_source = SourceName::parse("facts").expect("a test source");
    let lookup_source = SourceName::parse("geo").expect("a test source");
    let shared = shared();
    let warehouses = Warehouses::of(CountingDryRunLegsWarehouse::answering(
        fact_source.clone(),
        shared.clone(),
        federated_fact_rows(),
    ))
    .and(CountingDryRunLegsWarehouse::answering(
        lookup_source.clone(),
        shared,
        federated_lookup_rows(),
    ))
    .expect("two sources, one registry");

    let outcome = answer_federated(
        (&bundle(), sutura_domain::warehouse::schema::PinnedSchemas::default().digest()),
        &federated_plan(),
        &asked_by_a_person(),
        &crate::tests_support::CountingBroker::default(),
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeilings::DEFAULT,
    )
    .expect("a federated answer is not an error")
    .into_outcome();

    assert!(
        matches!(outcome, ToolOutcome::Answer { .. }),
        "the preflight accepts, so a two-leg answer is answered, not {outcome:?}"
    );
    assert_eq!(
        warehouses.get(&fact_source).expect("the fact leg is registered").dry_runs(),
        1,
        "an accepted two-leg answer dry-runs the fact leg exactly once"
    );
    assert_eq!(
        warehouses
            .get(&lookup_source)
            .expect("the lookup leg is registered")
            .dry_runs(),
        1,
        "an accepted two-leg answer dry-runs the lookup leg exactly once, even when the preflight accepts"
    );
}

/// A plan with three lookup legs, each on its own source, onto the fact leg.
fn three_lookup_plan() -> sutura_domain::plan::FederatedPlan {
    use sutura_domain::measure::{AggregatedColumn, Measure, Term};
    use sutura_domain::model::{Aggregate, ColumnName, TableName};
    use sutura_domain::plan::leg::{LegPlan, LegTerm};
    use sutura_domain::plan::{
        InternalLabel, LookupLeg, LookupLegs, PlanBindings, PlanBucket, PlanColumn, PlanKey, PlanTerm, ResultLabel,
        StatementTables, labels,
    };
    let table = TableName::parse("fct_subscription_monthly").expect("a test table");
    let tablecol = |n: &str| PlanColumn::new(table.clone(), ColumnName::parse(n).expect("a test column"));
    let link = |leg: usize| PlanKey::new(ResultLabel::internal(InternalLabel::Link(leg)), tablecol("customer_key"));
    let bucket = || PlanBucket::new(ResultLabel::bucket(), Grain::Month, tablecol("month"));
    let sum = Measure::Simple(Term::Aggregate(AggregatedColumn::new(
        Aggregate::Sum,
        ColumnName::parse("amount_cents").expect("a test column"),
    )));
    let federation = sutura_domain::federation::Federation::of(&sum);
    let terms = labels(&federation)
        .into_iter()
        .map(|label| {
            LegTerm::new(
                PlanTerm::CountIf {
                    column: tablecol("amount_cents"),
                },
                ResultLabel::internal(label),
            )
        })
        .collect();
    let fact = LegPlan::Fact {
        source: SourceName::parse("facts").expect("a test source"),
        metric: metric(),
        tables: StatementTables::only(table.clone()),
        bucket: bucket(),
        keys: vec![link(0), link(1), link(2)],
        terms,
        bindings: PlanBindings::none(),
        range: june(),
    };
    let lookup = |leg: usize, on: &str| {
        LookupLeg::onto_fact(
            LegPlan::Lookup {
                source: SourceName::parse(on).expect("a test source"),
                table: table.clone().into(),
                keys: vec![link(leg)],
                bindings: PlanBindings::none(),
            },
            true,
        )
    };
    let lookups =
        LookupLegs::parse(vec![lookup(0, "geo"), lookup(1, "crm"), lookup(2, "ops")]).expect("three legs onto the fact");
    sutura_domain::plan::FederatedPlan::new(
        metric(),
        ResultLabel::measure(&metric()),
        bucket(),
        fact,
        None,
        lookups,
        federation,
        Vec::new(),
    )
    .expect("a valid four-leg plan")
}

/// Three lookup legs count one dry-run each, beside the fact leg's one: the cut lists every lookup
/// leg, and the credential request and the posture record name each source, or the answer would
/// not be answered.
#[test]
fn three_lookup_legs_dry_run_once_each() {
    use sutura_domain::plan::InternalLabel;
    let link = |leg: usize| InternalLabel::Link(leg).label();
    let shared = shared();
    let fact_rows = RowSet::new(
        vec![
            link(0),
            link(1),
            link(2),
            String::from(sutura_domain::catalog::TIME_BUCKET_LABEL),
            InternalLabel::Leaf(0).label(),
        ],
        vec![vec![
            Value::Text("c1".into()),
            Value::Text("c1".into()),
            Value::Text("c1".into()),
            Value::Text("2026-06".into()),
            Value::Integer(100),
        ]],
    )
    .expect("a well-formed test fact result");
    let lookup_rows = |leg: usize| RowSet::new(vec![link(leg)], vec![vec![Value::Text("c1".into())]]).expect("a lookup result");
    let sources = ["facts", "geo", "crm", "ops"].map(|name| SourceName::parse(name).expect("a test source"));
    let [facts, geo, crm, ops] = sources.clone();
    let warehouses = Warehouses::of(CountingDryRunLegsWarehouse::answering(facts, shared.clone(), fact_rows))
        .and(CountingDryRunLegsWarehouse::answering(geo, shared.clone(), lookup_rows(0)))
        .and_then(|registry| registry.and(CountingDryRunLegsWarehouse::answering(crm, shared.clone(), lookup_rows(1))))
        .and_then(|registry| registry.and(CountingDryRunLegsWarehouse::answering(ops, shared, lookup_rows(2))))
        .expect("four sources, one registry");

    let outcome = answer_federated(
        (&bundle(), sutura_domain::warehouse::schema::PinnedSchemas::default().digest()),
        &three_lookup_plan(),
        &asked_by_a_person(),
        &crate::tests_support::CountingBroker::default(),
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeilings::DEFAULT,
    )
    .expect("a federated answer is not an error")
    .into_outcome();

    assert!(
        matches!(outcome, ToolOutcome::Answer { .. }),
        "a four-leg answer is answered, not {outcome:?}"
    );
    for source in &sources {
        assert_eq!(
            warehouses.get(source).expect("every leg is registered").dry_runs(),
            1,
            "an accepted four-leg answer dry-runs the leg on {source} exactly once"
        );
    }
}

/// Every leg's capability is checked before any credential is minted: the last of three lookup
/// legs cannot execute, so the answer is refused and the broker is never asked.
#[test]
fn a_three_lookup_answer_is_refused_before_minting_when_its_last_lookup_leg_cannot_execute() {
    let shared = shared();
    let [facts, geo, crm, ops] = ["facts", "geo", "crm", "ops"].map(|name| SourceName::parse(name).expect("a test source"));
    let leg =
        |source, result, can_execute_legs| AsymmetricLegWarehouse::answering(source, shared.clone(), result, can_execute_legs);
    let warehouses = Warehouses::of(leg(facts, federated_fact_rows(), true))
        .and(leg(geo, federated_lookup_rows(), true))
        .and_then(|registry| registry.and(leg(crm, federated_lookup_rows(), true)))
        .and_then(|registry| registry.and(leg(ops, federated_lookup_rows(), false)))
        .expect("four sources, one registry");
    let broker = crate::tests_support::CountingBroker::default();

    let outcome = answer_federated(
        (&bundle(), sutura_domain::warehouse::schema::PinnedSchemas::default().digest()),
        &three_lookup_plan(),
        &asked_by_a_person(),
        &broker,
        &warehouses,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        FEDERATED_BUDGET,
        test_deadline(),
        &SpendLedger::no_budget(),
        sutura_domain::plan::RowCeilings::DEFAULT,
    )
    .map(crate::Answered::into_outcome);

    assert!(
        matches!(
            outcome,
            Ok(ToolOutcome::Refusal {
                reason: RefusalReason::FederationNotExecutable
            })
        ),
        "one lookup leg cannot execute, so the answer is refused: {outcome:?}"
    );
    assert_eq!(broker.asked(), 0, "a refused answer mints no credential");
}
