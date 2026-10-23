//! Refusals provoked through the public entry points - `answer`, `run_sql`, `assemble` and
//! `LocalService` - with the shared fakes, asserting the typed variant each one maps to. Every
//! cell pins behaviour the tree already has, so each is a declared claim cell whose killing
//! mutation lives under `devco/claim-mutations/`.

use std::collections::BTreeSet;

use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{Audience, Definitions, Description, Metric, Model};
use sutura_domain::identity::PresentedDisagreesWithPosture;
use sutura_domain::knowledge::{Knowledge, KnowledgeCapabilities};
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{Aggregate, ColumnName, Grain, ModelName, SourceName, TableName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};
use sutura_domain::plan::{Ceilings, RefusingCombiner};
use sutura_domain::query::Query;
use sutura_domain::raw::{RawRefusalReason, RawStatement};

use crate::assemble::{CompositionError, assemble};
use crate::spend::SpendLedger;
use crate::surface::{LocalService, ServiceNotStarted, Surface as _, SurfaceFailure};
use crate::tests::{asked_by_a_person, certified, june, metric, shared, source, test_deadline};
use crate::tests_support::{
    AuthoredWarehouse, CountingBroker, DiscardingAuditSink, FixedBroker, FixedCatalog, FixedWarehouse, RawCapableWarehouse,
    authored_bundle,
};
use crate::warehouses::Warehouses;
use crate::{RunSqlError, ServiceError, run_sql, verify_and_validate};

#[test]
fn an_authored_metric_through_answer_is_a_compile_failure() {
    let working = Warehouses::of(AuthoredWarehouse::new(source(), shared()));
    let validated = verify_and_validate(authored_bundle(metric(), source()), &working).expect("no anchors to check");
    let failure = crate::answer(
        &validated,
        &question(),
        &asked_by_a_person(),
        &FixedBroker::GrantsShared,
        &working,
        &RefusingCombiner,
        1 << 30,
        test_deadline(),
        &SpendLedger::no_budget(),
        Ceilings::DEFAULT,
    )
    .expect_err("an authored metric has no representation in the semantic plan");
    assert!(
        matches!(failure, ServiceError::Compile { .. }),
        "the authored metric is a compile failure, not {failure:?}"
    );
}

#[test]
fn an_authored_metric_through_the_surface_is_a_compile_failure() {
    let failure = service(
        authored_bundle(metric(), source()),
        AuthoredWarehouse::new(source(), shared()),
        FixedBroker::GrantsShared,
    )
    .answer(&asked_by_a_person(), &question(), test_deadline())
    .expect_err("an authored metric has no representation in the semantic plan");
    assert!(
        matches!(failure, SurfaceFailure::Compile { .. }),
        "the surface maps the compile failure, not {failure:?}"
    );
}

#[test]
fn an_unreachable_broker_through_the_surface_is_a_broker_failure() {
    let failure = service(declared(), FixedWarehouse::new(source(), shared()), FixedBroker::Unreachable)
        .answer(&asked_by_a_person(), &question(), test_deadline())
        .expect_err("an unreachable broker is a failure");
    assert!(
        matches!(failure, SurfaceFailure::Broker { .. }),
        "the surface maps the broker failure, not {failure:?}"
    );
}

#[test]
fn credentials_that_do_not_fit_through_the_surface_are_a_miswiring() {
    let failure = service(
        declared(),
        FixedWarehouse::new(source(), shared()),
        FixedBroker::GrantsTheWrongSource,
    )
    .answer(&asked_by_a_person(), &question(), test_deadline())
    .expect_err("a grant for the wrong source is a wiring defect");
    assert!(
        matches!(failure, SurfaceFailure::Miswired { .. }),
        "the surface maps the credential mismatch, not {failure:?}"
    );
}

