//! The per-plan combine path: the one logical plan above the legs, and the labels and leaves it reads.

use datafusion::arrow::datatypes::DataType;
use datafusion::common::{Column, JoinType, TableReference};
use datafusion::functions::expr_fn::{abs, coalesce, isnan, nullif};
use datafusion::functions_aggregate::expr_fn::{count_distinct, max, min, sum};
use datafusion::logical_expr::{Expr, LogicalPlan, LogicalPlanBuilder, cast, lit, when};
use datafusion::prelude::SessionContext;
use sutura_domain::federation::{Above, Carried, Federation};
use sutura_domain::measure::ZeroDenominator;
use sutura_domain::model::Aggregate;
use sutura_domain::plan::{FederatedPlan, InternalLabel, labels};

use super::schema::{FACT, LeafKind, LegSchema};
use super::{CombineError, EXACT_SUM_PRECISION, FACT_TABLE, LOOKUP_TABLES, SECOND_FACT_TABLE, table_at};
use crate::collect::outputs;

/// The answer's column labels, in the order the answer projects them.
///
/// Keys in question order, then the fact leg's time bucket, then the measure under the metric's own
/// certified name. **The same order the mono path emits**, which is what makes a federated answer
/// and a single-source one comparable - and what lets `FederatedPlan::rank` find the measure by
/// position rather than by label.
pub(super) fn answer_labels(plan: &FederatedPlan) -> Vec<String> {
    let mut labels: Vec<String> = plan.keys().iter().map(|key| String::from(key.label())).collect();
    labels.push(String::from(plan.bucket_label()));
    labels.push(String::from(plan.measure_label()));
    labels
}

/// Every carried leaf, in `labels` order.
pub(super) type Leaves = Vec<Leaf>;

/// One carried leaf: the label the splitter projected it under, how its column is read above the
/// legs, and the registered table it is read from.
///
/// A named alias because the spelled-out form is past `clippy.toml`'s type-complexity threshold,
/// and naming it says which part of the triple is the label.
pub(super) type Leaf = (String, Reading, &'static str);

/// How a carried leaf's column is read above the legs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Reading {
    /// A column a leg aggregated, re-aggregated here by a sum, a minimum or a maximum.
    Reaggregated(LeafKind),
    /// A pulled-up key: the leg grouped by the column and the distinct values are counted here.
    ///
    /// Any type a single-source `COUNT(DISTINCT)` accepts, so no type is judged. `float` is the one
    /// type whose equality the engines disagree on, and [`distinct_key`] is what settles it.
    Counted { float: bool },
}

/// The label of every carried leaf, in carried order, with its type judged.
///
/// `labels(plan.federation())` is the SAME function the splitter names the fact leg's terms with, so
/// the column a leaf is read from and the label it was projected under cannot disagree - there is no
/// second copy of the naming rule. A two-fact plan (`telekom/sutura#780`) reads a leaf whose
/// `Carried::model` is `Some` off the second fact leg; every other leaf, and every leaf of a plan
/// with no second fact, reads the first. Decided here once, with the table it names, so the
/// expression built from a leaf cannot read a table this check did not.
pub(super) fn leaf_labels(
    federation: &Federation,
    fact: &LegSchema,
    second_fact: Option<&LegSchema>,
) -> Result<Leaves, CombineError> {
    let carried = federation.carried();
    labels(federation)
        .into_iter()
        .zip(carried.iter())
        .map(|(internal, carried)| {
            let label = internal.label();
            let (schema, table) = match (carried.model(), second_fact) {
                (Some(_), Some(second)) => (second, SECOND_FACT_TABLE),
                _ => (fact, FACT_TABLE),
            };
            let reading = if carried.is_pulled_up() {
                Reading::Counted {
                    float: schema.kind_of(&label)?.is_floating(),
                }
            } else {
                Reading::Reaggregated(schema.leaf_kind(&label)?)
            };
            Ok((label, reading, table))
        })
        .collect()
}

/// A qualified reference to one leg's column, built rather than parsed.
///
/// `Column::new` with a bare `TableReference`, never `col("fact.0_link")`: the parsing form
/// lowercases every part of a dotted name, which is the case-folding trap `crate::translate::column`
/// exists to avoid - and an internal label begins with a digit, which a parser would read as the
/// start of a number.
pub(super) fn qualified(table: &str, label: &str) -> Expr {
    Expr::Column(Column::new(Some(TableReference::bare(table)), label))
}

