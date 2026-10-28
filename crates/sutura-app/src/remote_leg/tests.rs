//! [`super::run_remote_leg`]'s own cells: Decision 7's posture gate, certification, spend and the
//! audit record it writes per outcome - `telekom/sutura#137` step 3's slice 2.

use sutura_domain::audit::{AuditSink, CallRecord, RecordedOutcome};
use sutura_domain::model::{Aggregate, ColumnName, Grain, TableName};
use sutura_domain::plan::remote::{LegWire, RemoteLegOutcome, to_wire};
use sutura_domain::plan::{
    InternalLabel, LegPlan, LegTerm, PlanBindings, PlanBucket, PlanColumn, PlanTerm, ResultLabel, StatementTables, range_filters,
};
use sutura_domain::query::RefusalReason;
use sutura_domain::source::SourcePosture;
use sutura_domain::warehouse::ParamValue;
use sutura_domain::warehouse::deadline::{Budget, Deadline};

use super::run_remote_leg;
use crate::Warehouses;
use crate::spend::{SpendBudget, SpendLedger};
use crate::tests::{asked_by_a_person, bundle, certified, june, metric, source};
use crate::tests_support::{
    AcknowledgingBroker, DryRunOutcome, LegPreflightWarehouse, MonoPreflightWarehouse, PricedWarehouse, shared_posture,
};

fn table() -> TableName {
    TableName::parse("orders").expect("a test table is a table")
}

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a test column is a column")
}

fn bucket_column() -> PlanColumn {
    PlanColumn::new(table(), column("order_date"))
}

/// A leg the sender planned against [`bundle`]'s own metric, which this same pin also certifies -
/// one model, one grain, no dimensions and no required filter, so the only predicates are the
/// range's own two bounds [`range_filters`] built.
fn certified_leg() -> (LegPlan, Vec<ParamValue>) {
    let mut params = Vec::new();
    let [from, until] = range_filters(june(), &bucket_column(), &mut params);
    let leg = LegPlan::Fact {
        source: source(),
        metric: metric(),
        tables: StatementTables::only(table()),
        bucket: PlanBucket::new(ResultLabel::bucket(), Grain::Month, bucket_column()),
        keys: Vec::new(),
        terms: vec![LegTerm::new(
            PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: PlanColumn::new(table(), column("amount_cents")),
            },
            ResultLabel::internal(InternalLabel::Leaf(0)),
        )],
        bindings: PlanBindings::parse(vec![from, until], params.clone()).expect("coherent"),
        range: june(),
    };
    (leg, params)
}

fn certified_wire() -> (LegWire, Vec<ParamValue>) {
    let (leg, params) = certified_leg();
    let wire = to_wire(&leg, &bundle()).expect("a leg built from this pin's own metric sends");
    (wire, params)
}

/// Identical to [`certified_leg`] except the bucket reads a column the metric never declared as
/// its time column - `BucketNotTimeColumn`, one of the ~20 causes `certify_fact` can raise, all of
/// which collapse to [`RefusalReason::RemoteLegNotCertified`]
/// (`docs/adr/20261017032054-leg-transport-between-sutura-nodes.md`).
fn uncertified_wire() -> (LegWire, Vec<ParamValue>) {
    let leg = LegPlan::Fact {
        source: source(),
        metric: metric(),
        tables: StatementTables::only(table()),
        bucket: PlanBucket::new(
            ResultLabel::bucket(),
            Grain::Month,
            PlanColumn::new(table(), column("amount_cents")),
        ),
        keys: Vec::new(),
        terms: vec![LegTerm::new(
            PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: PlanColumn::new(table(), column("amount_cents")),
            },
            ResultLabel::internal(InternalLabel::Leaf(0)),
        )],
        bindings: PlanBindings::none(),
        range: june(),
    };
    let wire = to_wire(&leg, &bundle()).expect("the sender's own pin still sends it");
    (wire, Vec::new())
}

fn generous_deadline() -> Deadline {
    Deadline::opened_at(
        std::time::Instant::now(),
        Budget::parse(std::time::Duration::from_secs(30)).expect("thirty seconds is a budget"),
    )
}

fn spent_deadline() -> Deadline {
    Deadline::opened_at(
        std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(10))
            .expect("ten seconds before now does not underflow the monotonic clock"),
        Budget::parse(std::time::Duration::from_secs(1)).expect("one second is a budget"),
    )
}

type Combiner = sutura_domain::plan::RefusingCombiner;

