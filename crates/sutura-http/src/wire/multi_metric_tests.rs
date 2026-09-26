//! The per-metric digest test, split out of `wire.rs`'s own `mod tests` for `cargo xtask
//! max-lines`'s per-file cap - a sibling module of `wire::tests`, not a child, so it reads
//! `super::` for the same private items that module does.

use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Audience, Definitions, Description, Metric, Model};
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, ModelName, SourceName, TableName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};
use sutura_domain::query::ToolOutcome;
use sutura_domain::source::{AcknowledgementReason, ExecutedAs, SharedIdentityDeclared, SourcePosture};

use super::Outcome;

/// A two-metric bundle over one `orders` model: a `Sum` and a `CountDistinct`, both undimensioned
/// and unanchored - the smallest shape `PinnedDefinitions::provenance_for` needs two of.
fn two_metric_bundle() -> PinnedDefinitions {
    let col = |raw: &str| ColumnName::parse(raw).expect("a test column is a column");
    let name = |raw: &str| MetricName::parse(raw).expect("a test metric is a metric");
    let one = |name: MetricName, aggregate, column: &str| {
        Metric::new(
            name,
            ModelName::parse("orders").expect("a test model is a model"),
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(aggregate, col(column)))),
            Vec::new(),
            col("order_date"),
            std::collections::BTreeSet::from([Grain::Month]),
            Vec::new(),
            None,
            Description::default(),
            Audience::Open,
        )
        .expect("no dimensions to duplicate")
    };
    let model = Model::new(
        ModelName::parse("orders").expect("a test model is a model"),
        SourceName::parse("local").expect("a test source is a source"),
        TableName::parse("orders").expect("a test table is a table"),
        std::collections::BTreeSet::from([col("amount_cents"), col("customer_key"), col("order_date")]),
        Description::default(),
    );
    let metrics = vec![
        one(name("revenue"), Aggregate::Sum, "amount_cents"),
        one(name("customers"), Aggregate::CountDistinct, "customer_key"),
    ];
    let definitions = Definitions::assemble(vec![model], vec![], metrics).expect("the test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions,
        sutura_domain::knowledge::Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(MetadataCapabilities::nothing()),
        ),
    )
    .expect("the test definitions hash")
}

/// A multi-metric answer's per-metric digests render in the body, in request order, and they
/// are two DIFFERENT digests - not the bundle's own repeated.
#[test]
fn a_multi_metric_answer_carries_one_digest_per_metric() {
    let asked = [
        MetricName::parse("customers").expect("a test metric is a metric"),
        MetricName::parse("revenue").expect("a test metric is a metric"),
    ];
    let executed_as = ExecutedAs::of(
        SourceName::parse("local").expect("a test source is a source"),
        SourcePosture::SharedServiceUser {
            declared: SharedIdentityDeclared::of(
                AcknowledgementReason::parse("a test source read in this process").expect("a test reason is a reason"),
            ),
        },
    );
    let provenance = two_metric_bundle()
        .provenance_for(executed_as, asked.iter())
        .expect("both metrics are defined");

    let rows = sutura_domain::warehouse::RowSet::new(
        vec![String::from("customers")],
        vec![vec![sutura_domain::warehouse::Value::Integer(3)]],
    )
    .expect("one column and one cell");
    let outcome = Outcome::from(&ToolOutcome::Answer { provenance, rows });
    let body = serde_json::to_value(outcome.body()).expect("the outcome serializes");
    let entries = body["provenance"]["metric_digests"]
        .as_array()
        .expect("metric_digests is an array");
    assert_eq!(
        entries.iter().map(|entry| entry["metric"].as_str()).collect::<Vec<_>>(),
        vec![Some("customers"), Some("revenue")],
        "{body}"
    );
    assert_ne!(
        entries[0]["digest"].as_str(),
        entries[1]["digest"].as_str(),
        "two different metrics must not hash the same"
    );
}
