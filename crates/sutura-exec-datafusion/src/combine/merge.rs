//! The drill-across merge: every arm's answer joined above its legs, in one session and one pool.
//!
//! Each arm is cut to one logical plan - a joined arm by `arm_plan`, the plan the two-leg
//! combine runs, a local arm by one aggregate over its fact leg - and the plans are joined left to
//! right into ONE plan that executes once. So the working-set ceiling is the question's, whatever the
//! arm count, and no arm's rows are collected before the join.
//!
//! **The join is FULL on every key and the bucket, with a null equal to a null** (`NullEquality::
//! NullEqualsNull`, `datafusion-expr` 55.1.0's `join_detailed`). Two arms that each group a row under
//! a null key - a LEFT lookup that found nothing - are one group, not two rows. FULL keeps a group only
//! one arm carries, and the other arm's measure answers null there: "no rows" stays visible, and a
//! ratio arm cannot fail the answer by dividing an invented zero.
//!
//! **What it refuses is a key or a bucket the arms type differently.** Two columns of different
//! Arrow kinds cannot be compared without choosing a reading of a value - a text `"01"` and an integer
//! `1` are one group under a cast and two without it - so the merge refuses before it joins, never
//! casts. The types are read off each arm's cut plan, so a key on a joined arm's lookup leg counts as
//! much as one on its fact leg. A kind is the join key's own (`Utf8` and `LargeUtf8` are one kind); a
//! pair of types neither of which is a join-key type must be the same type.

use datafusion::arrow::datatypes::DataType;
use datafusion::common::{Column, JoinType, NullEquality, TableReference};
use datafusion::functions::expr_fn::coalesce;
use datafusion::logical_expr::{Expr, LogicalPlan, LogicalPlanBuilder};
use datafusion::prelude::SessionContext;
use sutura_domain::plan::{Arm, ArmLegs, DrillAcrossCombiner, DrillAcrossPlan, LocalArm};
use sutura_domain::warehouse::ResultBatches;

use super::arm::{above_expression, leaf_labels, qualified, scan, schema_mismatch};
use super::schema::{FACT, LegSchema, same_kind};
use super::{CombineError, DataFusionCombiner, FACT_TABLE, arm_plan, bounded_session, executed, register, working_set_of};
use crate::collect::outputs;
use crate::pool::WorkingSet;

/// The name the running join of the arms so far is registered under.
const MERGED: &str = "merged";

/// The name the arm being joined in is registered under.
const NEXT: &str = "next_arm";

