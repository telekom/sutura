//! A question over several metrics that all read one data system: one statement on it, each metric
//! its own whole-answer arm.

use sutura_domain::query::{Top, TopBy, TopDirection, TopN};

use super::drill_across::june;
use super::*;
use crate::resolve::Resolved;

/// `each` alone, reaching `segment` through the one hop of `placed` or on its own model.
fn reach<'a>(each: &'a Metric, through: bool, placed: (&'a Model, &'a Model, &'a Relationship)) -> Resolution<'a> {
    let (facts, segments, link) = placed;
    Resolution {
        metric: each,
        metrics: vec![each],
        model: facts,
        grain: Grain::Month,
        range: june(),
        keys: vec![ResolvedDimension {
            dimension: each
                .dimension(&dimension_name("segment"))
                .expect("the metric declares this dimension"),
            join: through.then(|| {
                vec![ResolvedJoin {
                    relationship: link,
                    model: segments,
                }]
            }),
        }],
        filters: Vec::new(),
        top: None,
        cross: None,
    }
}

#[test]
fn metrics_on_one_data_system_are_one_stacked_statement_each_reading_its_own_path() {
    let facts = model("facts", "local", &["amount_cents", "segment_key", "segment_name", "day"]);
    let segments = model("segments", "local", &["segment_key", "name"]);
    let link = relationship("facts_segment", ("facts", "segment_key"), ("segments", "segment_key"));
    let metric = |name: &str, dimension: Dimension| {
        Metric::new(
            MetricName::parse(name).expect("a test metric is a metric"),
            ModelName::parse("facts").expect("a test model is a model"),
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
            Vec::new(),
            column("day"),
            BTreeSet::from([Grain::Month]),
            vec![dimension],
            None,
            Description::default(),
            Audience::Open,
        )
        .expect("no dimensions to duplicate")
    };
    let joined = metric("joined", declared("segment", "name", &["facts_segment"]));
    let own = metric(
        "own",
        Dimension::new(
            dimension_name("segment"),
            column("segment_name"),
            None,
            None,
            Description::default(),
        ),
    );
    let placed = (&facts, &segments, &link);
    let top = Top::new(
        TopN::parse(3).expect("three is a row count"),
        TopBy::Metric,
        TopDirection::Desc,
    );
    let topped = Resolution {
        top: Some(top),
        ..reach(&joined, true, placed)
    };
    let both = Resolved {
        whole: Resolution {
            metrics: vec![&joined, &own],
            ..reach(&joined, true, placed)
        },
        per_metric: vec![topped, reach(&own, false, placed)],
    };
    let planned = match crate::plan::plan_resolved(&both, PlanCeilings::DEFAULT) {
        Ok(Plan::Stacked(planned)) => planned,
        other => panic!("expected one stacked statement, got {:?}", other.err()),
    };
    assert_eq!(planned.top(), Some(top), "the top is the whole answer's, not an arm's");
    let read: Vec<(String, String)> = planned.arms().iter().map(|arm| read_from(arm.keys().first())).collect();
    assert_eq!(
        read,
        [
            (String::from("dim_segments"), String::from("name")),
            (String::from("dim_facts"), String::from("segment_name")),
        ],
        "each arm reads its own metric's column, not the first metric's"
    );
}
