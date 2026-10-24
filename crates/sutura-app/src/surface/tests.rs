//! `LocalService`'s own cells, moved out of `surface.rs` for the 1000-line cap.

use sutura_domain::model::Grain;
use sutura_domain::pinned::NotValidated;
use sutura_domain::pinned::view::ScopedView;
use sutura_domain::query::Query;

use super::{LocalService, ServiceNotStarted, Surface as _};
use crate::tests::{asked_by_a_person, june, metric, shared, source, test_deadline};
use crate::tests_support::{
    AuthoredWarehouse, DiscardingAuditSink, FixedBroker, FixedCatalog, RawCapableWarehouse, authored_bundle, bundle_over,
};
use crate::{Warehouses, verify_and_validate};

#[test]
fn an_authored_metric_does_not_boot_against_the_in_process_engine() {
    // The mechanism under test is `Warehouse::EXECUTES_AUTHORED_SQL` read inside
    // `verify_and_validate`, through the real composition path and the one adapter every shipped
    // binary links. Red on base by construction (`authored_bundle` cannot be built there); the
    // compiled mutation is `const EXECUTES_AUTHORED_SQL: bool = true;` on `DataFusionWarehouse`,
    // under which this test alone goes red.
    let catalog = FixedCatalog::of(authored_bundle(metric(), source()));
    let ceiling = core::num::NonZeroUsize::new(1 << 30).expect("a gibibyte is positive");
    let engine = sutura_exec_datafusion::DataFusionWarehouse::new(
        source(),
        shared(),
        sutura_exec_datafusion::WorkingSet::of_bytes(ceiling),
    )
    .expect("the in-process engine starts");
    let error = LocalService::start(
        &catalog,
        Warehouses::of(engine),
        DiscardingAuditSink,
        FixedBroker::GrantsShared,
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect_err("the in-process engine cannot execute authored SQL");
    let ServiceNotStarted::NotValidated { cause } = error else {
        panic!("the authored computation must be the startup refusal: {error:?}");
    };
    assert_eq!(cause, NotValidated::AuthoredSqlNotExecutable { metric: metric() });
}

#[test]
fn the_authored_sql_example_does_not_boot_against_the_in_process_engine() {
    // The other half of the fixture above, over the directory `examples/authored-sql` and the
    // README under it actually claim about: not a hand-built bundle, but
    // `LocalCatalog::load` through the real composition path `sutura query`, `sutura mcp` and
    // `sutura serve` all take. `LocalCatalog::capabilities()` is `everything()`
    // (`crates/sutura-catalog-local/src/lib.rs`), so composing this directory first requires the
    // example to carry every kind that declares: a second model, a relationship, a dimension
    // reached through it, a required filter, an anchor, and the four knowledge documents. Red
    // before those existed, at `CompositionError::Unfaithful` - a one-model, one-metric catalog
    // does not compose, which a review of this checkpoint found before the example carried
    // enough to reach the authored check at all. The compiled mutation is the same as the test
    // above.
    use std::path::Path;

    use sutura_catalog_local::LocalCatalog;
    use sutura_domain::pinned::DefinitionVersion;

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/authored-sql/catalog");
    let version = DefinitionVersion::parse("authored-sql-example").expect("a fixed version is a version");
    let catalog = LocalCatalog::new(source(), root, version);
    let ceiling = core::num::NonZeroUsize::new(1 << 30).expect("a gibibyte is positive");
    let engine = sutura_exec_datafusion::DataFusionWarehouse::new(
        source(),
        shared(),
        sutura_exec_datafusion::WorkingSet::of_bytes(ceiling),
    )
    .expect("the in-process engine starts");
    let error = LocalService::start(
        &catalog,
        Warehouses::of(engine),
        DiscardingAuditSink,
        FixedBroker::GrantsShared,
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect_err("the example carries an authored metric the in-process engine cannot execute");
    let ServiceNotStarted::NotValidated { cause } = error else {
        panic!("the example must compose and then hit the authored-SQL refusal: {error:?}");
    };
    let metric = sutura_domain::model::MetricName::parse("order_value_spread").expect("the example metric name is a name");
    assert_eq!(cause, NotValidated::AuthoredSqlNotExecutable { metric });
}

#[test]
fn an_adapter_declaring_authored_sql_support_passes_only_the_capability_gate() {
    // The other direction, so the gate above is a capability read and not an unconditional
    // refusal of the key. What the declaring fake does NOT get is an answer: the plan carries no
    // SQL, so the compiler names the metric rather than substituting a measure. No adapter this
    // workspace ships makes the declaration; the fake exists to hold the gate's shape.
    let pinned = authored_bundle(metric(), source());
    verify_and_validate(pinned.clone(), &Warehouses::of(AuthoredWarehouse::new(source(), shared())))
        .expect("the declaring fake passes startup because this bundle has no anchors");
    let question = Query::single(metric(), Grain::Month, june(), Vec::new(), Vec::new());
    let error = sutura_semantic::compile(
        &question,
        &ScopedView::everything(&pinned),
        sutura_domain::plan::Ceilings::DEFAULT,
    )
    .expect_err("an authored computation has no representation in the semantic plan");
    match error {
        sutura_semantic::CompileFailure::AuthoredSqlNotPlanned { metric: failed } => assert_eq!(failed, metric()),
        other => panic!("the compiler must name the authored metric rather than substitute a measure: {other:?}"),
    }
}

/// `#666`'s review, finding 3: the constructor `CallRecord::of_raw` was tested directly, but
/// nothing reached `LocalService::run_sql`'s own `self.sink.record(...)` call - a mutation that
/// builds the record and never sinks it (`crates/sutura-app/src/surface.rs:449-454`) left the
/// whole suite green. This calls `run_sql` through the real `Surface` implementation, over a
/// sink that only a genuine `record` call can reach.
#[test]
fn run_sql_writes_one_record_per_outcome_carrying_the_statement_text() {
    use sutura_domain::audit::{AuditSink, CallRecord, RecordedOutcome};
    use sutura_domain::raw::RawStatement;

    /// `Send + Sync + 'static`, without `std::sync::Mutex` (`clippy.toml` disallows it) or a
    /// `tokio` dependency this crate does not have: a channel is `Sync` for a `Send` item and
    /// needs neither.
    struct RecordingSink {
        sender: std::sync::mpsc::Sender<String>,
    }

    impl AuditSink for RecordingSink {
        fn record(&self, record: &CallRecord<'_>) {
            let statement = record.statement().map(|text| text.as_str().to_owned());
            let line = match *record.outcome() {
                RecordedOutcome::RawAnswered { rows, .. } => format!("raw_answered rows={rows} statement={statement:?}"),
                RecordedOutcome::RawRefused { reason, .. } => {
                    format!("raw_refused reason={reason:?} statement={statement:?}")
                }
                RecordedOutcome::Answered { .. } | RecordedOutcome::Refused { .. } => String::from("certified"),
                RecordedOutcome::Failed { .. } | RecordedOutcome::RawFailed { .. } => String::from("failed"),
            };
            drop(self.sender.send(line));
        }
    }

    let (sender, receiver) = std::sync::mpsc::channel();
    // An empty catalog: no metric, so no anchor `LocalService::start` would re-run against
    // this fake - `RawCapableWarehouse` answers only the raw path, deliberately, and boot must
    // not touch the certified one to reach it.
    let catalog = FixedCatalog::of(bundle_over(&[]));
    let warehouse = RawCapableWarehouse::answering_rows(source(), shared(), 1);
    let service = LocalService::start(
        &catalog,
        Warehouses::of(warehouse),
        RecordingSink { sender },
        FixedBroker::GrantsShared,
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("an empty catalog with no anchors boots against any warehouse");

    let context = asked_by_a_person();
    let answered_statement = RawStatement::parse("select 1").expect("a test statement is a statement");
    drop(
        service
            .run_sql(&context, &answered_statement, test_deadline())
            .expect("the fake warehouse answers `select 1`"),
    );
    let refused_statement = RawStatement::parse("refuse me").expect("a test statement is a statement");
    drop(
        service
            .run_sql(&context, &refused_statement, test_deadline())
            .expect("a refusal is a result, not an `Err`"),
    );

    let lines: Vec<String> = receiver.try_iter().collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].starts_with("raw_answered"), "{lines:?}");
    assert!(lines[0].contains("select 1"), "{lines:?}");
    assert!(lines[1].starts_with("raw_refused"), "{lines:?}");
    assert!(lines[1].contains("refuse me"), "{lines:?}");
}

#[test]
fn a_caller_with_no_linked_account_is_not_an_identity_provider_outage() {
    use sutura_domain::identity::DelegationFailed;
    let at_hop = |cause| DelegationFailed::AtHop {
        hop: 1,
        cause: Box::new(cause),
    };
    let not_linked = super::broker_failure(Box::new(at_hop(DelegationFailed::NotLinked)));
    assert!(
        matches!(not_linked, super::SurfaceFailure::NotLinked { .. }),
        "{not_linked:?}"
    );
    let refused = super::broker_failure(Box::new(at_hop(DelegationFailed::Refused {
        status: 400,
        error: Some(String::from("invalid_request")),
    })));
    assert!(matches!(refused, super::SurfaceFailure::Broker { .. }), "{refused:?}");
}