impl DrillAcrossCombiner for DataFusionCombiner {
    fn merge(&self, plan: &DrillAcrossPlan, arms: &[ArmLegs<'_>], working_set_bytes: u64) -> Result<ResultBatches, Self::Error> {
        let working_set = working_set_of(working_set_bytes);
        self.runtime()?.block_on(merged(plan, arms, working_set, working_set_bytes))
    }
}

async fn merged(
    plan: &DrillAcrossPlan,
    arms: &[ArmLegs<'_>],
    working_set: WorkingSet,
    ceiling_bytes: u64,
) -> Result<ResultBatches, CombineError> {
    if arms.len() != plan.arms().len() {
        return Err(CombineError::ArmsMiswired {
            arm: arms.len().min(plan.arms().len()),
        });
    }
    let context = bounded_session(working_set)?;
    let grouped: Vec<&str> = plan.keys().into_iter().chain([plan.bucket_label()]).collect();
    let first_legs = arms.first().ok_or(CombineError::ArmsMiswired { arm: 0 })?;
    let first = cut(
        0,
        plan.arms().first(),
        first_legs,
        &grouped,
        &context,
        (ceiling_bytes, working_set),
    )
    .await?;
    let first_kinds = kinds_of(&first, &grouped)?;
    let mut running = aliased(first, MERGED)?;
    let mut measures = vec![plan.arms().first().measure_label()];
    for (index, (arm, legs)) in plan.arms().iter().zip(arms).enumerate().skip(1) {
        let next = cut(index, arm, legs, &grouped, &context, (ceiling_bytes, working_set)).await?;
        agreeing_kinds(&grouped, plan.bucket_label(), &first_kinds, &kinds_of(&next, &grouped)?)?;
        running = joined_with(running, next, &grouped, &measures, arm.measure_label())?;
        measures.push(arm.measure_label());
    }
    let labels: Vec<String> = grouped.iter().chain(&measures).map(|label| String::from(*label)).collect();
    let (projection, ordering) = outputs(running.schema(), &labels, grouped.len()).map_err(schema_mismatch)?;
    let logical = LogicalPlanBuilder::from(running)
        .project(projection)
        .and_then(|projected| projected.sort_by(ordering))
        .and_then(LogicalPlanBuilder::build)
        .map_err(|cause| CombineError::Build { cause })?;
    executed(&context, logical, labels, measures.len(), ceiling_bytes, working_set).await
}

/// One arm's logical plan.
async fn cut(
    index: usize,
    arm: &Arm,
    legs: &ArmLegs<'_>,
    grouped: &[&str],
    context: &SessionContext,
    (ceiling_bytes, working_set): (u64, WorkingSet),
) -> Result<LogicalPlan, CombineError> {
    match (arm, legs) {
        (Arm::Joined(joined), ArmLegs::Joined(legs)) => arm_plan(joined, legs, context, ceiling_bytes, working_set).await,
        (Arm::Local(local), ArmLegs::Local { metric, batches }) if *metric == arm.metric() => {
            local_plan(local, grouped, context, batches).await
        }
        (Arm::Joined(_), ArmLegs::Local { .. }) | (Arm::Local(_), ArmLegs::Joined(_) | ArmLegs::Local { .. }) => {
            Err(CombineError::ArmsMiswired { arm: index })
        }
    }
}

/// The Arrow type each `grouped` label comes back as in `plan`, which names every label whichever
/// leg of the arm it was read off.
fn kinds_of(plan: &LogicalPlan, grouped: &[&str]) -> Result<Vec<DataType>, CombineError> {
    grouped
        .iter()
        .map(|label| {
            plan.schema()
                .field_with_unqualified_name(label)
                .map(|field| field.data_type().clone())
                .map_err(|cause| CombineError::Build { cause })
        })
        .collect()
}

/// Refuses an arm whose grouped columns are not of the first arm's kinds, before any join is built.
///
/// A type is read, never a value: the refusal names the two Arrow types, so a key the legs returned
/// cannot reach it.
fn agreeing_kinds(grouped: &[&str], bucket: &str, first: &[DataType], other: &[DataType]) -> Result<(), CombineError> {
    for ((label, first), other) in grouped.iter().zip(first).zip(other) {
        if same_kind(first, other) {
            continue;
        }
        let (first, other) = (format!("{first:?}"), format!("{other:?}"));
        return Err(if *label == bucket {
            CombineError::BucketKindMismatch { first, other }
        } else {
            CombineError::KeyKindMismatch {
                label: String::from(*label),
                first,
                other,
            }
        });
    }
    Ok(())
}

/// A local arm as a plan: its one fact leg, aggregated to the answer's keys and bucket and divided
/// through the arm's own tree, projected as `grouped` then the measure.
async fn local_plan(
    arm: &LocalArm,
    grouped: &[&str],
    context: &SessionContext,
    batches: &ResultBatches,
) -> Result<LogicalPlan, CombineError> {
    let schema = LegSchema::of(FACT, batches.schema())?;
    let leaves = leaf_labels(arm.federation(), &schema, None)?;
    for label in grouped {
        schema.projects(label)?;
    }
    register(context, FACT_TABLE, FACT, batches)?;
    let grouping: Vec<Expr> = grouped.iter().map(|label| qualified(FACT_TABLE, label)).collect();
    let measure = above_expression(arm.federation().above(), &leaves, &mut 0)?;
    let aggregated = LogicalPlanBuilder::from(scan(context, FACT_TABLE).await?)
        .aggregate(grouping, vec![measure])
        .map_err(|cause| CombineError::Build { cause })?;
    let labels: Vec<String> = grouped
        .iter()
        .copied()
        .chain([arm.measure_label()])
        .map(String::from)
        .collect();
    let (projection, _) = outputs(aggregated.schema(), &labels, grouped.len()).map_err(schema_mismatch)?;
    aggregated
        .project(projection)
        .and_then(LogicalPlanBuilder::build)
        .map_err(|cause| CombineError::Build { cause })
}

fn aliased(plan: LogicalPlan, name: &str) -> Result<LogicalPlan, CombineError> {
    LogicalPlanBuilder::from(plan)
        .alias(name)
        .and_then(LogicalPlanBuilder::build)
        .map_err(|cause| CombineError::Build { cause })
}

/// `left` FULL joined with `next` on every `grouped` label, a null equal to a null, the joined
/// labels coalesced and `next`'s measure appended after `measures`.
fn joined_with(
    left: LogicalPlan,
    next: LogicalPlan,
    grouped: &[&str],
    measures: &[&str],
    measure: &str,
) -> Result<LogicalPlan, CombineError> {
    let on = |table: &'static str| -> Vec<Column> {
        grouped
            .iter()
            .map(|label| Column::new(Some(TableReference::bare(table)), *label))
            .collect()
    };
    let mut projection: Vec<Expr> = grouped
        .iter()
        .map(|label| coalesce(vec![qualified(MERGED, label), qualified(NEXT, label)]).alias(*label))
        .collect();
    projection.extend(measures.iter().map(|label| qualified(MERGED, label)));
    projection.push(qualified(NEXT, measure));
    LogicalPlanBuilder::from(left)
        .join_detailed(
            aliased(next, NEXT)?,
            JoinType::Full,
            (on(MERGED), on(NEXT)),
            None,
            NullEquality::NullEqualsNull,
        )
        .and_then(|joined| joined.project(projection))
        .and_then(|projected| projected.alias(MERGED))
        .and_then(LogicalPlanBuilder::build)
        .map_err(|cause| CombineError::Build { cause })
}

#[cfg(test)]
mod tests {
    //! What the drill-across merge answers, and what it refuses to join.
    //!
    //! Every cell builds its arms from [`LocalArm`]s and one [`Arm::Joined`], over Arrow batches the
    //! legs would have returned. The merge is new API, so no base tree runs these: each cell's
    //! kill is a mutation of this module, in the commit body.

