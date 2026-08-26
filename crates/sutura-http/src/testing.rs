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
        Some(Anchor::new(june(), String::from(ANCHORED_VALUE))),
        String::from("Revenue, in minor units."),
    );
    let definitions = Definitions::assemble(vec![model], vec![], vec![revenue]).expect("the test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions,
        sutura_catalog_local::digest_of,
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
        Ok(self.result.clone())
    }
}

/// A warehouse whose one answer reproduces the anchor, so the bundle validates.
pub(crate) fn fake_warehouse() -> FakeWarehouse {
    let result = RowSet::new(vec![String::from("revenue")], vec![vec![Value::Integer(197_122)]])
        .expect("a one-cell result is a result set");
    FakeWarehouse {
        source: source(),
        result,
    }
}
