//! `Surface::adopt`: a refresh swaps the bundle a question reads, a question reads one snapshot
//! taken at its start, and a refused bundle leaves the served one in place.

use super::*;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind};
use sutura_domain::definitions::DefinitionDigest;
use sutura_domain::identity::Presented;
use sutura_domain::knowledge::KnowledgeCapabilities;
use sutura_domain::plan::{AnchorPlan, Executable, RefusingCombiner};
use sutura_domain::source::ImpersonationCapability;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, PreFlight, ResultBatches, Warehouse};

use crate::surface::{Adopted, AdoptionGate, ErasedCause, LocalService, NotAdopted, Surface};
use crate::tests_support::{DiscardingAuditSink, FixedCatalog, canned};

type Service<W> = LocalService<W, DiscardingAuditSink, FixedBroker, RefusingCombiner>;

/// The bundle every cell here boots from: the one metric of [`bundle`], declared.
fn served() -> PinnedDefinitions {
    bundle_of(&["revenue"], "197122")
}

/// The test model and anchor window with one metric per name, each anchored at `anchor`, and a
/// manifest declaring what it carries - `LocalService::start` refuses an undeclared contribution.
fn bundle_of(metrics: &[&str], anchor: &str) -> PinnedDefinitions {
    let column = |raw: &str| ColumnName::parse(raw).expect("a test column is a column");
    let model = Model::new(
        ModelName::parse("orders").expect("a test model is a model"),
        source(),
        TableName::parse("orders").expect("a test table is a table"),
        BTreeSet::from([column("amount_cents"), column("order_date")]),
        Description::default(),
    );
    let metrics = metrics
        .iter()
        .map(|name| {
            Metric::new(
                MetricName::parse(name).expect("a test metric name is a name"),
                ModelName::parse("orders").expect("a test model is a model"),
                Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
                Vec::new(),
                column("order_date"),
                BTreeSet::from([Grain::Month]),
                Vec::new(),
                Some(Anchor::new(
                    june(),
                    AnchorValue::parse(anchor).expect("a test anchor value is a value"),
                )),
                Description::default(),
                Audience::Open,
            )
            .expect("no dimensions to duplicate")
        })
        .collect();
    let definitions = Definitions::assemble(vec![model], vec![], metrics).expect("the test bundle is consistent");
    // The real hasher, from the catalog adapter that owns the canonical form. `pin` applies it to
    // the definitions being pinned, so there is no digest here for the bundle not to describe.
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions,
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(MetadataCapabilities::of(
                DefinitionCapabilities::of([
                    DefinitionKind::Structure,
                    DefinitionKind::Metrics,
                    DefinitionKind::Grains,
                    DefinitionKind::Anchors,
                ]),
                KnowledgeCapabilities::none(),
            )),
        ),
    )
    .expect("the test definitions hash")
}

fn serving<W>(warehouse: W) -> Service<W>
where
    W: Warehouse + Send + Sync + 'static,
    W::Error: Send + Sync,
{
    LocalService::start(
        &FixedCatalog::of(served()),
        Warehouses::of(warehouse),
        DiscardingAuditSink,
        FixedBroker::GrantsShared,
        RefusingCombiner,
        1 << 30,
    )
    .expect("the certified warehouse reproduces the test bundle's anchor")
}

fn gated(gate: AdoptionGate<FixedWarehouse>) -> Service<FixedWarehouse> {
    serving(certified_warehouse()).with_adoption_gate(gate)
}

/// One row with a column per metric [`with_bookings`] declares, because each anchor reads its own.
fn certified_rows() -> RowSet {
    RowSet::new(
        vec![String::from("revenue"), String::from("bookings")],
        vec![vec![Value::Integer(197_122), Value::Integer(197_122)]],
    )
    .expect("two columns and two cells is rectangular")
}

fn certified_warehouse() -> FixedWarehouse {
    FixedWarehouse::answering(source(), shared(), certified_rows())
}

/// The served bundle plus a second metric: a different digest that validates against the same data.
fn with_bookings() -> PinnedDefinitions {
    bundle_of(&["revenue", "bookings"], "197122")
}

fn ask<S>(service: &S, name: &str) -> ToolOutcome
where
    S: Surface,
{
    let query = Query::single(
        MetricName::parse(name).expect("a test metric name is a name"),
        Grain::Month,
        june(),
        Vec::new(),
        Vec::new(),
    );
    service
        .answer(&asked_by_a_person(), &query, test_deadline())
        .expect("the fake answers or refuses, it does not fail")
}

fn digest_of(outcome: &ToolOutcome) -> &DefinitionDigest {
    let ToolOutcome::Answer { ref provenance, .. } = *outcome else {
        panic!("expected an answer, not {outcome:?}");
    };
    provenance.digest()
}

fn is_unknown_metric(outcome: &ToolOutcome) -> bool {
    matches!(outcome.refusal(), Some(&RefusalReason::MetricUnknown { .. }))
}