    use std::error::Error as _;
    use std::sync::Arc;

    use datafusion::arrow::array::{Int64Array, LargeStringArray, RecordBatch};
    use datafusion::arrow::datatypes::{DataType, Field, Schema};
    use sutura_domain::calendar::{Date, TimeRange};
    use sutura_domain::catalog::TIME_BUCKET_LABEL;
    use sutura_domain::federation::Federation;
    use sutura_domain::measure::{AggregatedColumn, Measure, Term};
    use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, SourceName, TableName};
    use sutura_domain::plan::leg::{LegPlan, LegTerm};
    use sutura_domain::plan::{
        AnswerKey, Arm, ArmLegs, DrillAcrossCombiner as _, DrillAcrossPlan, FederatedAnswerRefusal, FederatedPlan,
        FederationCombiner as _, InternalLabel, LegResult, Legs, LocalArm, LookupLegs, PlanBindings, PlanBucket, PlanColumn,
        PlanKey, PlanTerm, ResultLabel, StatementTables, labels,
    };
    use sutura_domain::warehouse::arrow::of_row_set;
    use sutura_domain::warehouse::{Accumulating, Real, ResultBatches, ResultBudget, RowSet, Value};

    use crate::combine::{CombineError, DataFusionCombiner};

    const FACT: &str = "fct_usage";
    const UNBOUNDED: u64 = 1 << 30;

    fn combiner() -> DataFusionCombiner {
        DataFusionCombiner::new().expect("a combiner builds")
    }

    fn metric(name: &str) -> MetricName {
        MetricName::parse(name).expect("a test metric is a metric")
    }

    fn column(name: &str) -> ColumnName {
        ColumnName::parse(name).expect("a test column is a column")
    }

    fn table(name: &str) -> TableName {
        TableName::parse(name).expect("a test table is a table")
    }

    fn text(value: &str) -> Value {
        Value::Text(String::from(value))
    }

    fn region_key() -> PlanKey {
        PlanKey::new(
            ResultLabel::dimension(&DimensionName::parse("region").expect("a test dimension")),
            PlanColumn::new(table(FACT), column("region")),
        )
    }

