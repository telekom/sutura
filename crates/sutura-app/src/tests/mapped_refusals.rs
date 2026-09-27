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
use sutura_domain::plan::{RefusingCombiner, RowCeiling};
use sutura_domain::query::Query;
use sutura_domain::raw::RawStatement;

use crate::assemble::{CompositionError, assemble};
use crate::spend::SpendLedger;
use crate::surface::{LocalService, ServiceNotStarted, Surface as _, SurfaceFailure};
use crate::tests::{asked_by_a_person, certified, june, metric, shared, source, test_deadline};
use crate::tests_support::{
    AuthoredWarehouse, DiscardingAuditSink, FixedBroker, FixedCatalog, FixedWarehouse, RawCapableWarehouse, authored_bundle,
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
        RowCeiling::DEFAULT,
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
    let failure = run_sql(&asked_by_a_person(), &select_one(), &FixedBroker::Unreachable, &raw_capable())
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
    )
    .expect_err("a grant for the wrong source is a wiring defect");
    assert!(
        matches!(failure, RunSqlError::Credentials { .. }),
        "the credential mismatch is typed, not {failure:?}"
    );
}

#[test]
fn run_sql_over_no_adapter_that_accepts_a_raw_statement_is_refused_as_no_accepting_source() {
    // One warehouse is registered, but `FixedWarehouse` does not accept raw statements.
    let failure = run_sql(
        &asked_by_a_person(),
        &select_one(),
        &FixedBroker::GrantsShared,
        &Warehouses::of(FixedWarehouse::new(source(), shared())),
    )
    .expect_err("no adapter here accepts a raw statement");
    assert!(
        matches!(failure, RunSqlError::NoAcceptingSource),
        "the missing adapter is typed, not {failure:?}"
    );
}

#[test]
fn run_sql_handed_a_subject_leg_for_a_shared_source_is_a_posture_failure() {
    let failure = run_sql(
        &asked_by_a_person(),
        &select_one(),
        &FixedBroker::GrantsSubjectMaterial,
        &raw_capable(),
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
