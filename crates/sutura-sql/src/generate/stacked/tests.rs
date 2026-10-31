use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, SourceName, TableName};
use sutura_domain::plan::{
    PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin, QueryPlan,
    ResultLabel, StackedPlan, StatementTables,
};
use sutura_domain::query::{Top, TopBy, TopDirection, TopN};
use sutura_domain::warehouse::ParamValue;

use super::generate_stacked;
use crate::dialect::{ALL, Dialect};

/// The whole answer `metric` would be alone: a monthly sum over `table`, bounded by two dates
/// that are `metric`'s own, so a parameter shows which arm it binds for.
fn arm(metric: &str, table: &str) -> QueryPlan {
    let col = |name: &str| {
        PlanColumn::new(
            TableName::parse(table).expect("a test table is a table"),
            ColumnName::parse(name).expect("a test column is a column"),
        )
    };
    let date = |raw: &str| Date::parse(raw).expect("a test date is a date");
    let metric = MetricName::parse(metric).expect("a test metric is a metric");
    let bindings = PlanBindings::parse(
        vec![
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::AtOrAfter {
                    column: col("booked_on"),
                    param: 0,
                },
            ),
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::Before {
                    column: col("booked_on"),
                    param: 1,
                },
            ),
        ],
        vec![
            ParamValue::Text(String::from(metric.as_str())),
            ParamValue::Date(date("2026-07-01")),
        ],
    )
    .expect("two range bounds bind in placeholder order");
    QueryPlan::new(
        SourceName::parse("local").expect("a test source is a source"),
        metric.clone(),
        StatementTables::parse(TableName::parse(table).expect("a test table is a table"), Vec::new())
            .expect("one table is unambiguous"),
        PlanBucket::new(ResultLabel::bucket(), Grain::Month, col("booked_on")),
        Vec::new(),
        PlanMeasure::Simple {
            term: PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: col("amount"),
            },
        },
        ResultLabel::measure(&metric),
        bindings,
        TimeRange::new(date("2026-06-01"), date("2026-07-01")).expect("a test range is a range"),
    )
}

fn stacked() -> StackedPlan {
    StackedPlan::new(arm("revenue", "orders"), arm("refunds", "returns"), Vec::new())
        .expect("two arms on one source with one bucket and range stack")
}

fn sql(plan: &StackedPlan, dialect: Dialect) -> String {
    let query = generate_stacked(plan, dialect).unwrap_or_else(|e| panic!("a stacked plan renders for {dialect}: {e}"));
    String::from(query.sql())
}

/// A later arm's numbered placeholders name its own values, past the earlier arm's.
#[test]
fn a_later_arm_binds_its_own_values_past_the_earlier_arms() {
    let plan = stacked();
    let query = generate_stacked(&plan, Dialect::Postgres).expect("a stacked plan renders for Postgres");
    let text = query.sql();
    let second = text.find("returns").expect("the second arm reads its own table");
    let (_, later) = text.split_at(second);
    assert!(later.contains("$3") && later.contains("$4"), "{text}");
    assert_eq!(
        query.params().first(),
        Some(&ParamValue::Text(String::from("revenue"))),
        "{text}"
    );
    assert_eq!(
        query.params().get(2),
        Some(&ParamValue::Text(String::from("refunds"))),
        "{text}"
    );
    assert_eq!(query.params().len(), 4, "{text}");
}

/// Every dialect stacks the arms, groups them again and takes each measure's `MAX`, in the case
/// that dialect spells it, bounded by one row past the cap.
#[test]
fn every_dialect_stacks_the_arms_and_takes_each_measures_max() {
    let plan = stacked();
    for &dialect in ALL {
        let text = sql(&plan, dialect);
        assert!(text.contains("UNION ALL"), "{dialect}:\n{text}");
        assert!(text.contains("NULL AS"), "{dialect}:\n{text}");
        assert_eq!(text.to_ascii_lowercase().matches("max(").count(), 2, "{dialect}:\n{text}");
        assert!(text.contains("10001"), "{dialect}:\n{text}");
    }
}

/// A `top` by metric ranks the joined rows by the last metric, as the in-process merge does.
#[test]
fn a_top_by_metric_ranks_by_the_last_metric() {
    let top = Top::new(
        TopN::parse(3).expect("three is a row count"),
        TopBy::Metric,
        TopDirection::Desc,
    );
    let text = sql(&stacked().with_top(top), Dialect::DuckDb);
    let (_, order_by) = text.split_once("ORDER BY").expect("a top orders the answer");
    assert!(order_by.starts_with(" MAX(\"refunds\") DESC"), "{text}");
    assert!(text.ends_with("LIMIT 3"), "{text}");
}
