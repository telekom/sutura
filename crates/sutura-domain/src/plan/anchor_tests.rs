//! What `AnchorPlan::of` accepts, and what it now reads off the bundle instead of taking on trust.
//!
//! **These cases are a self-check on the boot path, and none of them may be read as authority.** The
//! type's own documentation carries that distinction at length; the short form is that every value
//! below is publicly constructible, so what the constructor can catch is a boot path that compiled
//! the wrong question - not code that wants to execute under the deployment's identity. What keeps
//! `Warehouse::verify_anchor` to one call site is the `clippy.toml` ban on it.
//!
//! The previous version of this file passed a caller-supplied `(metric, anchor)` pair, and a review
//! defeated the whole type by fabricating one: an ordinary question's plan, an `Anchor::new` with a
//! matching range and a value nobody certified, and all four guards passed. The pair is gone. The
//! bundle is the argument now, and the last two cases are the two holes that closed with it.

use std::collections::BTreeSet;

use super::{
    AnchorPlan, NotAnAnchorsPlan, PlanBucket, PlanColumn, PlanFilter, PlanKey, PlanMeasure, PlanPredicate, PlanTerm,
    PredicateOrigin, QueryPlan, StatementTables,
};
use crate::calendar::{Date, TimeRange};
use crate::catalog::{Anchor, Definitions, Description, Metric, Model};
use crate::knowledge::Knowledge;
use crate::measure::{AggregatedColumn, Measure, Term};
use crate::model::{Aggregate, ColumnName, Grain, MetricName, ModelName, SourceName, TableName};
use crate::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

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

/// The bundle the checks are made against: one model, one metric at `Month` grain, anchored over
/// [`certified`] unless `anchor` says otherwise.
///
/// A real [`PinnedDefinitions`], hashed for real, because that is what the constructor now reads. The
/// two parameters are what the last cases vary: whether the metric declares an anchor at all, and
/// which grains it declares.
fn bundle(anchor: Option<Anchor>, grains: BTreeSet<Grain>) -> PinnedDefinitions {
    let name = |raw: &str| ColumnName::parse(raw).expect("a test column is a column");
    let model_name = ModelName::parse("subscriptions").expect("a test model is a model");
    let model = Model::new(
        model_name.clone(),
        SourceName::parse("local").expect("a test source is a source"),
        table(),
        BTreeSet::from([name("mrr_amount"), name("month"), name("status")]),
        Description::default(),
    );
    let definition = Metric::new(
        metric(),
        model_name,
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, name("mrr_amount")))),
        Vec::new(),
        name("month"),
        grains,
        Vec::new(),
        anchor,
        Description::default(),
    )
    .expect("no dimensions to duplicate");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        Definitions::assemble(vec![model], vec![], vec![definition]).expect("the test bundle is consistent"),
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(crate::capabilities::MetadataCapabilities::nothing()),
        ),
    )
    .expect("the test definitions hash")
}

/// The bundle every case but the last two uses: anchored over the certified range, `Month` only.
fn anchored() -> PinnedDefinitions {
    bundle(
        Some(Anchor::new(certified(), String::from("197122"))),
        BTreeSet::from([Grain::Month]),
    )
}

