//! A metric's declared shared calendar: `telekom/sutura#780`'s `docs/adr/0002` second amendment.
//!
//! A concept module rather than a split of [`super`], which was over `cargo xtask
//! max-lines`'s thousand-line cap - the same reason [`super::chain`] and
//! [`super::column_metadata`] are their own files.

use super::*;
use crate::calendar::{Date, TimeRange};
use crate::catalog::{Anchor, AnchorValue};

/// `orders`, `customers` and `calendar`, all local: `orders` and `customers` each reach
/// `calendar` through their own relationship, in addition to the `orders`-`customers` link
/// [`two_models`] already declares.
fn three_models_with_calendar() -> ModelsAndJoins {
    let (mut models, mut relationships) = two_models();
    models.push(model("calendar", "local", &["day_key", "order_date"]));
    // `orders` and `customers` each need a column to join the calendar on - added by replacing
    // the two models rather than mutating in place, since `Model` has no column-adding method.
    models[0] = model("orders", "local", &["amount_cents", "order_date", "customer_id", "day_key"]);
    models[1] = model("customers", "local", &["id", "region_code", "day_key"]);
    relationships.push(Relationship::new(
        relationship_name("orders_calendar"),
        model_name("orders"),
        model_name("calendar"),
        JoinType::ManyToOne,
        JoinKeys::of(vec![JoinKey::Equal {
            origin: column("day_key"),
            target: column("day_key"),
        }])
        .expect("a test relationship declares one key"),
    ));
    relationships.push(Relationship::new(
        relationship_name("customers_calendar"),
        model_name("customers"),
        model_name("calendar"),
        JoinType::ManyToOne,
        JoinKeys::of(vec![JoinKey::Equal {
            origin: column("day_key"),
            target: column("day_key"),
        }])
        .expect("a test relationship declares one key"),
    ));
    (models, relationships)
}