/// The whole combine, as one logical plan.
pub(super) async fn combine_plan(
    plan: &FederatedPlan,
    context: &SessionContext,
    link: &str,
    leaves: &[Leaf],
    second_table: Option<&str>,
) -> Result<LogicalPlan, CombineError> {
    let fact = scan(context, FACT_TABLE).await?;
    // A second fact leg (`telekom/sutura#780`) is joined on the link AND the bucket: both legs are
    // grouped by both, so each first-fact row meets at most the one second-fact row of its own
    // period. On the link alone a second fact with two periods fans every first-fact row out across
    // both and multiplies the numerator. FULL, not INNER - drill-across: a (link, period) only one
    // fact carried still counts on its own side. The owner's empty-set rule decides the other side:
    // a fact that has no row there reads each leaf as its value over NO rows, which is 0 for the
    // sum and the count every leaf re-aggregates by - never a null pad. So revenue with no visits
    // divides by 0 and `zero_denominator` answers as declared, and visits with no revenue is 0/x.
    // A side is absent exactly when its bucket is null: every leg row carries one, because the leg
    // bounds the range on that same column. The join columns are coalesced and the result re-named
    // as the fact leg, so everything below reads one relation.
    let mut builder = LogicalPlanBuilder::from(fact);
    let mut leaves = leaves.to_vec();
    if let Some(table) = second_table {
        let second = scan(context, table).await?;
        let bucket = plan.bucket_label();
        let on = [
            qualified(FACT_TABLE, link).eq(qualified(table, link)),
            qualified(FACT_TABLE, bucket).eq(qualified(table, bucket)),
        ];
        if let Some(aggregate) = plan
            .federation()
            .carried()
            .iter()
            .map(|leaf| leaf.combine())
            .find(|a| *a != Aggregate::Sum)
        {
            // A minimum or a maximum has no value over no rows; `Definitions::assemble` refuses the
            // definition, and this is the combine refusing to invent one if a plan ever carries it.
            // An average cannot be seen here - it arrives as a sum leaf and a count leaf - so the
            // load check is its only refusal.
            return Err(CombineError::UnsupportedAggregate { aggregate });
        }
        let joined = builder
            .join_on(second, JoinType::Full, on)
            .map_err(|cause| CombineError::Build { cause })?;
        let leaf_labels: Vec<&str> = leaves.iter().map(|leaf| leaf.0.as_str()).collect();
        let mut columns = vec![
            coalesce(vec![qualified(FACT_TABLE, link), qualified(table, link)]).alias(link),
            coalesce(vec![qualified(FACT_TABLE, bucket), qualified(table, bucket)]).alias(bucket),
        ];
        for (qualifier, field) in joined.schema().iter() {
            if field.name() == link || field.name() == bucket {
                continue;
            }
            let column = Expr::Column(Column::new(qualifier.cloned(), field.name()));
            match qualifier.filter(|_| leaf_labels.contains(&field.name().as_str())) {
                Some(side) => {
                    let absent = Expr::Column(Column::new(Some(side.clone()), bucket)).is_null();
                    let leaf = when(absent, cast(lit(0_i64), field.data_type().clone()))
                        .otherwise(column)
                        .map_err(|cause| CombineError::Build { cause })?;
                    columns.push(leaf.alias(field.name()));
                }
                None => columns.push(column),
            }
        }
        builder = joined
            .project(columns)
            .and_then(|merged| merged.alias(FACT_TABLE))
            .map_err(|cause| CombineError::Build { cause })?;
        for leaf in &mut leaves {
            leaf.2 = FACT_TABLE;
        }
    }
    // **The join kind is the splitter's decision, carried on each lookup leg rather than guessed.**
    // LEFT keeps a row of the leg it joins onto whose link value found no row here, with null keys
    // from this leg; INNER drops it. A NULL link value is decided identically under both, and by the
    // join rather than by an arm of ours: `NULL = NULL` is not true, so such a row matches nothing.
    // Each lookup leg joins onto the leg it names on its own link label, in plan order, so the leg
    // it names is already in the relation.
    for (position, (lookup, table)) in plan.lookups().legs().iter().zip(LOOKUP_TABLES).enumerate() {
        let link = InternalLabel::Link(position).label();
        let kind = if lookup.include_unmatched() {
            JoinType::Left
        } else {
            JoinType::Inner
        };
        let on = qualified(table_at(lookup.onto(), &link)?, &link).eq(qualified(table, &link));
        builder = builder
            .join_on(scan(context, table).await?, kind, [on])
            .map_err(|cause| CombineError::Build { cause })?;
    }

    // Keys in question order, then the bucket - the order `answer_labels` states, so the aggregate's
    // output fields line up with the labels position for position.
    let mut grouping: Vec<Expr> = plan
        .keys()
        .iter()
        .map(|key| Ok(qualified(table_at(key.side(), key.label())?, key.label())))
        .collect::<Result<_, CombineError>>()?;
    grouping.push(qualified(FACT_TABLE, plan.bucket_label()));
    let group_count = grouping.len();

    let measure = above_expression(plan.federation().above(), &leaves, &mut 0)?;
    builder = builder
        .aggregate(grouping, vec![measure])
        .map_err(|cause| CombineError::Build { cause })?;

    let labels = answer_labels(plan);
    let (projection, ordering) = outputs(builder.schema(), &labels, group_count).map_err(schema_mismatch)?;
    // `sort_by` is ascending, nulls last - the ordered-result contract
    // `sutura_sql::generate::ordered_nulls_last` states for the rendered path and the hand-written
    // combine's own comparator restated. A null means there was nothing to group under, and that
    // sorts after every value.
    builder
        .project(projection)
        .and_then(|projected| projected.sort_by(ordering))
        .and_then(LogicalPlanBuilder::build)
        .map_err(|cause| CombineError::Build { cause })
}

