//! The catalog-bundle fixtures shared by `super`'s cells, split out when that file crossed
//! the unexemptable 1000-line cap. Every item here was already there - moved, not written -
//! so this file orphans nothing and adds no new cell; `crate::tests::bundle_over` still
//! resolves, through the re-export `super` keeps.

use std::collections::BTreeSet;

use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Definitions, Description, Model};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::model::{ColumnName, ModelName, SourceName, TableName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

use super::{ENGINE_SOURCE, entry, registry};

/// The ordinary declaration: the engine source, shared, over the example data.
pub(crate) fn engine_declared() -> sutura_config::SourceRegistry {
    registry(&entry(ENGINE_SOURCE, "shared-service-user", ""))
}

/// One model as a catalog document names it: the model, its data system, its table.
pub(crate) type DeclaredModel<'raw> = (&'raw str, &'raw str, &'raw str);

/// A pinned bundle over exactly the models given, and no metrics.
///
/// Models are all `open_engine` reads: [`sutura_app::sources`] maps over them and `attach` is
/// called once per model, so a metric would add nothing any arm of that function looks at.
/// Leaving them out is what lets one helper stand behind every arm below.
///
/// `pub(crate)` so `crate::boot`'s own tests build their bundles the same way rather than growing a
/// second copy of this that could drift from what a document really produces. Both modules are
/// `#[cfg(test)]`, so nothing compiled into the binary can reach it.
pub(crate) fn bundle_over(models: &[DeclaredModel<'_>]) -> PinnedDefinitions {
    let declared: Vec<Model> = models
        .iter()
        .map(|&(model, source, table)| {
            Model::new(
                ModelName::parse(model).expect("a test model is a model"),
                SourceName::parse(source).expect("a test source is a source"),
                TableName::parse(table).expect("a test table is a table"),
                BTreeSet::from([ColumnName::parse("customer_key").expect("a test column is a column")]),
                Description::default(),
            )
        })
        .collect();
    let definitions = Definitions::assemble(declared, vec![], vec![]).expect("the test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions,
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(MetadataCapabilities::nothing()),
        ),
    )
    .expect("the test definitions hash")
}

/// A bundle whose one metric declares an anchor, on `source`.
///
/// The anchor's NUMBER is irrelevant here and nothing executes it: what the boot check reads is
/// that an anchor exists and which source the metric's model sits on. The model is
/// `dim_customer`, so the table behind it is a real file - which keeps a refusal about identity
/// from being satisfied by a missing CSV.
pub(crate) fn bundle_with_an_anchor(source: &str) -> PinnedDefinitions {
    use sutura_domain::calendar::{Date, TimeRange};
    use sutura_domain::catalog::{Anchor, AnchorValue, Metric};
    use sutura_domain::measure::{AggregatedColumn, Measure, Term};
    use sutura_domain::model::{Aggregate, Grain, MetricName};

    let column = |raw: &str| ColumnName::parse(raw).expect("a test column is a column");
    let model = Model::new(
        ModelName::parse("customers").expect("a test model is a model"),
        SourceName::parse(source).expect("a test source is a source"),
        TableName::parse("dim_customer").expect("a test table is a table"),
        BTreeSet::from([column("customer_key"), column("signed_up_on")]),
        Description::default(),
    );
    let range = TimeRange::new(
        Date::parse("2026-06-01").expect("a test date is a date"),
        Date::parse("2026-07-01").expect("a test date is a date"),
    )
    .expect("June is a range");
    let metric = Metric::new(
        MetricName::parse("recurring_revenue").expect("a test metric is a metric"),
        ModelName::parse("customers").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(
            Aggregate::Count,
            column("customer_key"),
        ))),
        Vec::new(),
        column("signed_up_on"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        Some(Anchor::new(
            range,
            AnchorValue::parse("7").expect("a test anchor value is a value"),
        )),
        Description::default(),
    )
    .expect("no dimensions to duplicate");
    let definitions = Definitions::assemble(vec![model], vec![], vec![metric]).expect("the test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions,
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(MetadataCapabilities::nothing()),
        ),
    )
    .expect("the test definitions hash")
}
