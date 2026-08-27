//! Fakes for the two ports, and a bundle to serve.
//!
//! Compiled only under `cfg(test)`. Fakes rather than a stubbed HTTP layer, which is the whole
//! reason the surface is a port: the router, the refusals, the token gate and the limiter are all
//! exercisable with no catalog directory, no data system and no process to spawn.
//!
//! The bundle here is deliberately the smallest thing that can be *validated*: one model, one
//! metric, one anchor. A bundle with no anchor validates against any warehouse at all, which would
//! make the readiness gate look like it worked when nothing had been executed.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::catalog::{Anchor, Definitions, Dimension, Metric, Model};
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, ModelName, SourceName, TableName};
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog};
use sutura_domain::plan::QueryPlan;
use sutura_domain::warehouse::{RowSet, Value, Warehouse};

/// The number the anchor certifies, and the number the answering fake reproduces.
pub(crate) const ANCHORED_VALUE: &str = "197122";

/// The one data system name every fixture here uses.
pub(crate) fn source() -> SourceName {
    SourceName::parse("local").expect("a test source is a source")
}

pub(crate) fn metric_name() -> MetricName {
    MetricName::parse("revenue").expect("a test metric is a metric")
}

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a test column is a column")
}

fn june() -> TimeRange {
    TimeRange::new(
        Date::parse("2026-06-01").expect("a test date is a date"),
        Date::parse("2026-07-01").expect("a test date is a date"),
    )
    .expect("June is a range")
}

/// One model, one anchored metric, one filterable dimension.
pub(crate) fn bundle() -> PinnedDefinitions {
    pinned(Some(Anchor::new(june(), String::from(ANCHORED_VALUE))))
}

/// The same bundle with no anchor, so it validates against a data system that answers nothing.
///
/// **Only for tests about what happens AFTER startup.** A bundle with no anchor validates against
/// any warehouse at all - see the module documentation - which is exactly why it is not the default
/// fixture: it makes the readiness gate look like it worked when nothing had been executed. Here it
/// is what lets a service start over a warehouse that then fails every question.
pub(crate) fn unanchored_bundle() -> PinnedDefinitions {
    pinned(None)
}

/// A question the bundle above can answer, for a test that needs to reach the warehouse.
pub(crate) fn a_question() -> sutura_domain::query::Query {
    sutura_domain::query::Query::new(metric_name(), Grain::Month, june(), Vec::new(), Vec::new())
}

fn pinned(anchor: Option<Anchor>) -> PinnedDefinitions {
    let model = Model::new(
        ModelName::parse("orders").expect("a test model is a model"),
        source(),
        TableName::parse("orders").expect("a test table is a table"),
        BTreeSet::from([column("amount_cents"), column("order_date"), column("region")]),
        String::from("Orders, one row per order."),
    );
    let region = Dimension::new(
        DimensionName::parse("region").expect("a test dimension is a dimension"),
        column("region"),
        None,
        Some(BTreeSet::from([String::from("north"), String::from("south")])),
        String::from("Sales region."),
    );
    let revenue = Metric::new(
        metric_name(),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Day, Grain::Month]),
        BTreeMap::from([(
            DimensionName::parse("region").expect("a test dimension is a dimension"),
            region,
        )]),
        anchor,
        String::from("Revenue, in minor units."),
    );
    let definitions = Definitions::assemble(vec![model], vec![], vec![revenue]).expect("the test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions,
    )
    .expect("the test definitions hash")
}

/// A catalog port that hands back a bundle somebody else built.
pub(crate) struct FixedCatalog {
    bundle: PinnedDefinitions,
}

/// Never returned by [`FixedCatalog`]; the port requires an error type.
#[derive(Debug, thiserror::Error)]
#[error("a fixed catalog cannot fail")]
pub(crate) struct Infallible;

impl SemanticCatalog for FixedCatalog {
    type Error = Infallible;

    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        Ok(self.bundle.clone())
    }
}

pub(crate) fn catalog_of(bundle: PinnedDefinitions) -> FixedCatalog {
    FixedCatalog { bundle }
}

/// The driver's own complaint, one level below the adapter's.
#[derive(Debug, thiserror::Error)]
#[error("no such file: catalog/")]
pub(crate) struct DirectoryMissing;