#[test]
fn two_catalogs_defining_the_same_metric_are_refused_at_startup() {
    let error = LocalService::start_composed(
        &[
            FixedCatalog::of(pinned("test-1", single("alpha"))),
            FixedCatalog::of(pinned("test-1", single("beta"))),
        ],
        Warehouses::of(FixedWarehouse::answering(source(), shared(), certified())),
        DiscardingAuditSink,
        FixedBroker::GrantsShared,
        RefusingCombiner,
        1 << 30,
    )
    .expect_err("two sources defining one metric must not start");
    let ServiceNotStarted::Composition { cause } = error else {
        panic!("the composition failure is a startup refusal, not {error:?}");
    };
    assert!(
        matches!(cause, CompositionError::MetricCollision { .. }),
        "the composition failure is a metric collision, not {cause:?}"
    );
}

#[test]
fn an_empty_composition_is_refused() {
    let error = assemble(&[]).expect_err("an empty composition is not a bundle");
    assert!(
        matches!(error, CompositionError::Empty),
        "an empty slice is the empty variant, not {error:?}"
    );
}

#[test]
fn a_contribution_with_two_manifest_entries_is_refused() {
    let manifest = ContributionManifest::parse([
        (name("alpha"), Contribution::of(capabilities())),
        (name("beta"), Contribution::of(capabilities())),
    ])
    .expect("two distinct sources parse");
    let error = assemble(&[pinned("test-1", manifest)]).expect_err("a 2-entry manifest is not a single contribution");
    let CompositionError::NotASingleContribution { count } = error else {
        panic!("a 2-entry manifest is not a single contribution, not {error:?}");
    };
    assert_eq!(count, 2);
}

#[test]
fn two_bundles_with_different_versions_are_refused() {
    let error = assemble(&[pinned("test-1", single("local")), pinned("test-2", single("local"))])
        .expect_err("two versions must not compose");
    let CompositionError::VersionMismatch { first, second } = error else {
        panic!("a version mismatch is not {error:?}");
    };
    assert_eq!((first.as_str(), second.as_str()), ("test-1", "test-2"));
}

#[test]
fn an_unreachable_broker_through_run_sql_is_a_broker_failure() {
    let failure = run_sql(
        &asked_by_a_person(),
        &select_one(),
        &FixedBroker::Unreachable,
        &raw_capable(),
        test_deadline(),
    )
    .expect_err("an unreachable broker is a failure");
    assert!(
        matches!(failure, RunSqlError::Broker { .. }),
        "the broker failure is typed, not {failure:?}"
    );
}

#[test]
fn credentials_for_the_wrong_source_through_run_sql_are_a_credentials_failure() {
    let failure = run_sql(
        &asked_by_a_person(),
        &select_one(),
        &FixedBroker::GrantsTheWrongSource,
        &raw_capable(),
        test_deadline(),
    )
    .expect_err("a grant for the wrong source is a wiring defect");
    assert!(
        matches!(failure, RunSqlError::Credentials { .. }),
        "the credential mismatch is typed, not {failure:?}"
    );
}

/// Declined before a broker is asked, so each of the next two cells also holds `asked() == 0`: a
/// refusal that moved past the mint would have run credentials for a statement it then refused.
fn refused_before_minting(answered: &crate::AnsweredRaw, code: &str, broker: &CountingBroker) {
    assert_eq!(
        answered.outcome().refusal().map(RawRefusalReason::code),
        Some(code),
        "{:?}",
        answered.outcome()
    );
    assert!(
        answered.executed_until().is_none(),
        "a statement declined before minting ran under no credentials"
    );
    assert_eq!(broker.asked(), 0, "the refusal comes before any mint");
}

#[test]
fn run_sql_over_one_adapter_that_takes_no_raw_statement_is_a_refusal_the_caller_reads() {
    // `FixedWarehouse` does not accept raw statements, so the one open source cannot serve the tool.
    let broker = CountingBroker::default();
    let answered = run_sql(
        &asked_by_a_person(),
        &select_one(),
        &broker,
        &Warehouses::of(FixedWarehouse::new(source(), shared())),
        test_deadline(),
    )
    .expect("a deployment shape the tool does not serve is an answer, not a failure");
    refused_before_minting(&answered, "no_source_accepts_raw_statements", &broker);
}

