//! What a [`StackedPlan`] refuses to be, each refusal beside the arms it is built from.

use crate::calendar::{Date, TimeRange};
use crate::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, QualifiedTable, SourceName, TableName};
use crate::plan::{
    PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanKey, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin, QueryPlan,
    ResultLabel, StackedPlan, StackedPlanError, StatementTables,
};
use crate::query::{Top, TopBy, TopDirection, TopN};
use crate::warehouse::ParamValue;

fn day(iso: &str) -> Date {
    Date::parse(iso).expect("a test date is a date")
}

fn metric(name: &str) -> MetricName {
    MetricName::parse(name).expect("a test metric is a metric")
}

fn column(table: &str, name: &str) -> PlanColumn {
    PlanColumn::new(
        TableName::parse(table).expect("a test table is a table"),
        ColumnName::parse(name).expect("a test column is a column"),
    )
}

/// The arm of `name` over its own table `fct_<name>`, keyed by `keys`, monthly over June unless changed.
struct Arm {
    name: &'static str,
    source: &'static str,
    keys: Vec<&'static str>,
    grain: Grain,
    end: &'static str,
}

impl Arm {
    fn of(name: &'static str) -> Self {
        Self {
            name,
            source: "warehouse",
            keys: vec!["region"],
            grain: Grain::Month,
            end: "2026-07-01",
        }
    }

    fn plan(&self) -> QueryPlan {
        let table = format!("fct_{}", self.name);
        let bindings = PlanBindings::parse(
            vec![PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::AtOrAfter {
                    column: column(&table, "d"),
                    param: 0,
                },
            )],
            vec![ParamValue::Text(String::from(self.name))],
        )
        .expect("a test plan binds its one parameter");
        let keys = self
            .keys
            .iter()
            .map(|key| {
                let label = ResultLabel::dimension(&DimensionName::parse(key).expect("a test dimension is a dimension"));
                PlanKey::new(label, column(&table, key))
            })
            .collect();
        QueryPlan::new(
            SourceName::parse(self.source).expect("a test source is a source"),
            metric(self.name),
            StatementTables::parse(
                QualifiedTable::from(TableName::parse(&table).expect("a test table is a table")),
                Vec::new(),
            )
            .expect("one table is distinguishable"),
            PlanBucket::new(ResultLabel::bucket(), self.grain, column(&table, "d")),
            keys,
            PlanMeasure::Simple {
                term: PlanTerm::Aggregate {
                    aggregate: Aggregate::Sum,
                    column: column(&table, "amount"),
                },
            },
            ResultLabel::measure(&metric(self.name)),
            bindings,
            TimeRange::new(day("2026-06-01"), day(self.end)).expect("a test range is a range"),
        )
    }
}

fn stacked(first: &Arm, second: &Arm) -> Result<StackedPlan, StackedPlanError> {
    StackedPlan::new(first.plan(), second.plan(), Vec::new())
}

fn top() -> Top {
    Top::new(TopN::parse(3).expect("three is a top"), TopBy::Metric, TopDirection::Desc)
}

#[test]
fn two_arms_on_one_source_answer_every_measure_and_bind_every_parameter_in_arm_order() {
    let plan = stacked(&Arm::of("revenue"), &Arm::of("orders")).expect("two agreeing arms stack");
    assert_eq!(plan.result_labels(), ["region", "time_bucket", "revenue", "orders"]);
    assert_eq!(
        plan.params(),
        [
            ParamValue::Text(String::from("revenue")),
            ParamValue::Text(String::from("orders"))
        ]
    );
    assert_eq!(plan.with_top(top()).top(), Some(top()));
}

#[test]
fn an_arm_carrying_a_top_is_refused() {
    let carrying = Arm::of("orders").plan().with_top(top());
    assert_eq!(
        StackedPlan::new(Arm::of("revenue").plan(), carrying, Vec::new()),
        Err(StackedPlanError::ArmCarriesTop {
            metric: metric("orders")
        })
    );
}

#[test]
fn a_metric_in_two_arms_is_refused() {
    assert_eq!(
        stacked(&Arm::of("revenue"), &Arm::of("revenue")),
        Err(StackedPlanError::DuplicateLabel {
            metric: metric("revenue")
        })
    );
}

#[test]
fn an_arm_on_another_source_is_refused() {
    let elsewhere = Arm {
        source: "elsewhere",
        ..Arm::of("orders")
    };
    assert_eq!(
        stacked(&Arm::of("revenue"), &elsewhere),
        Err(StackedPlanError::SourceDiffers {
            metric: metric("orders")
        })
    );
}

#[test]
fn an_arm_with_other_keys_is_refused() {
    let other = Arm {
        keys: vec!["region", "channel"],
        ..Arm::of("orders")
    };
    assert_eq!(
        stacked(&Arm::of("revenue"), &other),
        Err(StackedPlanError::KeysDiffer {
            metric: metric("orders")
        })
    );
}

#[test]
fn an_arm_at_another_grain_is_refused() {
    let daily = Arm {
        grain: Grain::Day,
        ..Arm::of("orders")
    };
    assert_eq!(
        stacked(&Arm::of("revenue"), &daily),
        Err(StackedPlanError::BucketDiffers {
            metric: metric("orders")
        })
    );
}

#[test]
fn an_arm_over_another_range_is_refused() {
    let longer = Arm {
        end: "2026-08-01",
        ..Arm::of("orders")
    };
    assert_eq!(
        stacked(&Arm::of("revenue"), &longer),
        Err(StackedPlanError::RangeDiffers {
            metric: metric("orders")
        })
    );
}
