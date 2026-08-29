//! What `Warehouse::verify_anchor` can and cannot be handed.
//!
//! The method takes no credential, so what bounds it is its INPUT. Each case below is a shape a
//! caller's question reaches and an anchor's never does, and the last one is the shape the boot path
//! actually builds - so the refusals are not passing against a constructor that refuses everything.

use super::{
    AnchorPlan, NotAnAnchorsPlan, PlanBucket, PlanColumn, PlanFilter, PlanKey, PlanMeasure, PlanPredicate, PlanTerm,
    PredicateOrigin, QueryPlan,
};
use crate::calendar::{Date, TimeRange};
use crate::catalog::Anchor;
use crate::model::{Aggregate, ColumnName, Grain, MetricName, SourceName, TableName};

fn metric() -> MetricName {
    MetricName::parse("mrr").expect("a test metric is a metric")
}

fn table() -> TableName {
    TableName::parse("fct_subscription_monthly").expect("a test table is a table")
}

fn column(name: &str) -> PlanColumn {
    PlanColumn::new(table(), ColumnName::parse(name).expect("a test column is a column"))
}

fn day(iso: &str) -> Date {
    Date::parse(iso).expect("a test date is a date")
}

fn certified() -> TimeRange {
    TimeRange::new(day("2026-06-01"), day("2026-07-01")).expect("a test range is a range")
}

fn anchor() -> Anchor {
    Anchor::new(certified(), String::from("197122"))
}

/// The plan the boot path builds for an anchor: one metric, its coarsest grain, its certified range,
/// no keys and no requested predicate.
fn anchors_plan(range: TimeRange, keys: Vec<PlanKey>, filters: Vec<PlanFilter>) -> QueryPlan {
    QueryPlan::new(
        SourceName::parse("local").expect("a test source is a source"),
        metric(),
        table(),
        Vec::new(),
        PlanBucket::new(String::from("period"), Grain::Month, column("month")),
        keys,
        PlanMeasure::Simple {
            term: PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: column("mrr_amount"),
            },
        },
        String::from("mrr"),
        filters,
        Vec::new(),
        range,
    )
}

#[test]
fn the_plan_the_boot_path_builds_is_an_anchors_plan() {
    // The positive case first, because every refusal below is worthless without it: a constructor
    // that refused everything would satisfy all four assertions and check nothing.
    let plan = anchors_plan(certified(), Vec::new(), Vec::new());
    let parsed = AnchorPlan::of(&plan, &metric(), &anchor()).expect("the anchor's own plan is one");
    assert_eq!(parsed.plan(), &plan);

    // And a definitional predicate does not disqualify it: a required filter is part of what the
    // metric MEANS and is in every plan for it, the anchor's included.
    let definitional = anchors_plan(
        certified(),
        Vec::new(),
        vec![PlanFilter::new(
            PredicateOrigin::Definition,
            PlanPredicate::Equals {
                column: column("status"),
                param: 0,
            },
        )],
    );
    assert!(
        AnchorPlan::of(&definitional, &metric(), &anchor()).is_ok(),
        "a metric's own predicate is not a question's"
    );
}

#[test]
fn a_question_shaped_plan_cannot_reach_the_method_that_needs_no_credential() {
    // THE POINT OF THE TYPE. `verify_anchor` executes under whatever identity the deployment
    // configured the adapter with, and while it took a bare `QueryPlan` the only thing keeping a
    // caller's question off it was where the call sites happen to be. These four are the shapes a
    // question reaches and an anchor's plan does not.
    let grouped = anchors_plan(
        certified(),
        vec![PlanKey::new(String::from("region"), column("region"))],
        Vec::new(),
    );
    assert_eq!(
        AnchorPlan::of(&grouped, &metric(), &anchor()).unwrap_err(),
        NotAnAnchorsPlan::Grouped { keys: 1 },
        "an anchor is a metric's own number and not a slice of it"
    );

    let filtered = anchors_plan(
        certified(),
        Vec::new(),
        vec![PlanFilter::new(
            PredicateOrigin::Requested,
            PlanPredicate::Equals {
                column: column("region"),
                param: 0,
            },
        )],
    );
    assert_eq!(
        AnchorPlan::of(&filtered, &metric(), &anchor()).unwrap_err(),
        NotAnAnchorsPlan::Requested,
        "a predicate a question asked for is the mark of a question"
    );

    let elsewhere = TimeRange::new(day("2026-01-01"), day("2026-02-01")).expect("a test range is a range");
    let moved = anchors_plan(elsewhere, Vec::new(), Vec::new());
    assert_eq!(
        AnchorPlan::of(&moved, &metric(), &anchor()).unwrap_err(),
        NotAnAnchorsPlan::NotTheAnchorsRange {
            plan: elsewhere,
            anchor: certified(),
        },
        "the range an author certified is the only one this door opens for"
    );

    let other = MetricName::parse("active_subscriptions").expect("a test metric is a metric");
    assert_eq!(
        AnchorPlan::of(&anchors_plan(certified(), Vec::new(), Vec::new()), &other, &anchor()).unwrap_err(),
        NotAnAnchorsPlan::NotThatMetric {
            plan: metric(),
            anchor: other,
        },
        "a plan checked against another metric's anchor is not that anchor's plan"
    );
}