    fn link_key() -> PlanKey {
        PlanKey::new(
            ResultLabel::internal(InternalLabel::link(0)),
            PlanColumn::new(table(FACT), column("customer_key")),
        )
    }

    fn bucket() -> PlanBucket {
        PlanBucket::new(
            ResultLabel::bucket(),
            Grain::Month,
            PlanColumn::new(table(FACT), column("month")),
        )
    }

    fn federation() -> Federation {
        Federation::of(&Measure::Simple(Term::Aggregate(AggregatedColumn::new(
            Aggregate::Sum,
            column("amount"),
        ))))
    }

    fn terms(federation: &Federation) -> Vec<LegTerm> {
        labels(federation)
            .into_iter()
            .map(|label| {
                LegTerm::new(
                    PlanTerm::CountIf {
                        column: PlanColumn::new(table(FACT), column("amount")),
                    },
                    ResultLabel::internal(label),
                )
            })
            .collect()
    }

    fn fact_leg(name: &str, keys: Vec<PlanKey>, federation: &Federation) -> LegPlan {
        LegPlan::Fact {
            source: SourceName::parse("facts").expect("a test source"),
            metric: metric(name),
            tables: StatementTables::only(table(FACT)),
            bucket: bucket(),
            keys,
            terms: terms(federation),
            bindings: PlanBindings::none(),
            range: TimeRange::new(
                Date::parse("2026-01-01").expect("a test start"),
                Date::parse("2026-02-01").expect("a test end"),
            )
            .expect("a test range"),
        }
    }

    /// A metric whose one fact leg is grouped by region.
    fn local(name: &str) -> Arm {
        local_by(name, &["region"])
    }

    /// A metric whose one fact leg is grouped by each of `dimensions`, which are also the answer's keys.
    fn local_by(name: &str, dimensions: &[&str]) -> Arm {
        let federation = federation();
        let labels = |dimension: &&str| ResultLabel::dimension(&DimensionName::parse(dimension).expect("a test dimension"));
        let keys = dimensions.iter().map(labels).collect();
        let leg_keys = dimensions
            .iter()
            .map(|dimension| PlanKey::new(labels(dimension), PlanColumn::new(table(FACT), column(dimension))))
            .collect();
        let fact = fact_leg(name, leg_keys, &federation);
        Arm::Local(Box::new(
            LocalArm::new(fact, federation, keys).expect("a test local arm is well formed"),
        ))
    }

    /// A metric whose fact leg reaches a lookup leg for the region.
    fn joined(name: &str) -> (Arm, FederatedPlan) {
        let federation = federation();
        let lookup = LegPlan::Lookup {
            source: SourceName::parse("geo").expect("a test source"),
            metric: sutura_domain::model::MetricName::parse("revenue").expect("a test metric is a metric"),
            table: table(FACT).into(),
            keys: vec![link_key(), region_key()],
            bindings: PlanBindings::none(),
        };
        let plan = FederatedPlan::new(
            metric(name),
            ResultLabel::measure(&metric(name)),
            bucket(),
            fact_leg(name, vec![link_key()], &federation),
            None,
            LookupLegs::one(lookup, true),
            federation,
            vec![AnswerKey::lookup(
                0,
                ResultLabel::dimension(&DimensionName::parse("region").expect("a test dimension")),
            )],
        )
        .expect("a test joined plan");
        (Arm::Joined(Box::new(plan.clone())), plan)
    }

    /// One result per leg of `plan`, in the plan's order.
    fn results_of(plan: &FederatedPlan, every: [ResultBatches; 2]) -> Vec<LegResult> {
        plan.legs()
            .into_iter()
            .zip(every)
            .map(|(leg, batches)| LegResult::of(leg, batches))
            .collect()
    }

    fn leaf() -> String {
        InternalLabel::Leaf(0).label()
    }

    fn batches<const N: usize>(columns: [String; N], rows: Vec<[Value; N]>) -> ResultBatches {
        let rows = rows.into_iter().map(Vec::from).collect();
        of_row_set(&RowSet::new(Vec::from(columns), rows).expect("a test result is well formed")).expect("a result converts")
    }