/// One registered leg, as a plan to build on.
pub(super) async fn scan(context: &SessionContext, table: &str) -> Result<LogicalPlan, CombineError> {
    let frame = context
        .table(TableReference::bare(table))
        .await
        .map_err(|cause| CombineError::Analyze { cause })?;
    Ok(frame.into_unoptimized_plan())
}

/// [`crate::collect::outputs`]'s own refusal, as this module's.
pub(super) fn schema_mismatch(cause: crate::DataFusionError) -> CombineError {
    match cause {
        crate::DataFusionError::SchemaMismatch { expected, actual } => CombineError::SchemaMismatch { expected, actual },
        other => CombineError::Build {
            cause: datafusion::error::DataFusionError::External(Box::new(other)),
        },
    }
}

/// The divide tree above the legs, as one expression over the aggregate's own inputs.
///
/// `cursor` walks the tree in the same order [`labels`] collects the federation's leaves in, which
/// is what pairs each [`Above::Total`] node with the column its own leaf was projected under. The
/// one caller starts it at zero, so there is no call site that can point it elsewhere.
///
/// **The division happens HERE and cannot happen in a leg**, which is `sutura_domain::federation`'s
/// own shape: a `Carried` has no quotient variant, so a per-leg guard is unrepresentable and the
/// [`ZeroDenominator`] the definition asked for is applied once, above every leg's rows.
pub(super) fn above_expression(above: &Above, leaves: &[Leaf], cursor: &mut usize) -> Result<Expr, CombineError> {
    match *above {
        Above::Total(ref carried) => {
            let &(ref label, reading, table) = leaves.get(*cursor).ok_or_else(|| CombineError::MissingColumn {
                side: FACT,
                label: InternalLabel::Leaf(*cursor).label(),
            })?;
            *cursor = cursor.saturating_add(1);
            leaf_expression(carried, qualified(table, label), reading)
        }
        Above::Quotient {
            ref numerator,
            ref denominator,
            zero_denominator,
        } => {
            // The same arithmetic `crate::translate::measure_expression` emits for the mono path, so
            // a ratio cannot mean one thing for one source and another for two: the numerator casts
            // to a float first (integer division truncates, which would silently answer a whole
            // number), and the zero handling is the definition's.
            let top = cast(above_expression(numerator, leaves, cursor)?, DataType::Float64);
            let bottom = cast(above_expression(denominator, leaves, cursor)?, DataType::Float64);
            let bottom = match zero_denominator {
                ZeroDenominator::Null => nullif(bottom, lit(0.0_f64)),
                // No guard, deliberately: the division answers a non-finite value and
                // `refuse_non_finite` refuses the answer. That is the same mechanism the engine's own
                // mono path has for this case - the alternative would be a second definition of what
                // `fails` means.
                ZeroDenominator::Fail => bottom,
            };
            Ok(top / bottom)
        }
    }
}