/// The plan the boot path builds for an anchor: one metric, its coarsest grain, its certified range,
/// no keys and no requested predicate.
fn anchors_plan(range: TimeRange, grain: Grain, keys: Vec<PlanKey>, filters: Vec<PlanFilter>) -> QueryPlan {
    QueryPlan::new(
        SourceName::parse("local").expect("a test source is a source"),
        metric(),
        StatementTables::only(table()),
        PlanBucket::new(String::from("period"), grain, column("month")),
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
    // that refused everything would satisfy all of them and check nothing.
    let pinned = anchored();
    let plan = anchors_plan(certified(), Grain::Month, Vec::new(), Vec::new());
    let parsed = AnchorPlan::of(&plan, &pinned, &metric()).expect("the anchor's own plan is one");
    assert_eq!(parsed.plan(), &plan);

    // And a definitional predicate does not disqualify it: a required filter is part of what the
    // metric MEANS and is in every plan for it, the anchor's included.
    let definitional = anchors_plan(
        certified(),
        Grain::Month,
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
        AnchorPlan::of(&definitional, &pinned, &metric()).is_ok(),
        "a metric's own predicate is not a question's"
    );
}

#[test]
fn a_question_shaped_plan_is_not_an_anchors_plan() {
    // The shapes a question reaches and an anchor's plan does not. Not a claim that a caller cannot
    // reach the method - see this file's own preamble - but the check that catches a boot path which
    // compiled a question where an anchor was meant.
    let pinned = anchored();
    let grouped = anchors_plan(
        certified(),
        Grain::Month,
        vec![PlanKey::new(String::from("region"), column("region"))],
        Vec::new(),
    );
    assert_eq!(
        AnchorPlan::of(&grouped, &pinned, &metric()).unwrap_err(),
        NotAnAnchorsPlan::Grouped { keys: 1 },
        "an anchor is a metric's own number and not a slice of it"
    );

    let filtered = anchors_plan(
        certified(),
        Grain::Month,
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
        AnchorPlan::of(&filtered, &pinned, &metric()).unwrap_err(),
        NotAnAnchorsPlan::Requested,
        "a predicate a question asked for is the mark of a question"
    );

    let other = MetricName::parse("active_subscriptions").expect("a test metric is a metric");
    let plan = anchors_plan(certified(), Grain::Month, Vec::new(), Vec::new());
    assert_eq!(
        AnchorPlan::of(&plan, &pinned, &other).unwrap_err(),
        NotAnAnchorsPlan::NotThatMetric {
            plan: metric(),
            anchor: other,
        },
        "a plan checked against another metric's anchor is not that anchor's plan"
    );
}

#[test]
fn the_range_compared_against_is_the_bundles_and_not_the_callers() {
    // **The forgery a review used, and the reason the signature changed.** The pair the constructor
    // took was `(&MetricName, &Anchor)`, both publicly constructible, so an ordinary question's plan
    // plus an `Anchor::new` carrying that plan's own range and a value nobody certified passed every
    // guard. There is no anchor argument now: the range comes off the bundle, so a plan over a range
    // the bundle's own author did not certify is refused whatever a caller holds.
    let pinned = anchored();
    let elsewhere = TimeRange::new(day("2026-01-01"), day("2026-02-01")).expect("a test range is a range");
    let moved = anchors_plan(elsewhere, Grain::Month, Vec::new(), Vec::new());
    assert_eq!(
        AnchorPlan::of(&moved, &pinned, &metric()).unwrap_err(),
        NotAnAnchorsPlan::NotTheAnchorsRange {
            plan: elsewhere,
            anchor: certified(),
        },
        "the range the bundle's author certified is the only one this door opens for"
    );
}

#[test]
fn a_plan_at_a_finer_grain_than_the_metric_declares_is_not_an_anchors_plan() {
    // **The fifth gap, and it was open.** Nothing read the grain, so a `Day`-grain plan over the
    // anchor's range passed all four guards - and what came back was a row per day rather than the
    // one number the anchor certifies. Downstream insists on exactly one row, so the series arrived
    // as a mismatch that reads like a broken definition; and a series is strictly more than the
    // number the bundle already publishes in its own catalog document.
    let pinned = bundle(
        Some(Anchor::new(certified(), String::from("197122"))),
        BTreeSet::from([Grain::Day, Grain::Month]),
    );
    let daily = anchors_plan(certified(), Grain::Day, Vec::new(), Vec::new());
    assert_eq!(
        AnchorPlan::of(&daily, &pinned, &metric()).unwrap_err(),
        NotAnAnchorsPlan::NotTheCoarsestGrain {
            metric: metric(),
            plan: Grain::Day,
            coarsest: Some(Grain::Month),
        },
        "an anchor is asked at the coarsest grain its metric declares, so that one period is one row"
    );
}

#[test]
fn a_metric_the_bundle_does_not_anchor_has_no_anchors_plan() {
    // Two ways there is nothing to be the plan OF, and each is its own variant because they send a
    // reader somewhere different: a bundle that never defined the metric is a boot path asking about
    // something else, and a metric with no anchor is a catalog document that declares no number.
    let unanchored = bundle(None, BTreeSet::from([Grain::Month]));
    let plan = anchors_plan(certified(), Grain::Month, Vec::new(), Vec::new());
    assert_eq!(
        AnchorPlan::of(&plan, &unanchored, &metric()).unwrap_err(),
        NotAnAnchorsPlan::DeclaresNoAnchor { metric: metric() },
        "a metric with no certified number has no anchor for a plan to be of"
    );

    // The second absence. The plan has to name the missing metric too, or the first check catches it
    // as `NotThatMetric` and this variant is never reached - which is why the plan is rebuilt here
    // rather than reused.
    let absent = MetricName::parse("gross_margin").expect("a test metric is a metric");
    let for_absent = QueryPlan::new(
        SourceName::parse("local").expect("a test source is a source"),
        absent.clone(),
        StatementTables::only(table()),
        PlanBucket::new(String::from("period"), Grain::Month, column("month")),
        Vec::new(),
        PlanMeasure::Simple {
            term: PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: column("mrr_amount"),
            },
        },
        String::from("gross_margin"),
        Vec::new(),
        Vec::new(),
        certified(),
    );
    assert_eq!(
        AnchorPlan::of(&for_absent, &unanchored, &absent).unwrap_err(),
        NotAnAnchorsPlan::MetricNotDefined { metric: absent },
        "a bundle that does not define the metric cannot have anchored it"
    );
}

#[test]
fn the_grain_comparison_names_the_absence_rather_than_carrying_a_variant_nothing_can_provoke() {
    // The empty-set case is folded into `NotTheCoarsestGrain` as an `Option`, and there is no variant
    // of its own for it, because `Definitions::assemble` refuses a metric that declares no grain -
    // `NoGrains`, asserted here so that this test is the record of WHY the fold happened rather than
    // a sentence in a doc comment. A variant no test can provoke is one this crate does not carry.
    let column = |raw: &str| ColumnName::parse(raw).expect("a test column is a column");
    let model_name = ModelName::parse("subscriptions").expect("a test model is a model");
    let model = Model::new(
        model_name.clone(),
        SourceName::parse("local").expect("a test source is a source"),
        table(),
        BTreeSet::from([column("mrr_amount"), column("month")]),
        Description::default(),
    );
    let grainless = Metric::new(
        metric(),
        model_name,
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("mrr_amount")))),
        Vec::new(),
        column("month"),
        BTreeSet::new(),
        Vec::new(),
        Some(Anchor::new(certified(), String::from("1"))),
        Description::default(),
    )
    .expect("no dimensions to duplicate");
    assert!(
        Definitions::assemble(vec![model], vec![], vec![grainless]).is_err(),
        "a metric declaring no grain never reaches a pinned bundle, so the absence needs no variant"
    );

    // And the sentence the `Option` renders still reads, which is the other half of folding it in.
    let unreachable = NotAnAnchorsPlan::NotTheCoarsestGrain {
        metric: metric(),
        plan: Grain::Day,
        coarsest: None,
    };
    assert_eq!(
        unreachable.to_string(),
        "an anchor of `mrr` is asked at no grain it declares and this plan buckets at day"
    );
}