    /// A local arm's leg result: region, period, one leaf per row.
    fn leg(rows: Vec<(Value, i64)>) -> ResultBatches {
        batches(
            [String::from("region"), String::from(TIME_BUCKET_LABEL), leaf()],
            rows.into_iter()
                .map(|(region, total)| [region, text("2026-06"), Value::Integer(total)])
                .collect(),
        )
    }

    fn plan_of(arms: Vec<Arm>) -> DrillAcrossPlan {
        let mut arms = arms.into_iter();
        let (first, second) = (arms.next().expect("a first arm"), arms.next().expect("a second arm"));
        DrillAcrossPlan::new(first, second, arms.collect()).expect("a test drill-across plan")
    }

    /// The results handed over in order, each tagged with the metric its arm measures: `revenue`, then
    /// `visits`, then `tickets`.
    fn merged(plan: &DrillAcrossPlan, results: &[&ResultBatches], ceiling: u64) -> Result<ResultBatches, CombineError> {
        let metrics: Vec<MetricName> = ["revenue", "visits", "tickets", "extra"]
            .into_iter()
            .take(results.len())
            .map(metric)
            .collect();
        let arms: Vec<ArmLegs<'_>> = metrics
            .iter()
            .zip(results)
            .map(|(metric, batches)| ArmLegs::Local { metric, batches })
            .collect();
        combiner().merge(plan, &arms, ceiling)
    }

    fn rows(plan: &DrillAcrossPlan, results: &[&ResultBatches]) -> Vec<Vec<Value>> {
        merged(plan, results, UNBOUNDED)
            .expect("this merge answers")
            .to_rows()
            .expect("a merged answer decodes")
            .rows()
            .to_vec()
    }

    #[test]
    fn an_arm_with_no_rows_for_a_group_answers_null() {
        let plan = plan_of(vec![local("revenue"), local("visits")]);
        let revenue = leg(vec![(text("north"), 100), (text("south"), 50)]);
        let visits = leg(vec![(text("north"), 7)]);

        assert_eq!(
            rows(&plan, &[&revenue, &visits]),
            vec![
                vec![text("north"), text("2026-06"), Value::Integer(100), Value::Integer(7)],
                vec![text("south"), text("2026-06"), Value::Integer(50), Value::Null],
            ],
            "a group one metric lacks answers null under it: it is neither dropped nor zero"
        );
    }

    #[test]
    fn a_null_key_group_two_arms_share_is_one_row() {
        let plan = plan_of(vec![local("revenue"), local("visits")]);
        let revenue = leg(vec![(Value::Null, 10), (text("north"), 1)]);
        let visits = leg(vec![(Value::Null, 20), (text("south"), 5)]);

        assert_eq!(
            rows(&plan, &[&revenue, &visits]),
            vec![
                vec![text("north"), text("2026-06"), Value::Integer(1), Value::Null],
                vec![text("south"), text("2026-06"), Value::Null, Value::Integer(5)],
                vec![Value::Null, text("2026-06"), Value::Integer(10), Value::Integer(20)],
            ],
            "a null equals a null in the merge's join, so the group is one row with both measures"
        );
    }

    #[test]
    fn three_arms_join_by_the_keys_each_carries() {
        let plan = plan_of(vec![local("revenue"), local("visits"), local("tickets")]);
        let revenue = leg(vec![(text("a"), 1), (text("b"), 2)]);
        let visits = leg(vec![(text("b"), 20)]);
        let tickets = leg(vec![(text("a"), 100), (text("c"), 300)]);

        assert_eq!(
            rows(&plan, &[&revenue, &visits, &tickets]),
            vec![
                vec![
                    text("a"),
                    text("2026-06"),
                    Value::Integer(1),
                    Value::Null,
                    Value::Integer(100)
                ],
                vec![text("b"), text("2026-06"), Value::Integer(2), Value::Integer(20), Value::Null],
                vec![text("c"), text("2026-06"), Value::Null, Value::Null, Value::Integer(300)],
            ],
            "a group only the last arm carries is read through the coalesced key of the arms before it"
        );
    }

