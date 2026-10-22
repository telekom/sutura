//! **A question over several metrics, executed**: an arm per metric, each answered on its own and
//! joined above (`telekom/sutura#1471`).
//!
//! **What this does NOT establish.** Every leg runs under one operating-system identity, as in the
//! rest of `federated.rs`, and the bucket of two arms is compared by label and grain here: whether
//! two arms' buckets hold one kind of value is the combiner's run-time check, not a cell of this file.

use std::collections::BTreeMap;

use sutura_domain::pinned::view::ScopedView;
use sutura_domain::plan::RowCeiling;
use sutura_domain::query::{Query, RefusalReason, ToolOutcome};
use sutura_domain::warehouse::agreement::{RealTolerance, agree_on_content, agree_on_order};
use sutura_domain::warehouse::{RowSet, Value};
use sutura_semantic::{Compiled, compile};

use super::corpus::{derived, remote_products};
use super::harness::{Side, answered, bundle, one_source, two_engines};

const BY_REGION: &str = "metrics: [recurring_revenue, active_subscriptions]\ngrain: month\nrange:\n  start: 2026-06-01\n  end: 2026-07-01\ndimensions: [region]\n";
const BY_FAMILY: &str = "metrics: [recurring_revenue, voice_minutes]\ngrain: month\nrange:\n  start: 2026-06-01\n  end: 2026-07-01\ndimensions: [product_family]\n";

fn question(text: &str) -> Query {
    serde_norway::from_str(text).expect("a fixture question is a question")
}

fn rows<W>(side: &Side<W>, text: &str, name: &str) -> RowSet
where
    W: sutura_domain::warehouse::Warehouse + Sync,
    W::Error: Send,
{
    let combiner = sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds");
    match answered(side, &question(text), name, &combiner).unwrap_or_else(|e| panic!("{e}")) {
        ToolOutcome::Answer { rows, .. } => rows,
        refused @ ToolOutcome::Refusal { .. } => panic!("{name}: the question is answered, not {refused:?}"),
    }
}

/// A catalog's compiled answer to `text`.
fn compiled(catalog: &std::path::Path, text: &str) -> Compiled {
    compile(
        &question(text),
        &ScopedView::everything(&bundle(catalog)),
        RowCeiling::DEFAULT,
    )
    .expect("the derived catalog compiles the question")
}

/// Two metrics by a dimension on a second data system: an arm each, joined to the lookup, and the
/// two-source answer is the one-source answer.
#[test]
fn two_metrics_by_a_remote_dimension_are_the_one_source_answer() {
    let derived = derived();
    assert!(
        matches!(compiled(&derived.one_source, BY_REGION), Compiled::Planned { .. }),
        "on one source the two metrics share one statement"
    );
    let Compiled::DrillAcross { plan } = compiled(&derived.two_source, BY_REGION) else {
        panic!("on two sources each metric is an arm of its own");
    };
    assert_eq!(plan.legs().len(), 4, "a fact leg and a lookup leg for each of two metrics");
    let name = "two-metrics-by-region";
    let one = rows(&one_source(bundle(&derived.one_source)), BY_REGION, name);
    let two = rows(&two_engines(bundle(&derived.two_source)), BY_REGION, name);
    if let Err(disagreement) = agree_on_content(&one, &two, RealTolerance::DIFFERENTIAL) {
        panic!("{name}: one source and two sources returned different rows - {disagreement}");
    }
    if let Err(disagreement) = agree_on_order(&one, &two, RealTolerance::DIFFERENTIAL) {
        panic!("{name}: one source and two sources returned the rows in different orders - {disagreement}");
    }
}

/// One answer's measure by its `(family, month)` group, each group beside the cells that name it.
type Measures = BTreeMap<String, (Vec<Value>, Value)>;

/// One answer's measure by its `(family, month)` group, the group kept as the cells that name it.
fn by_group(rows: &RowSet) -> Measures {
    rows.rows()
        .iter()
        .filter_map(|row| {
            let (key, measures) = row.split_at(2);
            Some((format!("{key:?}"), (key.to_vec(), measures.first()?.clone())))
        })
        .collect()
}

/// Metrics on two models that reach one dimension: each measure column is that metric's own answer,
/// and a group one metric lacks is null under it.
#[test]
fn two_models_by_a_shared_dimension_are_each_metrics_own_answer() {
    let side = one_source(bundle(&derived().one_source));
    let name = "two-models-by-product-family";
    let together = rows(&side, BY_FAMILY, name);
    let alone = |metric: &str| {
        by_group(&rows(
            &side,
            &BY_FAMILY.replace("[recurring_revenue, voice_minutes]", &format!("[{metric}]")),
            name,
        ))
    };
    let (revenue, minutes) = (alone("recurring_revenue"), alone("voice_minutes"));
    assert!(!revenue.is_empty() && !minutes.is_empty(), "both metrics answer something");
    let mut groups: BTreeMap<&String, &Vec<Value>> = BTreeMap::new();
    for (group, (key, _)) in revenue.iter().chain(&minutes) {
        groups.entry(group).or_insert(key);
    }
    let measure = |solo: &Measures, group: &String| solo.get(group).map_or(Value::Null, |(_, value)| value.clone());
    let expected = RowSet::new(
        together.columns().to_vec(),
        groups
            .iter()
            .map(|(&group, key)| {
                let mut row = (*key).clone();
                row.extend([measure(&revenue, group), measure(&minutes, group)]);
                row
            })
            .collect(),
    )
    .expect("the expected answer is rectangular");
    if let Err(disagreement) = agree_on_content(&expected, &together, RealTolerance::DIFFERENTIAL) {
        panic!("{name}: a measure is not its metric's own answer - {disagreement}\n{together:?}");
    }
}

/// A dimension that ends on another column for one metric is refused, and the catalog that does not
/// edit it is not.
#[test]
fn a_dimension_ending_on_another_column_is_not_conformed() {
    let derived = remote_products("multi-metric-nonconformed", "source: local");
    assert!(
        matches!(compiled(&derived.one_source, BY_REGION), Compiled::Planned { .. }),
        "the unedited catalog answers the question"
    );
    let metric = derived.one_source.join("metrics/active_subscriptions.md");
    let text = std::fs::read_to_string(&metric).expect("the metric document is there");
    let find = "name: region\n    column: region";
    assert_eq!(text.matches(find).count(), 1, "the region dimension is declared once");
    std::fs::write(&metric, text.replace(find, "name: region\n    column: segment")).expect("the edit is written");
    let Compiled::Refused { reason } = compiled(&derived.one_source, BY_REGION) else {
        panic!("a dimension read off another column for one metric is refused");
    };
    assert!(
        matches!(
            reason,
            RefusalReason::DimensionNotConformed { ref dimension, ref first, ref other }
                if dimension.as_str() == "region"
                    && first.as_str() == "recurring_revenue"
                    && other.as_str() == "active_subscriptions"
        ),
        "{reason:?}"
    );
}