/// One carried leaf, re-aggregated by its own [`Carried::combine`].
///
/// Only four aggregates can arrive, and a `CountDistinct` only over a pulled-up key: any other pair
/// is a plan this workspace's own classification could not have built.
fn leaf_expression(carried: &Carried, column: Expr, reading: Reading) -> Result<Expr, CombineError> {
    let aggregate = carried.combine();
    match (aggregate, reading) {
        (Aggregate::CountDistinct, Reading::Counted { float }) => Ok(count_distinct(distinct_key(column, float)?)),
        (Aggregate::Sum, Reading::Reaggregated(kind)) => Ok(leaf_sum(column, kind)),
        // No cast: a minimum and a maximum are exact in the column's own type, and casting one would
        // be the widening `sutura_domain::warehouse::arrow` refuses for a 32-bit float.
        (Aggregate::Min, Reading::Reaggregated(_)) => Ok(min(column)),
        (Aggregate::Max, Reading::Reaggregated(_)) => Ok(max(column)),
        (Aggregate::Sum | Aggregate::Min | Aggregate::Max, Reading::Counted { .. })
        | (Aggregate::Count | Aggregate::Avg | Aggregate::CountDistinct, _) => {
            Err(CombineError::UnsupportedAggregate { aggregate })
        }
    }
}

/// The value a distinct count compares, which for a float is the one SQL equality means.
///
/// **A float is widened to 64 bits and canonicalised, because the engines a leg can come from do not
/// agree on what two floats are the same value.** `DuckDB` counts `-0.0` with `0.0` and every NaN as
/// one value; `DataFusion` counts the NaN payloads apart (`apache/datafusion#26091`), so a count
/// above would answer by which engine produced the leg. The widening is exact for every float width,
/// so no two distinct values meet. NULL stays NULL and is not counted.
///
/// **The `-0.0` arm is needed because the zeros are counted apart in some plan shapes and for some
/// key expressions.** `DataFusion` merges them in a lone distinct count over a plain column, and with
/// a sum beside it, but counts them apart when the one aggregate holds two distinct counts over
/// different columns (measured; the likely cause is that its single-distinct rewrite no longer
/// applies). A ratio of two distinct counts builds that aggregate. The key expression matters too:
/// with this arm's condition made never true, a lone distinct count over `0.0` under one link value
/// and `-0.0` under another counted 2, and with the arm deleted it counted 1. So the merge is not a
/// property of the plan shape alone, and the arm is needed in general. Without it the answer depends
/// on which zero a leg kept per link value. The arm is held by
/// `a_ratio_of_two_distinct_counts_counts_both_zeros_once`.
fn distinct_key(column: Expr, float: bool) -> Result<Expr, CombineError> {
    if !float {
        return Ok(column);
    }
    let wide = cast(column, DataType::Float64);
    // `abs`, because `DataFusion` compares floats in total order, where `-0.0 = 0.0` is false and
    // `abs(-0.0) = 0.0` is true.
    when(abs(wide.clone()).eq(lit(0.0_f64)), lit(0.0_f64))
        .when(isnan(wide.clone()), lit(f64::NAN))
        .otherwise(wide)
        .map_err(|cause| CombineError::Build { cause })
}

/// A leaf's total, accumulated wide enough that no total this workspace can produce wraps.
///
/// **`DataFusion`'s sum accumulator adds with WRAPPING arithmetic**, measured in the pinned 55.1.0
/// source: both the scalar and the grouped accumulator use `add_wrapping`, and `arrow`'s own
/// `sum` kernel documents that an overflow wraps rather than erroring. So the accumulator's WIDTH is
/// the bound, and the hand-written combine's `checked_add`-and-refuse is not available.
///
/// An exact leaf is therefore cast to a 256-bit decimal at the column's own scale before summing,
/// which is a better answer than the refusal it replaces: `ResultBatches::to_rows` widens a
/// zero-scale value that fits an `i64` back to an integer and renders one that does not as its
/// exact text, so a total past `i64::MAX` comes back **exact** where it used to be refused.
///
/// **The limit, next to the claim.** A 256-bit accumulator holds about `10^76`; a leaf value a
/// `Decimal128` column can carry is at most about `10^38`, so wrapping needs on the order of
/// `10^38` rows. No row ceiling in this workspace permits that, and it is a width rather than a
/// guard - a combiner that narrowed the accumulator would lose the property with nothing to say so.
///
/// A float leaf is summed as a float, which is the same addition the mono path's own `SUM` performs;
/// a total that leaves the finite range is refused by [`super::refuse_non_finite`].
fn leaf_sum(column: Expr, kind: LeafKind) -> Expr {
    match kind {
        LeafKind::Exact { scale } => sum(cast(column, DataType::Decimal256(EXACT_SUM_PRECISION, scale))),
        LeafKind::Float => sum(column),
    }
}