    #[test]
    fn a_joined_arm_and_a_local_arm_merge_under_one_plan() {
        let (arm, plan) = joined("revenue");
        let drill = plan_of(vec![arm, local("visits")]);
        let fact = batches(
            [InternalLabel::link(0).label(), String::from(TIME_BUCKET_LABEL), leaf()],
            vec![
                [text("c1"), text("2026-06"), Value::Integer(100)],
                [text("c2"), text("2026-06"), Value::Integer(200)],
            ],
        );
        let lookup = of_row_set(
            &RowSet::new(
                vec![InternalLabel::link(0).label(), String::from("region")],
                vec![vec![text("c1"), text("north")], vec![text("c2"), text("north")]],
            )
            .expect("a test lookup"),
        )
        .expect("a lookup converts");
        let visits = leg(vec![(text("north"), 7), (text("south"), 3)]);
        let results = results_of(&plan, [fact, lookup]);
        let legs = Legs::assign(&plan, &results).expect("one result per leg");
        let answered = combiner()
            .merge(
                &drill,
                &[
                    ArmLegs::Joined(legs),
                    ArmLegs::Local {
                        metric: &metric("visits"),
                        batches: &visits,
                    },
                ],
                UNBOUNDED,
            )
            .expect("the arms merge")
            .to_rows()
            .expect("the answer decodes");

        assert_eq!(answered.columns(), &["region", "period", "revenue", "visits"]);
        assert_eq!(
            answered.rows(),
            &[
                vec![text("north"), text("2026-06"), Value::Integer(300), Value::Integer(7)],
                vec![text("south"), text("2026-06"), Value::Null, Value::Integer(3)],
            ]
        );
    }

    #[test]
    fn arms_whose_buckets_have_different_arrow_types_are_refused_typed() {
        let plan = plan_of(vec![local("revenue"), local("visits")]);
        let revenue = leg(vec![(text("north"), 1)]);
        let visits = batches(
            [String::from("region"), String::from(TIME_BUCKET_LABEL), leaf()],
            vec![[text("north"), Value::Integer(202_606), Value::Integer(7)]],
        );

        let error = merged(&plan, &[&revenue, &visits], UNBOUNDED).expect_err("two kinds of period cannot be joined");
        assert!(
            matches!(&error, CombineError::BucketKindMismatch { first, other } if *first == format!("{:?}", DataType::Utf8) && *other == format!("{:?}", DataType::Int64)),
            "{error:?}"
        );
        assert_eq!(
            combiner().answer_not_well_formed(&error),
            Some(FederatedAnswerRefusal::BucketKindMismatch),
            "the refusal is about the data the legs returned, and a caller can read it"
        );
    }

    #[test]
    fn the_ceiling_bounds_the_whole_merge_and_a_generous_one_answers() {
        let plan = plan_of(vec![local("revenue"), local("visits")]);
        let many = |scale: i64| leg((0..2_000_i64).map(|n| (text(&format!("r{n}")), n * scale)).collect());
        let (revenue, visits) = (many(1), many(2));

        let error = merged(&plan, &[&revenue, &visits], 1).expect_err("a one-byte ceiling refuses the first reservation");
        assert_eq!(combiner().working_set_exhausted(&error), Some(1), "{error:?}");
        assert_eq!(combiner().answer_not_well_formed(&error), None);
        assert_eq!(
            rows(&plan, &[&revenue, &visits]).len(),
            2_000,
            "under a generous ceiling the same arms answer, one row per region"
        );
    }

    #[test]
    fn leg_results_that_are_not_the_plans_arms_are_a_wiring_defect() {
        let plan = plan_of(vec![local("revenue"), local("visits")]);
        let revenue = leg(vec![(text("north"), 1)]);
        let (_, joined_plan) = joined("revenue");
        let results = results_of(&joined_plan, [revenue.clone(), revenue.clone()]);
        let legs = Legs::assign(&joined_plan, &results).expect("one result per leg");

        let none = combiner().merge(&plan, &[], UNBOUNDED).expect_err("no results for two arms");
        let one = merged(&plan, &[&revenue], UNBOUNDED).expect_err("one result for two arms");
        let wrong = combiner()
            .merge(
                &plan,
                &[
                    ArmLegs::Local {
                        metric: &metric("revenue"),
                        batches: &revenue,
                    },
                    ArmLegs::Joined(legs),
                ],
                UNBOUNDED,
            )
            .expect_err("a joined arm's legs where a local arm's result belongs");

        assert!(matches!(none, CombineError::ArmsMiswired { arm: 0 }), "{none:?}");
        assert!(matches!(one, CombineError::ArmsMiswired { arm: 1 }), "{one:?}");
        assert!(matches!(wrong, CombineError::ArmsMiswired { arm: 1 }), "{wrong:?}");
        assert_eq!(combiner().answer_not_well_formed(&wrong), None, "ours, never the question's");
    }