/// What a catalog adapter returns, with the driver's cause underneath it.
#[derive(Debug, thiserror::Error)]
#[error("the catalog directory could not be read")]
pub(crate) struct CatalogUnreadable {
    #[source]
    cause: DirectoryMissing,
}

/// A catalog port that fails every read, with a cause worth keeping.
pub(crate) struct FailingCatalog;

impl SemanticCatalog for FailingCatalog {
    type Error = CatalogUnreadable;

    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        Err(CatalogUnreadable { cause: DirectoryMissing })
    }
}

/// The data system's own complaint.
#[derive(Debug, thiserror::Error)]
#[error("connection refused")]
pub(crate) struct ConnectionRefused;

/// What a warehouse adapter returns.
#[derive(Debug, thiserror::Error)]
#[error("the data system rejected the statement")]
pub(crate) struct StatementRejected {
    #[source]
    cause: ConnectionRefused,
}

/// A data system that fails every statement.
pub(crate) struct FailingWarehouse {
    source: SourceName,
}

impl FailingWarehouse {
    pub(crate) const fn new(source: SourceName) -> Self {
        Self { source }
    }
}

impl Warehouse for FailingWarehouse {
    type Error = StatementRejected;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn dry_run(&self, _plan: &QueryPlan) -> Result<(), Self::Error> {
        Err(StatementRejected {
            cause: ConnectionRefused,
        })
    }

    fn execute(&self, _plan: &QueryPlan) -> Result<RowSet, Self::Error> {
        Err(StatementRejected {
            cause: ConnectionRefused,
        })
    }
}

/// A data system that answers every statement with one prepared result.
///
/// One result for every plan, which is exactly enough: nothing in the transport layer depends on
/// the number, and the anchor check only needs the metric's own column to carry it.
pub(crate) struct FakeWarehouse {
    source: SourceName,
    result: RowSet,
    held: Arc<AtomicBool>,
}

impl Warehouse for FakeWarehouse {
    type Error = StatementRejected;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn dry_run(&self, _plan: &QueryPlan) -> Result<(), Self::Error> {
        Ok(())
    }

    fn execute(&self, _plan: &QueryPlan) -> Result<RowSet, Self::Error> {
        // Held rather than slept, and that is about the test suite rather than about realism. A
        // `spawn_blocking` task that sleeps keeps running after the assertion, and dropping a
        // `tokio` runtime waits for the blocking pool - so a fixed sleep long enough to outrun the
        // request bound would be added to the wall time of the whole suite. A flag the test clears
        // costs nothing once the assertion is made.
        //
        // The cap is a bug guard, not a timeout: it is what a forgotten `release` costs instead of
        // hanging the suite.
        let deadline = Instant::now() + HELD_AT_MOST;
        while self.held.load(Ordering::Relaxed) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(self.result.clone())
    }
}

/// The longest a held statement is held, whatever the test does.
const HELD_AT_MOST: Duration = Duration::from_secs(20);

/// Whether the fake is currently refusing to return.
///
/// A separate handle because the warehouse itself is moved into the service at `start`, and what a
/// test needs to hold is the switch rather than the adapter. It is armed only AFTER `start`, because
/// `start` re-executes every anchor and a held statement there would hold startup.
pub(crate) struct Held(Arc<AtomicBool>);

impl Held {
    /// From here on, a statement does not come back.
    pub(crate) fn arm(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Let whatever is waiting finish, so the blocking pool drains with the test.
    pub(crate) fn release(&self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

/// A warehouse whose one answer reproduces the anchor, so the bundle validates.
pub(crate) fn fake_warehouse() -> FakeWarehouse {
    warehouse_that_can_be_held().0
}

/// The same warehouse, with the switch that makes a statement outrun the request bound.
///
/// For the `408` assertion: the surface bounds the RESPONSE and not the work, so the way to observe
/// a timeout is a port call that has not returned yet.
pub(crate) fn warehouse_that_can_be_held() -> (FakeWarehouse, Held) {
    let result = RowSet::new(vec![String::from("revenue")], vec![vec![Value::Integer(197_122)]])
        .expect("a one-cell result is a result set");
    let held = Arc::new(AtomicBool::new(false));
    (
        FakeWarehouse {
            source: source(),
            result,
            held: Arc::clone(&held),
        },
        Held(held),
    )
}