/// Answers every statement with [`certified_rows`], except that its first `execute` meets the test
/// thread twice: once to say the question is past its snapshot, once to be let go. Two barriers are
/// the whole ordering - no sleep. Every later `execute` runs straight through.
struct RendezvousWarehouse {
    source: SourceName,
    posture: SourcePosture,
    rows: RowSet,
    first: AtomicBool,
    reached: Arc<Barrier>,
    proceed: Arc<Barrier>,
}

impl Warehouse for RendezvousWarehouse {
    type Error = AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

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
        Ok(PreFlight::NotAsked)
    }

    fn execute(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        if self.first.swap(false, Ordering::SeqCst) {
            self.reached.wait();
            self.proceed.wait();
        }
        Ok(canned(&self.rows))
    }

    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(self.rows.clone()))
    }
}

#[test]
fn a_new_question_sees_a_metric_the_old_snapshot_lacked() {
    // Catches an `adopt` that validates and reports a rotation but never stores the bundle: the
    // second question would still be refused as unknown.
    let service = serving(certified_warehouse());
    assert!(
        is_unknown_metric(&ask(&service, "bookings")),
        "the booted bundle has no bookings metric"
    );
    let next = with_bookings();
    let next_digest = next.digest().clone();

    let adopted = service
        .adopt(next)
        .expect("the second bundle validates against the same data");

    assert_eq!(
        adopted,
        Adopted::Rotated {
            previous: served().digest().clone(),
            digest: next_digest.clone(),
        }
    );
    assert_eq!(digest_of(&ask(&service, "bookings")), &next_digest);
    assert_eq!(service.definitions().digest(), &next_digest);
}

#[test]
fn a_question_in_flight_answers_from_the_snapshot_it_started_with() {
    // Catches an `answer` that reads the served bundle again after its start: the rotation lands
    // while this question is inside `execute`, so a second read would stamp it with the new digest.
    let reached = Arc::new(Barrier::new(2));
    let proceed = Arc::new(Barrier::new(2));
    let service = serving(RendezvousWarehouse {
        source: source(),
        posture: shared(),
        rows: certified_rows(),
        first: AtomicBool::new(true),
        reached: Arc::clone(&reached),
        proceed: Arc::clone(&proceed),
    });

    let (in_flight, adopted) = std::thread::scope(|scope| {
        let question = scope.spawn(|| ask(&service, "revenue"));
        reached.wait();
        let adopted = service.adopt(with_bookings());
        proceed.wait();
        (question.join().expect("the question thread does not panic"), adopted)
    });

    assert!(
        matches!(adopted, Ok(Adopted::Rotated { .. })),
        "the rotation landed mid-question, not {adopted:?}"
    );
    assert_eq!(digest_of(&in_flight), served().digest());
    let after = ask(&service, "bookings");
    assert_eq!(digest_of(&after), with_bookings().digest());
}

#[test]
fn a_bundle_that_does_not_validate_leaves_the_served_one_in_place() {
    // Catches an `adopt` that stores before, or regardless of, the anchor check: an anchor the data
    // does not reproduce must leave the old bundle serving.
    let service = serving(certified_warehouse());

    let refused = service.adopt(bundle_of(&["revenue", "bookings"], "1"));

    assert!(
        matches!(refused, Err(NotAdopted::NotValidated { .. })),
        "an unreproduced anchor refuses, not {refused:?}"
    );
    assert_eq!(service.definitions().digest(), served().digest());
    assert!(is_unknown_metric(&ask(&service, "bookings")));
    assert_eq!(digest_of(&ask(&service, "revenue")), served().digest());
}

#[test]
fn a_bundle_the_preflight_refuses_leaves_the_served_one_in_place() {
    // Catches an `adopt` that skips the deployment's pre-flight: the bundle validates, so only the
    // gate stands between it and the next question.
    let service = gated(Box::new(|_, _| {
        Err(ErasedCause::from("model orders names a table this process does not hold"))
    }));

    let refused = service.adopt(with_bookings());

    let Err(NotAdopted::Preflight { ref cause }) = refused else {
        panic!("the gate's refusal is a pre-flight refusal, not {refused:?}");
    };
    assert_eq!(cause.to_string(), "model orders names a table this process does not hold");
    assert_eq!(service.definitions().digest(), served().digest());
    assert!(is_unknown_metric(&ask(&service, "bookings")));
}

#[test]
fn an_unchanged_bundle_is_not_offered_to_the_gate() {
    // Catches an `adopt` that re-checks the digest already served: every poll would run the
    // pre-flight and every anchor again against the data systems for nothing.
    let offered = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&offered);
    let service = gated(Box::new(move |_, _| {
        counted.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }));

    assert_eq!(
        service.adopt(served()).expect("the served bundle is unchanged"),
        Adopted::Unchanged
    );
    assert_eq!(offered.load(Ordering::SeqCst), 0);
    assert!(matches!(service.adopt(with_bookings()), Ok(Adopted::Rotated { .. })));
    assert_eq!(offered.load(Ordering::SeqCst), 1);
}