    /// One leg row's two text cells and its total.
    type Row<'a> = (&'a str, &'a str, i64);

    /// A local arm's leg result with one row per `(region, period, total)`.
    fn periods(rows: Vec<Row<'_>>) -> ResultBatches {
        batches(
            [String::from("region"), String::from(TIME_BUCKET_LABEL), leaf()],
            rows.into_iter()
                .map(|(region, period, total)| [text(region), text(period), Value::Integer(total)])
                .collect(),
        )
    }

    #[test]
    fn two_periods_of_one_key_join_on_the_period() {
        let plan = plan_of(vec![local("revenue"), local("visits")]);
        let revenue = periods(vec![("north", "2026-05", 1), ("north", "2026-06", 2)]);
        let visits = periods(vec![("north", "2026-06", 20), ("north", "2026-07", 30)]);

        assert_eq!(
            rows(&plan, &[&revenue, &visits]),
            vec![
                vec![text("north"), text("2026-05"), Value::Integer(1), Value::Null],
                vec![text("north"), text("2026-06"), Value::Integer(2), Value::Integer(20)],
                vec![text("north"), text("2026-07"), Value::Null, Value::Integer(30)],
            ],
            "the periods stay apart: a period joins its own period, and a lone one answers null under the other arm"
        );
    }

    #[test]
    fn two_keys_join_on_both() {
        let plan = plan_of(vec![
            local_by("revenue", &["region", "tier"]),
            local_by("visits", &["region", "tier"]),
        ]);
        let pairs = |rows: Vec<Row<'_>>| {
            batches(
                [
                    String::from("region"),
                    String::from("tier"),
                    String::from(TIME_BUCKET_LABEL),
                    leaf(),
                ],
                rows.into_iter()
                    .map(|(region, tier, total)| [text(region), text(tier), text("2026-06"), Value::Integer(total)])
                    .collect(),
            )
        };
        let revenue = pairs(vec![("north", "gold", 1), ("north", "silver", 2), ("south", "gold", 3)]);
        let visits = pairs(vec![("north", "gold", 10), ("north", "silver", 20)]);

