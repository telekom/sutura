//! A question over several metrics: when it stays one statement, and when it is an arm per metric.
//!
//! Every resolution is built by hand, because `resolve` is what makes one per metric and no pinned
//! catalog is needed to read what the planner does with them.

use sutura_domain::plan::{Arm, DrillAcrossPlan};

use super::*;
use crate::resolve::{CrossModel, Resolved};

/// [`crate::plan::plan_resolved`] under the default ceilings.
fn plan_resolved(resolved: &Resolved<'_>) -> Result<Plan, PlanError> {
    crate::plan::plan_resolved(resolved, PlanCeilings::DEFAULT)
}

fn june() -> TimeRange {
    TimeRange::new(
        Date::parse("2026-06-01").expect("a test date is a date"),
        Date::parse("2026-07-01").expect("a test date is a date"),
    )
    .expect("June is a range")
}

fn simple(name: &str) -> Metric {
    TwoSources::metric(
        name,
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
    )
}

/// `metrics` asked together, and each alone, over `corpus`.
fn resolved<'a>(corpus: &'a TwoSources, metrics: &[&'a Metric], by_region: bool) -> Resolved<'a> {
    Resolved {
        whole: corpus.asking(metrics.to_vec(), by_region),
        per_metric: metrics.iter().map(|&each| corpus.asking(vec![each], by_region)).collect(),
    }
}

fn across(answer: Result<Plan, PlanError>) -> DrillAcrossPlan {
    match answer {
        Ok(Plan::DrillAcross(planned)) => *planned,
        other => panic!("expected an arm per metric, got {:?}", other.err()),
    }
}

#[test]
fn metrics_reaching_a_remote_dimension_are_one_joined_arm_each() {
    let corpus = TwoSources::new();
    let (revenue, subs) = (simple("revenue"), simple("subs"));
    let planned = across(plan_resolved(&resolved(&corpus, &[&revenue, &subs], true)));
    assert!(planned.arms().iter().all(|arm| matches!(arm, Arm::Joined(_))));
    assert_eq!(
        planned.arms().iter().map(Arm::measure_label).collect::<Vec<_>>(),
        ["revenue", "subs"]
    );
    assert_eq!(planned.keys(), ["region"]);
    assert_eq!(
        planned.sources().map(SourceName::as_str).collect::<Vec<_>>(),
        ["local", "remote", "local", "remote"]
    );
}

/// `revenue` beside a cross-model ratio over `visits`, both by `region` on `customers`: the ratio is
/// an arm with a second fact leg beside the plain metric's two. The same pair with no dimension is
/// refused by name, not planned as one statement.
#[test]
fn a_cross_model_ratio_beside_another_metric_is_an_arm_with_its_second_fact() {
    let corpus = TwoSources::new();
    let visits = model("visits", "local", &["visit_id", "customer_key", "day_key"]);
    let calendar = model("calendar", "local", &["day_key", "day"]);
    let facts_calendar = relationship("facts_calendar", ("facts", "day_key"), ("calendar", "day_key"));
    let visits_calendar = relationship("visits_calendar", ("visits", "day_key"), ("calendar", "day_key"));
    let visits_customer = relationship("visits_customer", ("visits", "customer_key"), ("customers", "customer_key"));
    let revenue = simple("revenue");
    let ratio = TwoSources::metric(
        "revenue_per_visit",
        Measure::Ratio {
            numerator: Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents"))),
            denominator: Term::Aggregate(AggregatedColumn::on_model(
                Aggregate::Count,
                column("visit_id"),
                ModelName::parse("visits").expect("a test model is a model"),
            )),
            zero_denominator: sutura_domain::measure::ZeroDenominator::Null,
        },
    )
    .with_shared_calendar(ModelName::parse("calendar").expect("a test model is a model"));
    let resolved = |by_region: bool| {
        let ratio_alone = Resolution {
            cross: Some(CrossModel {
                second: &visits,
                calendar: &calendar,
                own_to_calendar: &facts_calendar,
                second_to_calendar: &visits_calendar,
                second_relationships: vec![&visits_customer],
            }),
            ..corpus.asking(vec![&ratio], by_region)
        };
        Resolved {
            whole: corpus.asking(vec![&revenue, &ratio], by_region),
            per_metric: vec![corpus.asking(vec![&revenue], by_region), ratio_alone],
        }
    };
    let planned = across(plan_resolved(&resolved(true)));
    let seconds: Vec<bool> = planned
        .arms()
        .iter()
        .map(|arm| match arm {
            Arm::Joined(joined) => joined.second_fact().is_some(),
            Arm::Local(_) => panic!("both metrics reach a remote dimension"),
        })
        .collect();
    assert_eq!(seconds, [false, true], "only the ratio reads a second fact");
    assert_eq!(planned.legs().len(), 5, "two legs for the sum and three for the ratio");
    // With no remote dimension the ratio has no link to join its two facts on, so it is refused by name.
    let flat = plan_resolved(&resolved(false));
    assert!(
        matches!(
            flat,
            Err(PlanError::Refused(
                RefusalReason::CrossModelRatioWithoutSharedDimension { .. }
            ))
        ),
        "{:?}",
        flat.err()
    );
}

#[test]
fn metrics_that_read_every_dimension_the_same_way_stay_one_statement() {
    let corpus = TwoSources::new();
    let (revenue, subs) = (simple("revenue"), simple("subs"));
    let planned = plan_resolved(&resolved(&corpus, &[&revenue, &subs], false));
    assert!(matches!(planned, Ok(Plan::Mono(_))), "{:?}", planned.err());
}

#[test]
fn a_question_over_more_legs_than_one_answer_may_run_is_refused() {
    let corpus = TwoSources::new();
    let metrics: Vec<Metric> = ["a", "b", "c", "d"].into_iter().map(simple).collect();
    let ask = |count: usize| {
        let named: Vec<&Metric> = metrics.iter().take(count).collect();
        plan_resolved(&resolved(&corpus, &named, true))
    };
    assert_eq!(across(ask(3)).legs().len(), 6, "three joined arms are the cap");
    assert!(
        matches!(
            ask(4),
            Err(PlanError::Refused(RefusalReason::QuestionNeedsTooManyLegs {
                legs: 8,
                limit: 6
            }))
        ),
        "four joined arms are eight legs"
    );
}

#[test]
fn metrics_on_different_sources_beside_a_remote_one_plan_over_three_data_systems() {
    let corpus = TwoSources::new();
    let revenue = simple("revenue");
    let orders = model("orders", "other", &["amount", "region_code", "day"]);
    let sales = Metric::new(
        MetricName::parse("sales").expect("a test metric is a metric"),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount")))),
        Vec::new(),
        column("day"),
        BTreeSet::from([Grain::Month]),
        vec![Dimension::new(
            dimension_name("region"),
            column("region_code"),
            None,
            None,
            Description::default(),
        )],
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("no dimensions to duplicate");
    let sales_alone = Resolution {
        metric: &sales,
        metrics: vec![&sales],
        model: &orders,
        grain: Grain::Month,
        range: june(),
        keys: vec![ResolvedDimension {
            dimension: sales
                .dimension(&dimension_name("region"))
                .expect("the metric declares this dimension"),
            join: None,
        }],
        filters: Vec::new(),
        top: None,
        cross: None,
    };
    let both = Resolved {
        whole: corpus.asking(vec![&revenue, &sales], true),
        per_metric: vec![corpus.asking(vec![&revenue], true), sales_alone],
    };
    let planned = across(plan_resolved(&both));
    assert_eq!(
        planned.legs().len(),
        3,
        "one fact and one lookup for revenue, one fact for sales"
    );
    assert_eq!(
        planned.sources().collect::<BTreeSet<_>>().len(),
        3,
        "local, remote and other are three data systems, and the lookup-leg cap bounds them"
    );
}

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
fn metrics_that_reach_a_dimension_by_different_paths_are_an_arm_each() {
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
    let both = Resolved {
        whole: Resolution {
            metrics: vec![&joined, &own],
            ..reach(&joined, true, placed)
        },
        per_metric: vec![reach(&joined, true, placed), reach(&own, false, placed)],
    };
    let planned = across(plan_resolved(&both));
    let read: Vec<(String, String)> = planned
        .arms()
        .iter()
        .map(|arm| {
            assert!(matches!(arm, Arm::Local(_)), "no dimension is on another data system");
            read_from(arm.fact().keys().first())
        })
        .collect();
    assert_eq!(
        read,
        [
            (String::from("dim_segments"), String::from("name")),
            (String::from("dim_facts"), String::from("segment_name")),
        ],
        "each arm reads its own metric's column, not the first metric's"
    );
}

/// `held`'s twin under another name, so two arms of one question can be told apart.
fn twin() -> Metric {
    Metric::new(
        MetricName::parse("twin").expect("a test metric is a metric"),
        ModelName::parse("facts").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("day"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("a metric with no dimensions has none to duplicate")
}

/// The corpus metric asked over `first` and `twin` asked over `second`, each by the dimensions `by`.
fn arms_over<'a>(first: &'a Corpus, second: &'a Corpus, twin: &'a Metric, by: &[&str]) -> Resolved<'a> {
    let asked = |corpus: &'a Corpus| corpus.asking(by.iter().map(|name| corpus.key(name)).collect());
    Resolved {
        whole: asked(first),
        per_metric: vec![
            asked(first),
            Resolution {
                metric: twin,
                metrics: vec![twin],
                ..asked(second)
            },
        ],
    }
}

/// An arm reads up to three lookup legs, and a fourth is refused by name with the count.
#[test]
fn an_arm_over_several_lookup_legs_plans_up_to_the_cap() {
    let twin = twin();
    let three = Corpus::placed("a", "a", "b");
    let planned = across(plan_resolved(&arms_over(&three, &three, &twin, &["region", "brand"])));
    assert_eq!(
        planned.legs().len(),
        6,
        "a fact leg and two lookup legs, for each of two arms"
    );
    let four = Corpus::spread("a", "b", "c", "d");
    let refused = plan_resolved(&arms_over(&four, &four, &twin, &["region", "brand"]));
    assert!(
        matches!(
            refused,
            Err(PlanError::Refused(RefusalReason::PlanNeedsTooManyLookupLegs {
                legs: 4,
                limit: 3
            }))
        ),
        "an arm over four lookup legs is refused, got {:?}",
        refused.err()
    );
}

/// The data systems one answer reads are counted once each over every arm: four plan, five are refused.
#[test]
fn an_answer_over_several_metrics_reads_four_data_systems_and_not_five() {
    let twin = twin();
    let first = Corpus::spread("a", "b", "local", "local");
    let four = Corpus::spread("a", "c", "local", "local");
    let planned = across(plan_resolved(&arms_over(&first, &four, &twin, &["region"])));
    assert_eq!(
        planned.sources().collect::<BTreeSet<_>>().len(),
        4,
        "local, a, b and c; the source both arms read counts once"
    );
    let five = Corpus::spread("c", "d", "local", "local");
    let refused = plan_resolved(&arms_over(&first, &five, &twin, &["region"]));
    assert!(
        matches!(
            refused,
            Err(PlanError::Refused(RefusalReason::PlanSpansTooManySources {
                sources: 5,
                limit: 4
            }))
        ),
        "local, a, b, c and d are five data systems, got {:?}",
        refused.err()
    );
}

/// The configured ceiling bounds several metrics both ways: raised to five, the five-source answer the
/// default refuses plans; lowered to three, a four-source answer is refused naming three.
#[test]
fn several_metrics_are_held_to_the_configured_ceiling_and_not_the_default() {
    let ceiling = |sources| ceilings(sources, 3, 6);
    let twin = twin();
    let first = Corpus::spread("a", "b", "local", "local");
    let five = Corpus::spread("c", "d", "local", "local");
    let raised = across(crate::plan::plan_resolved(
        &arms_over(&first, &five, &twin, &["region"]),
        ceiling(5),
    ));
    assert_eq!(
        raised.sources().collect::<BTreeSet<_>>().len(),
        5,
        "local, a, b, c and d plan under a ceiling of five"
    );
    let four = Corpus::spread("a", "c", "local", "local");
    let lowered = crate::plan::plan_resolved(&arms_over(&first, &four, &twin, &["region"]), ceiling(3));
    assert!(
        matches!(
            lowered,
            Err(PlanError::Refused(RefusalReason::PlanSpansTooManySources {
                sources: 4,
                limit: 3
            }))
        ),
        "local, a, b and c are four data systems over a ceiling of three, got {:?}",
        lowered.err()
    );
}

/// The configured leg ceiling counts every leg of every metric, each side of a value below and above
/// the default six: two legs per joined arm, so four arms plan under eight and five are refused
/// naming eight, and two arms plan under four and three are refused naming four.
#[test]
fn several_metrics_are_held_to_the_configured_leg_ceiling_lower_or_higher_than_six() {
    let corpus = TwoSources::new();
    let metrics: Vec<Metric> = ["a", "b", "c", "d", "e"].into_iter().map(simple).collect();
    let ask = |count: usize, legs: usize| {
        let named: Vec<&Metric> = metrics.iter().take(count).collect();
        crate::plan::plan_resolved(&resolved(&corpus, &named, true), ceilings(4, 3, legs))
    };
    assert_eq!(across(ask(4, 8)).legs().len(), 8, "four joined arms plan under eight");
    assert!(
        matches!(
            ask(5, 8),
            Err(PlanError::Refused(RefusalReason::QuestionNeedsTooManyLegs {
                legs: 10,
                limit: 8
            }))
        ),
        "five joined arms are ten legs, over eight"
    );
    assert_eq!(across(ask(2, 4)).legs().len(), 4, "two joined arms plan under four");
    assert!(
        matches!(
            ask(3, 4),
            Err(PlanError::Refused(RefusalReason::QuestionNeedsTooManyLegs {
                legs: 6,
                limit: 4
            }))
        ),
        "three joined arms are six legs, over four"
    );
}

/// The configured lookup leg ceiling bounds each arm of several metrics, each side of the default three:
/// raised to four, the arm over four lookup legs the default refuses plans; lowered to one, an arm over
/// two is refused naming one.
#[test]
fn an_arm_is_held_to_the_configured_lookup_leg_ceiling_lower_or_higher_than_three() {
    let twin = twin();
    let four = Corpus::spread("a", "b", "c", "d");
    let raised = across(crate::plan::plan_resolved(
        &arms_over(&four, &four, &twin, &["region", "brand"]),
        ceilings(5, 4, 10),
    ));
    assert_eq!(
        raised.legs().len(),
        10,
        "a fact leg and four lookup legs, for each of two arms"
    );
    let two = Corpus::placed("a", "a", "b");
    let lowered = crate::plan::plan_resolved(&arms_over(&two, &two, &twin, &["region", "brand"]), ceilings(4, 1, 6));
    assert!(
        matches!(
            lowered,
            Err(PlanError::Refused(RefusalReason::PlanNeedsTooManyLookupLegs {
                legs: 2,
                limit: 1
            }))
        ),
        "an arm over two lookup legs is refused naming the configured one, got {:?}",
        lowered.err()
    );
}
