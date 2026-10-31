//! A question over several metrics: one statement when they all read one data system, each metric
//! its own whole-answer arm.
//!
//! Every resolution is built by hand, as `plan/tests/drill_across.rs` builds them.

use std::collections::BTreeSet;

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::catalog::{Audience, Description, Dimension, JoinKey, JoinKeys, Metric, Model, Relationship, ViaChain};
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{
    Aggregate, ColumnName, DimensionName, Grain, JoinType, MetricName, ModelName, RelationshipName, SourceName, TableName,
};
use sutura_domain::plan::{KeyValue, PlanKey};
use sutura_domain::query::{Top, TopBy, TopDirection, TopN};

use super::plan_resolved;
use crate::resolve::{Resolution, Resolved, ResolvedDimension, ResolvedJoin};

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a test column is a column")
}

fn segment() -> DimensionName {
    DimensionName::parse("segment").expect("a test dimension is a dimension")
}

/// A model whose physical table is `dim_{name}`.
fn model(name: &str, on: &str, columns: &[&str]) -> Model {
    Model::new(
        ModelName::parse(name).expect("a test model is a model"),
        SourceName::parse(on).expect("a test source is a source"),
        TableName::parse(format!("dim_{name}")).expect("a test table is a table"),
        columns.iter().map(|c| column(c)),
        Description::default(),
    )
}

fn metric(name: &str, on: &str, dimension: Dimension) -> Metric {
    Metric::new(
        MetricName::parse(name).expect("a test metric is a metric"),
        ModelName::parse(on).expect("a test model is a model"),
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
}

/// `segment` read from `col` of the metric's own model.
fn own(col: &str) -> Dimension {
    Dimension::new(segment(), column(col), None, None, Description::default())
}

/// `each` alone on `facts`, reaching `segment` through `hop` when there is one.
fn reach<'a>(each: &'a Metric, facts: &'a Model, hop: Option<(&'a Relationship, &'a Model)>) -> Resolution<'a> {
    Resolution {
        metric: each,
        metrics: vec![each],
        model: facts,
        grain: Grain::Month,
        range: TimeRange::new(
            Date::parse("2026-06-01").expect("a test date is a date"),
            Date::parse("2026-07-01").expect("a test date is a date"),
        )
        .expect("June is a range"),
        keys: vec![ResolvedDimension {
            dimension: each.dimension(&segment()).expect("the metric declares this dimension"),
            join: hop.map(|(relationship, model)| vec![ResolvedJoin { relationship, model }]),
        }],
        filters: Vec::new(),
        top: None,
        cross: None,
    }
}

fn read_from(key: Option<&PlanKey>) -> (String, String) {
    let KeyValue::Column(column) = key.expect("the arm carries this key").value() else {
        panic!("a key here reads its column as stored");
    };
    (column.table().to_string(), column.column().to_string())
}

#[test]
fn metrics_on_one_data_system_are_one_stacked_statement_each_reading_its_own_path() {
    let facts = model("facts", "local", &["amount_cents", "segment_key", "segment_name", "day"]);
    let segments = model("segments", "local", &["segment_key", "name"]);
    let link = Relationship::new(
        RelationshipName::parse("facts_segment").expect("a test relationship is a relationship"),
        ModelName::parse("facts").expect("a test model is a model"),
        ModelName::parse("segments").expect("a test model is a model"),
        JoinType::ManyToOne,
        JoinKeys::of(vec![JoinKey::Equal {
            origin: column("segment_key"),
            target: column("segment_key"),
        }])
        .expect("a test relationship declares one key"),
    );
    let via = ViaChain::of(vec![
        RelationshipName::parse("facts_segment").expect("a test relationship is a relationship"),
    ])
    .expect("a test chain has hops");
    let joined = metric(
        "joined",
        "facts",
        Dimension::new(segment(), column("name"), Some(via), None, Description::default()),
    );
    let by_own = metric("own", "facts", own("segment_name"));
    let top = Top::new(
        TopN::parse(3).expect("three is a row count"),
        TopBy::Metric,
        TopDirection::Desc,
    );
    let through = Some((&link, &segments));
    let both = Resolved {
        whole: Resolution {
            metrics: vec![&joined, &by_own],
            ..reach(&joined, &facts, through)
        },
        per_metric: vec![
            Resolution {
                top: Some(top),
                ..reach(&joined, &facts, through)
            },
            reach(&by_own, &facts, None),
        ],
    };
    let planned = match plan_resolved(&both) {
        Some(Ok(planned)) => planned,
        other => panic!("expected one stacked statement, got {:?}", other.map(Result::err)),
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

#[test]
fn metrics_on_two_data_systems_are_not_stacked() {
    let here = model("facts", "local", &["amount_cents", "segment_name", "day"]);
    let there = model("ledger", "remote", &["amount_cents", "segment_name", "day"]);
    let first = metric("first", "facts", own("segment_name"));
    let second = metric("second", "ledger", own("segment_name"));
    let both = Resolved {
        whole: Resolution {
            metrics: vec![&first, &second],
            ..reach(&first, &here, None)
        },
        per_metric: vec![reach(&first, &here, None), reach(&second, &there, None)],
    };
    assert!(plan_resolved(&both).is_none(), "metrics on two data systems are an arm each");
}