        let period = || text("2026-06");
        assert_eq!(
            rows(&plan, &[&revenue, &visits]),
            vec![
                vec![text("north"), text("gold"), period(), Value::Integer(1), Value::Integer(10)],
                vec![text("north"), text("silver"), period(), Value::Integer(2), Value::Integer(20)],
                vec![text("south"), text("gold"), period(), Value::Integer(3), Value::Null],
            ],
            "a group is every key together: two arms agreeing on one key and differing on the other are two groups"
        );
    }

    #[test]
    fn a_non_finite_measure_is_refused_in_any_arm() {
        let plan = plan_of(vec![local("revenue"), local("visits")]);
        let real_leg = |totals: &[f64]| {
            batches(
                [String::from("region"), String::from(TIME_BUCKET_LABEL), leaf()],
                totals
                    .iter()
                    .map(|total| {
                        [
                            text("north"),
                            text("2026-06"),
                            Value::Real(Real::parse(*total).expect("a finite test real")),
                        ]
                    })
                    .collect(),
            )
        };
        let overflowing = real_leg(&[f64::MAX, f64::MAX]);
        let finite = real_leg(&[7.0]);

        let error = merged(&plan, &[&overflowing, &finite], UNBOUNDED).expect_err("a sum that left the finite range");

        assert!(matches!(error, CombineError::NonFinite), "{error:?}");
        assert_eq!(
            combiner().answer_not_well_formed(&error),
            Some(FederatedAnswerRefusal::NonFinite),
            "refused here as the question's, not left to fail at the presentation edge as a retryable fault"
        );
    }

    #[test]
    fn more_results_than_arms_is_a_wiring_defect() {
        let plan = plan_of(vec![local("revenue"), local("visits")]);
        let (first, second, third) = (
            leg(vec![(text("north"), 1)]),
            leg(vec![(text("north"), 2)]),
            leg(vec![(text("north"), 3)]),
        );

        let error = merged(&plan, &[&first, &second, &third], UNBOUNDED).expect_err("three results for two arms");

        assert!(matches!(error, CombineError::ArmsMiswired { arm: 2 }), "{error:?}");
    }

    #[test]
    fn a_local_result_tagged_with_another_arms_metric_is_a_wiring_defect() {
        let plan = plan_of(vec![local("revenue"), local("visits")]);
        let (revenue, visits) = (leg(vec![(text("north"), 100)]), leg(vec![(text("north"), 7)]));
        let swapped = [
            ArmLegs::Local {
                metric: &metric("visits"),
                batches: &visits,
            },
            ArmLegs::Local {
                metric: &metric("revenue"),
                batches: &revenue,
            },
        ];

        let error = combiner()
            .merge(&plan, &swapped, UNBOUNDED)
            .expect_err("each result is under the other arm's metric");

        assert!(matches!(error, CombineError::ArmsMiswired { arm: 0 }), "{error:?}");
        assert_eq!(combiner().answer_not_well_formed(&error), None, "ours, never the question's");
    }

    /// A local arm's leg result whose key and period columns are `LargeUtf8` and whose leaf is `Int64`.
    fn large_text_leg(region: &str, total: i64) -> ResultBatches {
        let leaf = leaf();
        let schema = Arc::new(Schema::new(vec![
            Field::new("region", DataType::LargeUtf8, true),
            Field::new(TIME_BUCKET_LABEL, DataType::LargeUtf8, true),
            Field::new(leaf.as_str(), DataType::Int64, true),
        ]));
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![
                Arc::new(LargeStringArray::from(vec![region])),
                Arc::new(LargeStringArray::from(vec!["2026-06"])),
                Arc::new(Int64Array::from(vec![total])),
            ],
        )
        .expect("a test batch is well formed");
        let mut accumulating = Accumulating::announcing(schema, 1, ResultBudget::of_bytes(core::num::NonZeroUsize::MAX));
        accumulating.push(batch).expect("a test batch agrees with its own schema");
        accumulating.finish()
    }

    #[test]
    fn keys_of_different_kinds_are_refused_and_the_refusal_names_no_key() {
        let plan = plan_of(vec![local("revenue"), local("visits")]);
        let revenue = leg(vec![(text("abc"), 1), (text("01"), 2)]);
        let visits = leg(vec![(Value::Integer(1), 7)]);

        let error = merged(&plan, &[&revenue, &visits], UNBOUNDED).expect_err("a text key and an integer key cannot be joined");

        assert!(
            matches!(&error, CombineError::KeyKindMismatch { label, first, other } if label == "region" && *first == format!("{:?}", DataType::Utf8) && *other == format!("{:?}", DataType::Int64)),
            "{error:?}"
        );
        assert_eq!(
            combiner().answer_not_well_formed(&error),
            Some(FederatedAnswerRefusal::KeyKindMismatch)
        );
        let mut told = vec![error.to_string(), format!("{error:?}")];
        let mut cause = error.source();
        while let Some(source) = cause {
            told.push(source.to_string());
            told.push(format!("{source:?}"));
            cause = source.source();
        }
        for text in told {
            assert!(!text.contains("abc") && !text.contains("01"), "a key value reached `{text}`");
        }
    }

    #[test]
    fn a_key_and_a_bucket_of_either_text_width_are_one_kind() {
        let plan = plan_of(vec![local("revenue"), local("visits")]);
        let (revenue, visits) = (leg(vec![(text("north"), 100)]), large_text_leg("north", 7));

        assert_eq!(
            rows(&plan, &[&revenue, &visits]),
            vec![vec![text("north"), text("2026-06"), Value::Integer(100), Value::Integer(7)]],
            "Utf8 against LargeUtf8 is one kind of key and one kind of period, and joins"
        );
    }
}
