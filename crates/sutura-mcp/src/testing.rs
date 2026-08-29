//! Fakes for the ports, and a bundle to serve.
//!
//! Compiled only under `cfg(test)`. Fakes rather than a stubbed transport, which is the whole reason
//! the surface is a port: the handler, the tool schema and every refusal are exercisable with no
//! catalog directory, no data system and no process to spawn.
//!
//! The bundle here is deliberately the smallest thing that can be *validated*: one model, one
//! metric, one anchor. A bundle with no anchor validates against any warehouse at all, which would
//! make the readiness gate look like it worked when nothing had been executed - so the tests that
//! answer go through the real `sutura_app::LocalService`, and a service that exists is one whose
//! anchor reproduced the number its author certified.
//!
//! [`FailingSurface`] is the exception, and it is a fake of the *driving* port rather than of a
//! driven one. `SurfaceFailure` is what a transport must not confuse with a refusal, and the honest
//! instrument for it is a surface that fails - not a warehouse that has to answer an anchor first and
//! then stop, which would need interior mutability to say something a two-line fake says plainly.
//!
//! This duplicates `sutura_http`'s own fixtures, and it is the same duplication `crate::wire`
//! explains: `testing` there is `cfg(test)`, so there is nothing to share even if an adapter were
//! allowed to reach into another adapter, which it is not.

use std::collections::{BTreeMap, BTreeSet};

use sutura_app::surface::{Surface, SurfaceFailure};
use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::catalog::{Anchor, Definitions, Description, Dimension, DimensionValue, Metric, Model};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, ModelName, SourceName, TableName};
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog};
use sutura_domain::plan::QueryPlan;
use sutura_domain::query::{Query, ToolOutcome};
use sutura_domain::warehouse::{RowSet, Value, Warehouse};

/// The number the anchor certifies, and the number the answering fake reproduces.
pub(crate) const ANCHORED_VALUE: i64 = 197_122;

pub(crate) fn source() -> SourceName {
    SourceName::parse("local").expect("a test source is a source")
}

fn description(raw: &str) -> Description {
    Description::parse(raw).expect("a test description is a description")
}

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a test column is a column")
}

pub(crate) fn june() -> TimeRange {
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
        description("Orders, one row per order."),
    );
    let region = Dimension::new(
        DimensionName::parse("region").expect("a test dimension is a dimension"),
        column("region"),
        None,
        Some(BTreeSet::from([
            DimensionValue::parse("north").expect("a test value is a value"),
            DimensionValue::parse("south").expect("a test value is a value"),
        ])),
        description("Sales region."),
    );
    let revenue = Metric::new(
        MetricName::parse("revenue").expect("a test metric is a metric"),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Day, Grain::Month]),
        BTreeMap::from([(
            DimensionName::parse("region").expect("a test dimension is a dimension"),
            region,
        )]),
        Some(Anchor::new(june(), ANCHORED_VALUE.to_string())),
        description("Revenue, in minor units."),
    );
    let definitions = Definitions::assemble(vec![model], vec![], vec![revenue]).expect("the test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions,
        Knowledge::none(),
    )
    .expect("the test definitions hash")
}

/// A catalog port that hands back the bundle above.
pub(crate) struct FixedCatalog;

/// Never returned by [`FixedCatalog`] or [`FakeWarehouse`]; both ports require an error type.
#[derive(Debug, thiserror::Error)]
#[error("a fixture cannot fail")]
pub(crate) struct Unreachable;

impl SemanticCatalog for FixedCatalog {
    type Error = Unreachable;

    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        Ok(bundle())
    }
}

/// A data system that answers every statement with one prepared result.
///
/// One result for every plan, which is exactly enough: nothing in the transport depends on the
/// number, and the anchor check only needs the metric's own column to carry it.
pub(crate) struct FakeWarehouse {
    source: SourceName,
    result: RowSet,
}

impl Warehouse for FakeWarehouse {
    type Error = Unreachable;

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
    FakeWarehouse {
        source: source(),
        result: RowSet::new(vec![String::from("revenue")], vec![vec![Value::Integer(ANCHORED_VALUE)]])
            .expect("a one-cell result is a result set"),
    }
}

/// The data system's own complaint, under the surface failure that carries it.
#[derive(Debug, thiserror::Error)]
#[error("connection refused")]
pub(crate) struct ConnectionRefused;

/// A surface that answers nothing, so a transport's failure path can be asserted.
///
/// It still hands back a real bundle, because a `Surface` is a validated bundle plus a data system
/// and only the second half is broken here.
pub(crate) struct FailingSurface {
    definitions: PinnedDefinitions,
}

impl FailingSurface {
    pub(crate) fn new() -> Self {
        Self { definitions: bundle() }
    }
}

impl Surface for FailingSurface {
    fn definitions(&self) -> &PinnedDefinitions {
        &self.definitions
    }

    fn answer(&self, _query: &Query) -> Result<ToolOutcome, SurfaceFailure> {
        Err(SurfaceFailure::Warehouse {
            cause: Box::new(ConnectionRefused),
        })
    }
}