#[test]
fn a_certified_leg_runs_and_returns_its_batches() {
    let (wire, params) = certified_wire();
    let warehouse = LegPreflightWarehouse::new(
        source(),
        SourcePosture::ImpersonationAtSource,
        certified(),
        DryRunOutcome::Accepted,
    );
    let warehouses = Warehouses::of(warehouse);
    let broker = AcknowledgingBroker::over(&[(source(), SourcePosture::ImpersonationAtSource)]);
    let ran = run_remote_leg::<_, _, Combiner>(
        &bundle(),
        wire,
        params,
        &asked_by_a_person(),
        &broker,
        &warehouses,
        generous_deadline(),
        &SpendLedger::no_budget(),
    )
    .expect("a certified leg over an impersonating source that runs legs answers");
    let RemoteLegOutcome::Answer { batches } = ran.into_outcome() else {
        panic!("a certified leg must answer, not refuse");
    };
    assert_eq!(batches.rows(), 1);
}

#[test]
fn an_uncertified_leg_is_refused_naming_only_the_source() {
    let (wire, params) = uncertified_wire();
    let warehouse = LegPreflightWarehouse::new(
        source(),
        SourcePosture::ImpersonationAtSource,
        certified(),
        DryRunOutcome::Accepted,
    );
    let warehouses = Warehouses::of(warehouse);
    let broker = AcknowledgingBroker::over(&[(source(), SourcePosture::ImpersonationAtSource)]);
    let ran = run_remote_leg::<_, _, Combiner>(
        &bundle(),
        wire,
        params,
        &asked_by_a_person(),
        &broker,
        &warehouses,
        generous_deadline(),
        &SpendLedger::no_budget(),
    )
    .expect("a refusal is an Ok, never an Err");
    let RemoteLegOutcome::Refusal {
        reason: RefusalReason::RemoteLegNotCertified { source: named },
    } = ran.into_outcome()
    else {
        panic!("a leg this node cannot certify must refuse with the closed code");
    };
    assert_eq!(named, source());
}

#[test]
fn a_shared_source_is_refused_the_same_way_as_an_uncertified_one() {
    // Decision 7: a source declared `SharedServiceUser` never runs a remote leg at all, whatever a
    // leg against it asks for - so even this module's own certified fixture is refused here.
    let (wire, params) = certified_wire();
    let warehouse = LegPreflightWarehouse::new(source(), shared_posture(), certified(), DryRunOutcome::Accepted);
    let warehouses = Warehouses::of(warehouse);
    let broker = AcknowledgingBroker::over(&[(source(), shared_posture())]);
    let ran = run_remote_leg::<_, _, Combiner>(
        &bundle(),
        wire,
        params,
        &asked_by_a_person(),
        &broker,
        &warehouses,
        generous_deadline(),
        &SpendLedger::no_budget(),
    )
    .expect("a refusal is an Ok, never an Err");
    let RemoteLegOutcome::Refusal {
        reason: RefusalReason::RemoteLegNotCertified { source: named },
    } = ran.into_outcome()
    else {
        panic!("a shared source must refuse by the same closed code as an uncertified leg");
    };
    assert_eq!(named, source());
    let touched = warehouses.get(&source()).expect("the registry holds it").executions();
    assert_eq!(touched, 0, "a source refused by posture is never asked");
}

#[test]
fn a_source_that_runs_no_legs_is_refused_the_same_way_as_an_uncertified_one() {
    // The other half of Decision 7: impersonation alone is not enough, the adapter must also
    // declare `executes_legs()` - `MonoPreflightWarehouse` is opened with `EXECUTES_LEGS = false`.
    let (wire, params) = certified_wire();
    let warehouse = MonoPreflightWarehouse::new(
        source(),
        SourcePosture::ImpersonationAtSource,
        certified(),
        DryRunOutcome::Accepted,
    );
    let warehouses = Warehouses::of(warehouse);
    let broker = AcknowledgingBroker::over(&[(source(), SourcePosture::ImpersonationAtSource)]);
    let ran = run_remote_leg::<_, _, Combiner>(
        &bundle(),
        wire,
        params,
        &asked_by_a_person(),
        &broker,
        &warehouses,
        generous_deadline(),
        &SpendLedger::no_budget(),
    )
    .expect("a refusal is an Ok, never an Err");
    let RemoteLegOutcome::Refusal {
        reason: RefusalReason::RemoteLegNotCertified { source: named },
    } = ran.into_outcome()
    else {
        panic!("a source that does not run legs must refuse by the same closed code");
    };
    assert_eq!(named, source());
}

