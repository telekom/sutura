//! Fixtures for `describe_catalog(model: ...)`'s own tests - `github.com/telekom/sutura#2071`'s
//! second ask.
//!
//! A file of its own because these fixtures plus the rest of `testing.rs` cross the 1000-line cap
//! `cargo xtask max-lines` enforces.

use std::collections::BTreeSet;

use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Anchor, AnchorValue, Audience, Definitions, Metric, Model};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{Aggregate, AudienceId, ColumnName, Grain, MetricName, ModelName, SourceName, TableName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

use super::{ANCHORED_VALUE, column, description, june, source};

/// Two physical models, one of them with no metric at all - for `describe_catalog(model: ...)`'s
/// own tests: a caller naming `orders` sees `revenue`, a caller naming `customers` sees a real,
/// visible model with an empty metrics list, never a fallback to the whole catalog.
pub(crate) fn bundle_with_two_models() -> PinnedDefinitions {
    let orders = Model::new(
        ModelName::parse("orders").expect("a test model is a model"),
        source(),
        TableName::parse("orders").expect("a test table is a table"),
        BTreeSet::from([column("amount_cents"), column("order_date")]),
        description("Orders, one row per order."),
    );
    let customers = Model::new(
        ModelName::parse("customers").expect("a test model is a model"),
        source(),
        TableName::parse("customers").expect("a test table is a table"),
        BTreeSet::from([column("customer_id")]),
        description("Customers, one row per customer."),
    );
    let revenue = Metric::new(
        MetricName::parse("revenue").expect("a test metric is a metric"),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Day, Grain::Month]),
        Vec::new(),
        None,
        description("Revenue, in minor units."),
        Audience::Open,
    )
    .expect("no dimensions to duplicate");
    let definitions =
        Definitions::assemble(vec![orders, customers], vec![], vec![revenue]).expect("the test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions.clone(),
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(MetadataCapabilities::produced(&definitions, &Knowledge::none())),
        ),
    )
    .expect("the test definitions hash")
}

/// Two physical models, EACH owning its own metric - unlike [`bundle_with_two_models`], where
/// `customers` owns none. `describe_catalog(model: ...)`'s own narrowing test needs a metric that
/// an excluded model actually owns, so that asserting its absence proves the filter ran rather than
/// merely restating that the excluded model had nothing to filter in the first place.
pub(crate) fn bundle_with_two_metric_owning_models() -> PinnedDefinitions {
    let orders = Model::new(
        ModelName::parse("orders").expect("a test model is a model"),
        source(),
        TableName::parse("orders").expect("a test table is a table"),
        BTreeSet::from([column("amount_cents"), column("order_date")]),
        description("Orders, one row per order."),
    );
    let customers = Model::new(
        ModelName::parse("customers").expect("a test model is a model"),
        source(),
        TableName::parse("customers").expect("a test table is a table"),
        BTreeSet::from([column("customer_id")]),
        description("Customers, one row per customer."),
    );
    let revenue = Metric::new(
        MetricName::parse("revenue").expect("a test metric is a metric"),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Day, Grain::Month]),
        Vec::new(),
        None,
        description("Revenue, in minor units."),
        Audience::Open,
    )
    .expect("no dimensions to duplicate");
    let customer_count = Metric::new(
        MetricName::parse("customer_count").expect("a test metric is a metric"),
        ModelName::parse("customers").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(
            Aggregate::Count,
            column("customer_id"),
        ))),
        Vec::new(),
        column("customer_id"),
        BTreeSet::from([Grain::Day]),
        Vec::new(),
        None,
        description("How many customers, counted once each."),
        Audience::Open,
    )
    .expect("no dimensions to duplicate");
    let definitions = Definitions::assemble(vec![orders, customers], vec![], vec![revenue, customer_count])
        .expect("the test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions.clone(),
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(MetadataCapabilities::produced(&definitions, &Knowledge::none())),
        ),
    )
    .expect("the test definitions hash")
}

/// Enough physical models, each sized under [`sutura_domain::catalog::MAX_DESCRIPTION_BYTES`]'s own
/// cap on one [`sutura_domain::catalog::Description`] (4 KiB), that an unfiltered, physical-schema
/// `describe_catalog` crosses [`sutura_app::catalog_budget::MAX_CATALOG_LISTING_BYTES`]'s 64 KiB
/// bound on the whole listing - built for that bound's own tests, never for anything answered.
///
/// No metric at all: the bound this fixture exercises is on [`crate::wire::catalog::ModelContent`]
/// alone, and a metric would only add bytes nothing here reads.
pub(crate) fn bundle_with_many_models(count: usize) -> PinnedDefinitions {
    // Just under the per-field cap, so each model's own JSON is a few kilobytes - few enough models
    // are needed to cross 64 KiB that the fixture stays cheap to build and to serialize.
    let long = description(&"x".repeat(4000));
    let models = (0..count)
        .map(|index| {
            let name = format!("model_{index:03}");
            Model::new(
                ModelName::parse(&name).expect("a generated test model name is a name"),
                source(),
                TableName::parse(&name).expect("a generated test table name is a name"),
                BTreeSet::from([column("amount_cents")]),
                long.clone(),
            )
        })
        .collect();
    let definitions = Definitions::assemble(models, vec![], vec![]).expect("the generated test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions.clone(),
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(MetadataCapabilities::produced(&definitions, &Knowledge::none())),
        ),
    )
    .expect("the test definitions hash")
}

/// [`bundle`], plus a second metric restricted to the `finance` audience - `docs/adr/0028`.
///
/// One bundle, two callers, two different catalogs: `revenue` stays open so every caller can still
/// ask the ordinary question, and `finance_only` is what tells the two callers apart through
/// `ScopedView`'s per-identity filter. The mirror of `sutura_http`'s identity e2e fixture, so the two
/// transports prove the same property with the same shape.
pub(crate) fn bundle_with_a_restricted_metric() -> PinnedDefinitions {
    let column = |raw: &str| ColumnName::parse(raw).expect("a test column is a column");
    let model = Model::new(
        ModelName::parse("orders").expect("a test model is a model"),
        source(),
        TableName::parse("orders").expect("a test table is a table"),
        BTreeSet::from([column("amount_cents"), column("order_date"), column("region")]),
        description("Orders, one row per order."),
    );
    let revenue = Metric::new(
        MetricName::parse("revenue").expect("a test metric is a metric"),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Day, Grain::Month]),
        Vec::new(),
        Some(Anchor::new(
            june(),
            AnchorValue::parse(ANCHORED_VALUE.to_string()).expect("a test anchor value is a value"),
        )),
        description("Revenue, in minor units."),
        Audience::Open,
    )
    .expect("no dimensions to duplicate");
    let restricted_to_finance = Audience::Restricted(
        sutura_domain::catalog::AudienceGrant::parse(BTreeSet::from([
            AudienceId::parse("finance").expect("a test audience id is one")
        ]))
        .expect("one id grants"),
    );
    let finance_only = Metric::new(
        MetricName::parse("finance_only").expect("a test metric is a metric"),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        None,
        description("Only a finance-granted caller may see this."),
        restricted_to_finance,
    )
    .expect("no dimensions to duplicate");
    let definitions = sutura_domain::catalog::Definitions::assemble(vec![model], vec![], vec![revenue, finance_only])
        .expect("the test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions.clone(),
        Knowledge::none(),
        ContributionManifest::single(
            source(),
            Contribution::of(MetadataCapabilities::produced(&definitions, &Knowledge::none())),
        ),
    )
    .expect("the test definitions hash")
}