#[test]
fn run_sql_over_two_open_sources_is_a_refusal_the_caller_reads() {
    let broker = CountingBroker::default();
    let both = Warehouses::of(RawCapableWarehouse::answering_rows(source(), shared(), 1))
        .and(RawCapableWarehouse::answering_rows(name("other"), shared(), 1))
        .expect("two distinct sources open together");
    let answered = run_sql(&asked_by_a_person(), &select_one(), &broker, &both, test_deadline())
        .expect("a deployment shape the tool does not serve is an answer, not a failure");
    refused_before_minting(&answered, "more_than_one_source_open", &broker);
}

#[test]
fn the_surface_answers_run_sql_over_a_shape_the_tool_does_not_serve_with_a_refusal_not_a_miswiring() {
    let outcome = service(declared(), FixedWarehouse::new(source(), shared()), FixedBroker::GrantsShared)
        .run_sql(&asked_by_a_person(), &select_one(), test_deadline())
        .expect("the surface answers a refusal rather than failing");
    assert_eq!(
        outcome.refusal().map(RawRefusalReason::code),
        Some("no_source_accepts_raw_statements"),
        "{outcome:?}"
    );
}

#[test]
fn an_adapter_declaring_raw_and_answering_none_stays_a_wiring_defect() {
    let declined = RawStatement::parse("decline me").expect("a test statement is a statement");
    let failure = run_sql(
        &asked_by_a_person(),
        &declined,
        &FixedBroker::GrantsShared,
        &raw_capable(),
        test_deadline(),
    )
    .expect_err("an adapter that declares raw and answers none is not a refusal the caller can read");
    assert!(matches!(failure, RunSqlError::NoAcceptingSource), "{failure:?}");

    let surfaced = service(
        declared(),
        RawCapableWarehouse::answering_rows(source(), shared(), 1),
        FixedBroker::GrantsShared,
    )
    .run_sql(&asked_by_a_person(), &declined, test_deadline())
    .expect_err("the surface reports it as a failure, not an answer");
    assert!(matches!(surfaced, SurfaceFailure::Miswired { .. }), "{surfaced:?}");
}

/// A refusal declined before any mint is still a call, so it must reach the sink: one `RawRefused`
/// record, carrying the statement, the same as a refusal after execution.
#[test]
fn run_sql_refused_over_an_unserved_shape_writes_one_audit_record_carrying_the_statement() {
    use sutura_domain::audit::{AuditSink, CallRecord, RecordedOutcome};

    struct RecordingSink(std::sync::mpsc::Sender<String>);

    impl AuditSink for RecordingSink {
        fn record(&self, record: &CallRecord<'_>) {
            let RecordedOutcome::RawRefused { reason, statement } = *record.outcome() else {
                drop(self.0.send(String::from("not a raw refusal")));
                return;
            };
            drop(self.0.send(format!("{} {}", reason.code(), statement.as_str())));
        }
    }

    fn audited<W>(warehouses: Warehouses<W>) -> Vec<String>
    where
        W: sutura_domain::warehouse::Warehouse + Send + Sync + 'static,
        W::Error: Send + Sync,
    {
        let (sender, receiver) = std::sync::mpsc::channel();
        let service = LocalService::start(
            &FixedCatalog::of(declared()),
            warehouses,
            RecordingSink(sender),
            FixedBroker::GrantsShared,
            RefusingCombiner,
            1 << 30,
        )
        .expect("a bundle with no anchors boots against any warehouse");
        drop(
            service
                .run_sql(&asked_by_a_person(), &select_one(), test_deadline())
                .expect("a deployment shape the tool does not serve is an answer, not a failure"),
        );
        receiver.try_iter().collect()
    }

    let two = Warehouses::of(RawCapableWarehouse::answering_rows(source(), shared(), 1))
        .and(RawCapableWarehouse::answering_rows(name("other"), shared(), 1))
        .expect("two distinct sources open together");
    assert_eq!(audited(two), ["more_than_one_source_open select 1"]);
    assert_eq!(
        audited(Warehouses::of(FixedWarehouse::new(source(), shared()))),
        ["no_source_accepts_raw_statements select 1"]
    );
}

