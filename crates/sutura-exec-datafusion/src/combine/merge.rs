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
//! **What it refuses is one thing: a bucket the arms type differently.** Two period columns of
//! different Arrow types cannot be compared without choosing a reading of a period, so the merge
//! refuses rather than casts. A key of different types across arms is NOT checked: the engine's own
//! coercion decides, and a pair it cannot coerce leaves as this workspace's own failure.

use datafusion::arrow::datatypes::DataType;
use datafusion::common::{Column, JoinType, NullEquality, TableReference};
use datafusion::functions::expr_fn::coalesce;
use datafusion::logical_expr::{Expr, LogicalPlan, LogicalPlanBuilder};
use datafusion::prelude::SessionContext;
use sutura_domain::plan::{Arm, ArmLegs, DrillAcrossCombiner, DrillAcrossPlan, LocalArm};
use sutura_domain::warehouse::ResultBatches;

use super::arm::{above_expression, leaf_labels, qualified, scan, schema_mismatch};
use super::schema::{FACT, LegSchema};
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
    let context = bounded_session(working_set)?;
    let grouped: Vec<&str> = plan.keys().into_iter().chain([plan.bucket_label()]).collect();
    let first_legs = arms.first().ok_or(CombineError::ArmsMiswired { arm: 0 })?;
    let (first, bucket_kind) = cut(
        0,
        plan,
        plan.arms().first(),
        first_legs,
        &grouped,
        &context,
        (ceiling_bytes, working_set),
    )
    .await?;
    let mut running = aliased(first, MERGED)?;
    let mut measures = vec![plan.arms().first().measure_label()];
    for (index, (arm, legs)) in plan.arms().iter().zip(arms).enumerate().skip(1) {
        let (next, kind) = cut(index, plan, arm, legs, &grouped, &context, (ceiling_bytes, working_set)).await?;
        if kind != bucket_kind {
            return Err(CombineError::BucketKindMismatch {
                first: format!("{bucket_kind:?}"),
                other: format!("{kind:?}"),
            });
        }
        running = joined_with(running, next, &grouped, &measures, arm.measure_label())?;
        measures.push(arm.measure_label());
    }
    if arms.len() != plan.arms().len() {
        return Err(CombineError::ArmsMiswired {
            arm: arms.len().min(plan.arms().len()),
        });
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

/// One arm's logical plan, with the Arrow type its bucket column came back as.
async fn cut(
    index: usize,
    plan: &DrillAcrossPlan,
    arm: &Arm,
    legs: &ArmLegs<'_>,
    grouped: &[&str],
    context: &SessionContext,
    (ceiling_bytes, working_set): (u64, WorkingSet),
) -> Result<(LogicalPlan, DataType), CombineError> {
    let kind = LegSchema::of(FACT, fact_schema(legs))?.kind_of(plan.bucket_label())?.clone();
    let cut = match (arm, legs) {
        (Arm::Joined(joined), ArmLegs::Joined(legs)) => arm_plan(joined, *legs, context, ceiling_bytes, working_set).await?,
        (Arm::Local(local), ArmLegs::Local(batches)) => local_plan(local, grouped, context, batches).await?,
        (Arm::Joined(_), ArmLegs::Local(_)) | (Arm::Local(_), ArmLegs::Joined(_)) => {
            return Err(CombineError::ArmsMiswired { arm: index });
        }
    };
    Ok((cut, kind))
}

/// The schema of the leg a drill-across arm's bucket column is read off.
const fn fact_schema<'a>(legs: &ArmLegs<'a>) -> &'a datafusion::arrow::datatypes::SchemaRef {
    match *legs {
        ArmLegs::Joined(legs) => legs.fact().schema(),
        ArmLegs::Local(batches) => batches.schema(),
    }
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

    use datafusion::arrow::datatypes::DataType;
    use sutura_domain::calendar::{Date, TimeRange};
    use sutura_domain::catalog::TIME_BUCKET_LABEL;
    use sutura_domain::federation::Federation;
    use sutura_domain::measure::{AggregatedColumn, Measure, Term};
    use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, SourceName, TableName};
    use sutura_domain::plan::leg::{LegPlan, LegTerm};
    use sutura_domain::plan::{
        AnswerKey, Arm, ArmLegs, DrillAcrossCombiner as _, DrillAcrossPlan, FederatedAnswerRefusal, FederatedPlan,
        FederationCombiner as _, InternalLabel, LegResult, LocalArm, PlanBindings, PlanBucket, PlanColumn, PlanKey, PlanTerm,
        ResultLabel, StatementTables, labels,
    };
    use sutura_domain::warehouse::arrow::of_row_set;
    use sutura_domain::warehouse::{ResultBatches, RowSet, Value};

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
            ResultLabel::internal(InternalLabel::Link),
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
        let federation = federation();
        let fact = fact_leg(name, vec![region_key()], &federation);
        let keys = vec![ResultLabel::dimension(
            &DimensionName::parse("region").expect("a test dimension"),
        )];
        Arm::Local(Box::new(
            LocalArm::new(fact, federation, keys).expect("a test local arm is well formed"),
        ))
    }

    /// A metric whose fact leg reaches a lookup leg for the region.
    fn joined(name: &str) -> (Arm, FederatedPlan) {
        let federation = federation();
        let lookup = LegPlan::Lookup {
            source: SourceName::parse("geo").expect("a test source"),
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
            lookup,
            true,
            federation,
            vec![AnswerKey::lookup(ResultLabel::dimension(
                &DimensionName::parse("region").expect("a test dimension"),
            ))],
        )
        .expect("a test joined plan");
        (Arm::Joined(Box::new(plan.clone())), plan)
    }

    fn leaf() -> String {
        InternalLabel::Leaf(0).label()
    }

    fn batches(columns: [String; 3], rows: Vec<[Value; 3]>) -> ResultBatches {
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

    fn merged(plan: &DrillAcrossPlan, results: &[&ResultBatches], ceiling: u64) -> Result<ResultBatches, CombineError> {
        let arms: Vec<ArmLegs<'_>> = results.iter().map(|batches| ArmLegs::Local(batches)).collect();
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
        let fact = LegResult::of(
            plan.fact(),
            batches(
                [InternalLabel::Link.label(), String::from(TIME_BUCKET_LABEL), leaf()],
                vec![
                    [text("c1"), text("2026-06"), Value::Integer(100)],
                    [text("c2"), text("2026-06"), Value::Integer(200)],
                ],
            ),
        );
        let lookup = LegResult::of(
            plan.lookup(),
            of_row_set(
                &RowSet::new(
                    vec![InternalLabel::Link.label(), String::from("region")],
                    vec![vec![text("c1"), text("north")], vec![text("c2"), text("north")]],
                )
                .expect("a test lookup"),
            )
            .expect("a lookup converts"),
        );
        let visits = leg(vec![(text("north"), 7), (text("south"), 3)]);
        let legs = sutura_domain::plan::Legs::of(&fact, &lookup).expect("one of each");
        let answered = combiner()
            .merge(&drill, &[ArmLegs::Joined(legs), ArmLegs::Local(&visits)], UNBOUNDED)
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
        let fact = LegResult::of(joined_plan.fact(), revenue.clone());
        let lookup = LegResult::of(joined_plan.lookup(), revenue.clone());
        let legs = sutura_domain::plan::Legs::of(&fact, &lookup).expect("one of each");

        let none = combiner().merge(&plan, &[], UNBOUNDED).expect_err("no results for two arms");
        let one = merged(&plan, &[&revenue], UNBOUNDED).expect_err("one result for two arms");
        let wrong = combiner()
            .merge(&plan, &[ArmLegs::Local(&revenue), ArmLegs::Joined(legs)], UNBOUNDED)
            .expect_err("a joined arm's legs where a local arm's result belongs");

        assert!(matches!(none, CombineError::ArmsMiswired { arm: 0 }), "{none:?}");
        assert!(matches!(one, CombineError::ArmsMiswired { arm: 1 }), "{one:?}");
        assert!(matches!(wrong, CombineError::ArmsMiswired { arm: 1 }), "{wrong:?}");
        assert_eq!(combiner().answer_not_well_formed(&wrong), None, "ours, never the question's");
    }
}