#[test]
fn a_spent_deadline_is_refused_before_the_port_is_ever_asked() {
    let (wire, params) = certified_wire();
    let warehouse = LegPreflightWarehouse::new(
        source(),
        SourcePosture::ImpersonationAtSource,
        certified(),
        DryRunOutcome::Accepted,
    );
    let warehouses = Warehouses::of(warehouse);
    let broker = AcknowledgingBroker::over(&[(source(), SourcePosture::ImpersonationAtSource)]);
    let ran = run_remote_leg::<_, _, Combiner>(
        &bundle(),
        wire,
        params,
        &asked_by_a_person(),
        &broker,
        &warehouses,
        spent_deadline(),
        &SpendLedger::no_budget(),
    )
    .expect("a refusal is an Ok, never an Err");
    assert!(
        matches!(
            ran.outcome(),
            RemoteLegOutcome::Refusal {
                reason: RefusalReason::DeadlineExceeded { .. }
            }
        ),
        "{:?}",
        ran.outcome()
    );
    let touched = warehouses.get(&source()).expect("the registry holds it").executions();
    assert_eq!(
        touched, 0,
        "a spent budget must refuse before the data system's own port is reached"
    );
}

#[test]
fn spend_is_charged_on_this_nodes_own_ledger() {
    let (wire, params) = certified_wire();
    let warehouse = PricedWarehouse::pricing(source(), SourcePosture::ImpersonationAtSource, certified(), Some(1_000));
    let warehouses = Warehouses::of(warehouse);
    let broker = AcknowledgingBroker::over(&[(source(), SourcePosture::ImpersonationAtSource)]);
    let ledger = SpendLedger::new(Some(SpendBudget::new(100_000, std::time::Duration::from_secs(60))));
    let ran = run_remote_leg::<_, _, Combiner>(
        &bundle(),
        wire,
        params,
        &asked_by_a_person(),
        &broker,
        &warehouses,
        generous_deadline(),
        &ledger,
    )
    .expect("a priced leg under budget answers");
    assert!(
        matches!(ran.outcome(), RemoteLegOutcome::Answer { .. }),
        "{:?}",
        ran.outcome()
    );
    assert_eq!(ledger.spent_bytes_total(), Some(1_000));
}

/// One record per outcome - an answer and a refusal - naming the remote-leg shape rather than the
/// mono query path's, so a reviewer reading the sink cannot mistake one for the other.
///
/// This goes through [`crate::surface::LocalService`], not the free function above: the free
/// function never touches a sink, and `LocalService::run_remote_leg` is the call site that does
/// (`crates/sutura-app/src/surface.rs`).
#[test]
fn one_audit_record_is_written_per_outcome() {
    use sutura_domain::capabilities::MetadataCapabilities;
    use sutura_domain::knowledge::Knowledge;
    use sutura_domain::pinned::{Contribution, ContributionManifest, PinnedDefinitions};

    use crate::surface::{LocalService, Surface as _};
    use crate::tests_support::FixedCatalog;

    /// [`bundle`], re-pinned with what it actually declares (`crate::tests::delegated_record`'s own
    /// reason): `bundle`'s own manifest contributes `nothing`, which `LocalService::start`'s own
    /// composition check refuses as unfaithful - fine for the free-function cells above, which never
    /// compose, and wrong for a cell that boots a real service.
    fn declared_bundle() -> PinnedDefinitions {
        let pinned = bundle();
        let produced = MetadataCapabilities::produced(pinned.definitions(), &Knowledge::none());
        PinnedDefinitions::pin(
            pinned.version().clone(),
            pinned.definitions().clone(),
            Knowledge::none(),
            ContributionManifest::single(source(), Contribution::of(produced)),
        )
        .expect("the test definitions hash")
    }

    struct Recording(std::sync::mpsc::Sender<&'static str>);

    impl AuditSink for Recording {
        fn record(&self, record: &CallRecord<'_>) {
            let line = match *record.outcome() {
                RecordedOutcome::RemoteLegAnswered { .. } => "answered",
                RecordedOutcome::RemoteLegRefused { .. } => "refused",
                _ => "other",
            };
            _ = self.0.send(line);
        }
    }

    let (sender, records) = std::sync::mpsc::channel();
    let catalog = FixedCatalog::of(declared_bundle());
    let warehouse = LegPreflightWarehouse::new(
        source(),
        SourcePosture::ImpersonationAtSource,
        certified(),
        DryRunOutcome::Accepted,
    );
    let broker = AcknowledgingBroker::over(&[(source(), SourcePosture::ImpersonationAtSource)]);
    let service = LocalService::start(
        &catalog,
        Warehouses::of(warehouse),
        Recording(sender),
        broker,
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("the anchor-free bundle boots against any warehouse");

    let (answered_wire, answered_params) = certified_wire();
    drop(
        service
            .run_remote_leg(&asked_by_a_person(), answered_wire, answered_params, generous_deadline())
            .expect("the fake answers"),
    );
    let (refused_wire, refused_params) = uncertified_wire();
    drop(
        service
            .run_remote_leg(&asked_by_a_person(), refused_wire, refused_params, generous_deadline())
            .expect("a refusal is an Ok, never an Err"),
    );

    let lines: Vec<&str> = records.try_iter().collect();
    assert_eq!(lines, ["answered", "refused"], "{lines:?}");
}
