//! The system prompt's metric vocabulary is held to [`crate::catalog_budget::MAX_CATALOG_LISTING_BYTES`]
//! the same way `sutura-mcp`'s and `sutura-http`'s own listings are - `render` spends the same shared
//! budget, it just has nobody else's test watching it do so.

use std::collections::BTreeSet;

use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Definitions, Description, Metric, Model};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, ModelName, SourceName, TableName};
use sutura_domain::pinned::view::ScopedView;
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

use crate::catalog_budget::MAX_CATALOG_LISTING_BYTES;

use super::{CatalogProse, PromptInputs, Tool, render};

/// `count` metrics over one model, each described at [`sutura_domain::catalog::MAX_DESCRIPTION_BYTES`],
/// just under the per-field cap, so a handful of them already dwarfs the shared prompt budget
/// without the fixture needing to build an unreasonable number of metrics.
fn bundle_with_many_metrics(count: usize) -> PinnedDefinitions {
    let amount = ColumnName::parse("amount_cents").expect("a test column is a column");
    let when = ColumnName::parse("order_date").expect("a test column is a column");
    let model = Model::new(
        ModelName::parse("orders").expect("a test model is a model"),
        SourceName::parse("local").expect("a test source is a source"),
        TableName::parse("orders").expect("a test table is a table"),
        BTreeSet::from([amount.clone(), when.clone()]),
        Description::parse("Orders, one row per order.").expect("a test description is a description"),
    );
    let long = Description::parse("x".repeat(sutura_domain::catalog::MAX_DESCRIPTION_BYTES))
        .expect("the description cap itself is a description");
    let metrics = (0..count)
        .map(|index| {
            Metric::new(
                MetricName::parse(format!("metric_{index:03}")).expect("a generated test metric name is a name"),
                ModelName::parse("orders").expect("a test model is a model"),
                Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, amount.clone()))),
                Vec::new(),
                when.clone(),
                BTreeSet::from([Grain::Day]),
                Vec::new(),
                None,
                long.clone(),
                sutura_domain::catalog::Audience::Open,
            )
            .expect("no dimensions to duplicate")
        })
        .collect();
    let definitions = Definitions::assemble(vec![model], vec![], metrics).expect("the generated test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions.clone(),
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(MetadataCapabilities::produced(&definitions, &Knowledge::none())),
        ),
    )
    .expect("the generated test definitions hash")
}

fn prompt(bundle: &PinnedDefinitions) -> String {
    render(
        &ScopedView::everything(bundle),
        &PromptInputs::new(Tool::ALL, CatalogProse::Quoted, None),
    )
}

/// Few enough metrics that the whole vocabulary fits under the shared budget: every metric's own
/// heading reaches the prompt, and the listing carries no omission note - the ordinary case, not a
/// special one.
#[test]
fn an_under_bound_system_prompt_carries_every_metric_with_nothing_omitted() {
    let bundle = bundle_with_many_metrics(2);
    let text = prompt(&bundle);
    for index in 0..2 {
        assert!(text.contains(&format!("### metric_{index:03}")), "{text}");
    }
    assert!(!text.contains("Left out of this listing"), "{text}");
}

/// Enough metrics, each near the per-field description cap, that the vocabulary crosses
/// [`MAX_CATALOG_LISTING_BYTES`]: the prompt still carries whole metric sections up to the bound,
/// never a metric cut mid-way, and names the rest in a machine-readable-enough omission note that
/// points back at `describe_catalog`'s own `metric`/`model` arguments.
#[test]
fn an_over_bound_system_prompt_carries_whole_metrics_and_names_the_rest() {
    // Each metric's rendered section is at least its own description (4 KiB), so forty of them is
    // comfortably past 128 KiB even before the grain/dimension lines each section also carries.
    const METRIC_COUNT: usize = 40;
    let bundle = bundle_with_many_metrics(METRIC_COUNT);
    let text = prompt(&bundle);

    let included: Vec<usize> = (0..METRIC_COUNT)
        .filter(|index| text.contains(&format!("### metric_{index:03}")))
        .collect();
    let omitted: Vec<usize> = (0..METRIC_COUNT).filter(|index| !included.contains(index)).collect();

    assert!(!included.is_empty(), "{text}");
    assert!(!omitted.is_empty(), "every metric fit - make the fixture bigger: {text}");
    assert_eq!(included.len() + omitted.len(), METRIC_COUNT);

    assert!(text.contains("Left out of this listing"), "{text}");
    assert!(text.contains(&MAX_CATALOG_LISTING_BYTES.to_string()), "{text}");
    assert!(text.contains("describe_catalog"), "{text}");
    // Every omitted metric is named, with its model, so a caller can still read it whole.
    for index in &omitted {
        assert!(
            text.contains(&format!("metric_{index:03} (model orders)")),
            "metric_{index:03} is omitted but not named: {text}"
        );
    }
}
