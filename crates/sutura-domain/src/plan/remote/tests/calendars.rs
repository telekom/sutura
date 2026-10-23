//! Which calendar a ratio across two models is bucketed on: the one the planner picks.
//!
//! `cost_ratio` sums `orders` on sales over `tickets` on crm. Each source holds a calendar, so the
//! bundle lists two, named so that the crm calendar sorts first. This node's planner refuses
//! `cost_ratio` as `CrossModelRatioSpansSources`; the cells hold which calendar `certify` takes for
//! a leg on each source, not a leg the planner built.

use super::*;
use crate::catalog::SharedCalendars;

fn on(name: &str, source_name: &str, table_name: &str, columns: &[&str]) -> Model {
    Model::new(
        model_name(name),
        source(source_name),
        table(table_name),
        columns.iter().map(|c| column(c)),
        Description::default(),
    )
}

/// The bundle, with each calendar's table name and whether the ratio lists its calendars.
fn calendars(sales_table: &str, crm_table: &str, listed: bool) -> PinnedDefinitions {
    let models = vec![
        on("orders", "sales", "orders", &["amount_cents", "order_date", "day_key"]),
        on("sales_calendar", "sales", sales_table, &["day_key", "order_date"]),
        on("tickets", "crm", "tickets", &["cost", "day_key"]),
        on("crm_calendar", "crm", crm_table, &["day_key", "order_date"]),
    ];
    let relationships = vec![
        relationship("orders_calendar", "orders", "sales_calendar", "day_key", "day_key"),
        relationship("tickets_calendar", "tickets", "crm_calendar", "day_key", "day_key"),
    ];
    let ratio = Metric::new(
        metric_name("cost_ratio"),
        model_name("orders"),
        Measure::Ratio {
            numerator: sum("amount_cents"),
            denominator: Term::Aggregate(AggregatedColumn::on_model(
                Aggregate::Sum,
                column("cost"),
                model_name("tickets"),
            )),
            zero_denominator: ZeroDenominator::Null,
        },
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        None,
        Description::default(),
        Audience::Open,
    )
    .expect("no dimensions to collide");
    let ratio = if listed {
        ratio.with_shared_calendars(
            SharedCalendars::parse([model_name("sales_calendar"), model_name("crm_calendar")]).expect("two calendars"),
        )
    } else {
        ratio
    };
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        Definitions::assemble(models, relationships, vec![ratio]).expect("the test bundle is consistent"),
        Knowledge::none(),
        ContributionManifest::single(source("sales"), Contribution::of(MetadataCapabilities::nothing())),
    )
    .expect("the test definitions hash")
}

/// A leg of `cost_ratio` on `source`, rooted on `root`, joined through `joins` and bucketed on
/// `bucket`'s order date, summing `term`.
fn leg(pinned: &PinnedDefinitions, source_name: &str, root: &str, joins: &[&str], bucket: &str, term: PlanColumn) -> Value {
    let definitions = pinned.definitions();
    let joins = joins
        .iter()
        .map(|name| {
            let relationship = definitions.relationship(&relationship_name(name)).expect("declared");
            let target = definitions.model(relationship.target_model()).expect("declared");
            PlanJoin::hop(relationship, &table(root), target)
        })
        .collect();
    let leg = LegPlan::Fact {
        source: source(source_name),
        metric: metric_name("cost_ratio"),
        tables: StatementTables::parse(QualifiedTable::from(table(root)), joins).expect("distinct tables"),
        bucket: PlanBucket::new(ResultLabel::bucket(), Grain::Month, col(bucket, "order_date")),
        keys: Vec::new(),
        terms: vec![LegTerm::new(
            PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: term,
            },
            ResultLabel::internal(InternalLabel::Leaf(0)),
        )],
        bindings: PlanBindings::parse(in_range(col(bucket, "order_date")).into(), dated()).expect("coherent"),
        range: range(),
    };
    sent(&leg, pinned)
}

fn verdict(pinned: &PinnedDefinitions, json: Value) -> Result<LegPlan, LegNotCertified> {
    parsed(json, pinned, dated())
}

#[test]
fn a_ratio_leg_on_its_own_source_is_bucketed_on_the_calendar_listed_there() {
    let pinned = calendars("sales_calendar", "crm_calendar", true);
    let json = leg(
        &pinned,
        "sales",
        "orders",
        &["orders_calendar"],
        "sales_calendar",
        col("orders", "amount_cents"),
    );
    let accepted = verdict(&pinned, json);
    assert!(accepted.is_ok(), "{accepted:?}");
}

#[test]
fn a_ratio_leg_on_its_own_source_is_not_bucketed_on_another_sources_calendar() {
    let pinned = calendars("sales_calendar", "crm_calendar", true);
    let json = leg(
        &pinned,
        "sales",
        "orders",
        &["orders_calendar"],
        "crm_calendar",
        col("orders", "amount_cents"),
    );
    let refused = verdict(&pinned, json);
    assert!(
        matches!(refused, Err(LegNotCertified::BucketNotTimeColumn { ref table, .. }) if *table == self::table("crm_calendar")),
        "{refused:?}"
    );
}

#[test]
fn a_leg_on_another_source_is_not_bucketed_on_a_calendar_of_the_same_name() {
    let pinned = calendars("calendar", "calendar", true);
    let json = leg(
        &pinned,
        "crm",
        "tickets",
        &["tickets_calendar"],
        "calendar",
        col("tickets", "cost"),
    );
    let refused = verdict(&pinned, json);
    assert!(
        matches!(refused, Err(LegNotCertified::BucketNotTimeColumn { .. })),
        "{refused:?}"
    );
}

#[test]
fn a_crossing_ratio_without_a_shared_calendar_is_not_bucketed_on_its_own_table() {
    let pinned = calendars("sales_calendar", "crm_calendar", false);
    let json = leg(&pinned, "sales", "orders", &[], "orders", col("orders", "amount_cents"));
    let refused = verdict(&pinned, json);
    assert!(
        matches!(refused, Err(LegNotCertified::BucketNotTimeColumn { ref table, .. }) if *table == self::table("orders")),
        "{refused:?}"
    );
}