#[test]
fn run_sql_handed_a_subject_leg_for_a_shared_source_is_a_posture_failure() {
    let failure = run_sql(
        &asked_by_a_person(),
        &select_one(),
        &FixedBroker::GrantsSubjectMaterial,
        &raw_capable(),
        test_deadline(),
    )
    .expect_err("a subject leg for a shared source is a wiring defect");
    assert!(
        matches!(
            failure,
            RunSqlError::Posture {
                cause: PresentedDisagreesWithPosture::ShapeIsNotThePosture { .. }
            }
        ),
        "the posture mismatch is typed, not {failure:?}"
    );
}

fn question() -> Query {
    Query::single(metric(), Grain::Month, june(), Vec::new(), Vec::new())
}

/// Boots a `LocalService` over one bundle and one warehouse.
fn service<W>(
    bundle: PinnedDefinitions,
    warehouse: W,
    broker: FixedBroker,
) -> LocalService<W, DiscardingAuditSink, FixedBroker, RefusingCombiner>
where
    W: sutura_domain::warehouse::Warehouse + Send + Sync + 'static,
    W::Error: Send + Sync,
{
    LocalService::start(
        &FixedCatalog::of(bundle),
        Warehouses::of(warehouse),
        DiscardingAuditSink,
        broker,
        RefusingCombiner,
        1 << 30,
    )
    .expect("a bundle with no anchors boots against any warehouse")
}

fn raw_capable() -> Warehouses<RawCapableWarehouse> {
    Warehouses::of(RawCapableWarehouse::answering_rows(source(), shared(), 1))
}

fn select_one() -> RawStatement {
    RawStatement::parse("select 1").expect("a test statement is a statement")
}

fn name(raw: &str) -> SourceName {
    SourceName::parse(raw).expect("a test source is a source")
}

/// What `pinned` holds, declared, so validation has nothing to refuse.
fn capabilities() -> MetadataCapabilities {
    MetadataCapabilities::of(
        DefinitionCapabilities::of([DefinitionKind::Structure, DefinitionKind::Metrics, DefinitionKind::Grains]),
        KnowledgeCapabilities::none(),
    )
}

fn single(contributor: &str) -> ContributionManifest {
    ContributionManifest::single(name(contributor), Contribution::of(capabilities()))
}

fn declared() -> PinnedDefinitions {
    pinned("test-1", single("local"))
}

/// `tests::bundle()` without its anchor, at `version`, contributed as `manifest` says.
fn pinned(version: &str, manifest: ContributionManifest) -> PinnedDefinitions {
    let column = |raw: &str| ColumnName::parse(raw).expect("a test column is a column");
    let orders = || ModelName::parse("orders").expect("a test model is a model");
    let model = Model::new(
        orders(),
        source(),
        TableName::parse("orders").expect("a test table is a table"),
        BTreeSet::from([column("amount_cents"), column("order_date")]),
        Description::default(),
    );
    let revenue = Metric::new(
        metric(),
        orders(),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("no dimensions to duplicate");
    PinnedDefinitions::pin(
        DefinitionVersion::parse(version).expect("a test version is a version"),
        Definitions::assemble(vec![model], vec![], vec![revenue]).expect("the test bundle is consistent"),
        Knowledge::none(),
        manifest,
    )
    .expect("the test definitions hash")
}