#[test]
fn a_shared_calendar_naming_an_undeclared_model_is_refused() {
    // A metric may declare a `shared_calendar`, checked at load. Naming a model this catalog
    // never declared is refused before either fact model is even considered - the same "does the
    // name resolve at all" question `UnknownTermModel` answers for a ratio term's model.
    let broken = Metric::new(
        metric_name("revenue"),
        model_name("orders"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("no dimensions to duplicate")
    .with_shared_calendar(model_name("no_such_calendar"));
    assert_eq!(
        assemble_one(broken).unwrap_err(),
        InconsistentDefinitions::SharedCalendarNotReachable {
            metric: metric_name("revenue"),
            calendar: model_name("no_such_calendar"),
            model: model_name("no_such_calendar"),
        }
    );
}

#[test]
fn a_shared_calendar_the_metric_s_own_model_cannot_reach_is_refused() {
    // The calendar model exists this time, but nothing declares a relationship from `orders` (the
    // metric's own model) to it - [`two_models`] has no such link. A calendar one fact cannot
    // reach is a definition whose ratio could never be bucketed through it, refused at
    // declaration time rather than surfacing as a plan-time failure over some later question.
    let (mut models, relationships) = two_models();
    models.push(model("calendar", "local", &["day_key", "day"]));
    let broken = Metric::new(
        metric_name("revenue"),
        model_name("orders"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("no dimensions to duplicate")
    .with_shared_calendar(model_name("calendar"));
    assert_eq!(
        Definitions::assemble(models, relationships, vec![broken]).unwrap_err(),
        InconsistentDefinitions::SharedCalendarNotReachable {
            metric: metric_name("revenue"),
            calendar: model_name("calendar"),
            model: model_name("orders"),
        }
    );
}

/// `revenue_per_customer`: `orders`' `amount_cents` over a count of `customers`' `id`, bucketed
/// through the shared `calendar`.
fn ratio_over_two_facts() -> Metric {
    ratio_over_two_facts_anchored(None)
}

fn ratio_over_two_facts_anchored(anchor: Option<Anchor>) -> Metric {
    Metric::new(
        metric_name("revenue_per_customer"),
        model_name("orders"),
        Measure::Ratio {
            numerator: Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents"))),
            denominator: Term::Aggregate(AggregatedColumn::on_model(
                Aggregate::Count,
                column("id"),
                model_name("customers"),
            )),
            zero_denominator: crate::measure::ZeroDenominator::Null,
        },
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        anchor,
        Description::default(),
        Audience::Open,
    )
    .expect("no dimensions to duplicate")
    .with_shared_calendar(model_name("calendar"))
}

fn june_anchor() -> Anchor {
    let june = TimeRange::new(
        Date::parse("2026-06-01").expect("a test date is a date"),
        Date::parse("2026-07-01").expect("a test date is a date"),
    )
    .expect("June is a range");
    Anchor::new(june, AnchorValue::parse("12").expect("a test anchor value is a value"))
}

#[test]
fn an_anchor_on_a_cross_model_ratio_is_refused_at_load_by_name() {
    // The boot check runs an anchor as one statement over the metric's own model, so it cannot
    // certify a two-fact ratio; the same catalog without the anchor assembles (the next cell).
    let anchored = ratio_over_two_facts_anchored(Some(june_anchor()));
    let (models, relationships) = three_models_with_calendar();
    assert_eq!(
        Definitions::assemble(models, relationships, vec![anchored]).unwrap_err(),
        InconsistentDefinitions::AnchorOnCrossModelRatio {
            metric: metric_name("revenue_per_customer"),
        }
    );
}

#[test]
fn a_shared_calendar_reachable_from_both_a_ratio_s_fact_models_assembles() {
    // The positive control: `orders` (the metric's own model) and `customers` (the ratio's
    // second fact model) each reach `calendar` through their own relationship, so the shared
    // calendar this metric declares holds together rather than being a name nothing can use.
    let (models, relationships) = three_models_with_calendar();
    let definitions = Definitions::assemble(models, relationships, vec![ratio_over_two_facts()])
        .expect("both fact models reach the declared calendar");
    assert!(definitions.metric(&metric_name("revenue_per_customer")).is_some());
}

#[test]
fn a_one_model_ratio_with_an_anchor_and_a_calendar_assembles() {
    // Cross-model is a term naming another model, not a declared calendar: both terms here read
    // `orders`, so its anchor is the one-statement number the boot check certifies.
    let one_model = Metric::new(
        metric_name("revenue_per_order"),
        model_name("orders"),
        Measure::Ratio {
            numerator: Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents"))),
            denominator: Term::Aggregate(AggregatedColumn::new(Aggregate::Count, column("customer_id"))),
            zero_denominator: crate::measure::ZeroDenominator::Null,
        },
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        Some(june_anchor()),
        Description::default(),
        Audience::Open,
    )
    .expect("no dimensions to duplicate")
    .with_shared_calendar(model_name("calendar"));
    let (models, relationships) = three_models_with_calendar();
    let definitions = Definitions::assemble(models, relationships, vec![one_model]).expect("a one-model ratio may be anchored");
    assert!(
        definitions
            .metric(&metric_name("revenue_per_order"))
            .is_some_and(|metric| metric.anchor().is_some())
    );
}

/// The same catalog with one relationship replaced - by name - or dropped, and the refusal it
/// must produce naming the model at fault.
fn refused_without(relationship: &str, replacement: Option<Relationship>) -> InconsistentDefinitions {
    let (models, mut relationships) = three_models_with_calendar();
    relationships.retain(|declared| declared.name() != &relationship_name(relationship));
    relationships.extend(replacement);
    Definitions::assemble(models, relationships, vec![ratio_over_two_facts()])
        .expect_err("a calendar one fact cannot join is refused at load")
}

fn not_reachable_from(model: &str) -> InconsistentDefinitions {
    InconsistentDefinitions::SharedCalendarNotReachable {
        metric: metric_name("revenue_per_customer"),
        calendar: model_name("calendar"),
        model: model_name(model),
    }
}

#[test]
fn a_shared_calendar_a_ratio_term_s_model_cannot_reach_is_refused() {
    // The metric's own model reaches the calendar; the SECOND fact does not, and it is the one named.
    assert_eq!(refused_without("customers_calendar", None), not_reachable_from("customers"));
}

#[test]
fn a_calendar_that_only_joins_toward_the_fact_is_refused() {
    // A relationship FROM the calendar TO `orders` is not the hop a fact leg joins - each leg
    // joins the calendar from its own table - so it does not count as reaching it.
    let reversed = Relationship::new(
        relationship_name("calendar_orders"),
        model_name("calendar"),
        model_name("orders"),
        JoinType::OneToMany,
        JoinKeys::of(vec![JoinKey::Equal {
            origin: column("day_key"),
            target: column("day_key"),
        }])
        .expect("a test relationship declares one key"),
    );
    assert_eq!(
        refused_without("orders_calendar", Some(reversed)),
        not_reachable_from("orders")
    );
}

#[test]
fn a_shared_calendar_without_the_metric_s_time_column_is_refused() {
    // Both legs bucket and bound on the metric's time column READ ON THE CALENDAR, so a calendar
    // that does not declare it is a plan naming a column that does not exist.
    let (mut models, relationships) = three_models_with_calendar();
    let calendar = models
        .iter_mut()
        .find(|m| m.name() == &model_name("calendar"))
        .expect("the fixture declares it");
    *calendar = model("calendar", "local", &["day_key", "day"]);
    assert_eq!(
        Definitions::assemble(models, relationships, vec![ratio_over_two_facts()]).unwrap_err(),
        InconsistentDefinitions::UnknownTimeColumn {
            metric: metric_name("revenue_per_customer"),
            model: model_name("calendar"),
            column: column("order_date"),
        }
    );
}

/// What the load says of [`ratio_with_a`]'s metric: its refusal, if any.
fn refused_with_a(aggregate: Aggregate) -> Option<InconsistentDefinitions> {
    let (models, relationships) = three_models_with_calendar();
    Definitions::assemble(models, relationships, vec![ratio_with_a(aggregate)]).err()
}

/// `revenue_per_customer` with its denominator's aggregate replaced by `aggregate`.
fn ratio_with_a(aggregate: Aggregate) -> Metric {
    let numerator = Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")));
    Metric::new(
        metric_name("revenue_per_customer"),
        model_name("orders"),
        Measure::Ratio {
            numerator,
            denominator: Term::Aggregate(AggregatedColumn::on_model(aggregate, column("id"), model_name("customers"))),
            zero_denominator: crate::measure::ZeroDenominator::Null,
        },
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("no dimensions to duplicate")
    .with_shared_calendar(model_name("calendar"))
}

fn no_empty_value(aggregate: Aggregate) -> InconsistentDefinitions {
    InconsistentDefinitions::CrossModelTermHasNoEmptyValue {
        metric: metric_name("revenue_per_customer"),
        aggregate,
    }
}

// A group only one fact reached reads the other fact's term over no rows. An average, a minimum
// and a maximum have no value there - not 0, not null - so each definition is refused at load.
#[test]
fn a_cross_model_ratio_with_a_term_that_has_no_empty_value_is_refused() {
    assert_eq!(refused_with_a(Aggregate::Max), Some(no_empty_value(Aggregate::Max)));
}

#[test]
fn a_cross_model_ratio_with_a_minimum_term_is_refused() {
    assert_eq!(refused_with_a(Aggregate::Min), Some(no_empty_value(Aggregate::Min)));
}

/// The load check is the ONLY guard for an average: it reaches the combiner as a sum leaf and a
/// count leaf, which pass its minimum-or-maximum guard.
#[test]
fn a_cross_model_ratio_with_an_average_term_is_refused() {
    assert_eq!(refused_with_a(Aggregate::Avg), Some(no_empty_value(Aggregate::Avg)));
}

#[test]
fn a_cross_model_ratio_with_a_distinct_count_term_is_refused_at_load() {
    assert_eq!(
        refused_with_a(Aggregate::CountDistinct),
        Some(InconsistentDefinitions::CrossModelTermDoesNotReaggregate {
            metric: metric_name("revenue_per_customer"),
            aggregate: Aggregate::CountDistinct,
        })
    );
}
