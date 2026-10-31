//! Several metrics on one data system, as one statement:
//! `docs/adr/20261031173920-several-metrics-on-one-data-system.md`.
//!
//! Each arm is the grouped `SELECT` [`super::generate()`] would render for it, less its order and
//! limit, projecting a null under every other arm's measure label. The arms are stacked with
//! `UNION ALL` and grouped again on the keys and the bucket, and each measure is its `MAX`: an arm
//! has at most one row per group, so `MAX` is that row's value, or null where the arm has none.

use polyglot_sql::builder::{self, Expr};
use polyglot_sql::expressions::{Expression, Union};
use sutura_domain::plan::{PlannedMeasure, QueryPlan, StackedPlan};
use sutura_domain::warehouse::ParamValue;

use super::{GenerateError, Guard, Head, aliased, measure_expression, ordered_nulls_last, top};
use crate::GeneratedQuery;
use crate::dialect::{Dialect, PlaceholderStyle};

/// The name the stacked arms are read under.
const STACKED: &str = "stacked";

/// Renders a stacked plan as one statement, paired with its parameters in arm order.
///
/// # Errors
///
/// [`GenerateError`] as [`super::generate()`] returns it for any arm, or for the whole statement.
pub fn generate_stacked(plan: &StackedPlan, dialect: Dialect) -> Result<GeneratedQuery, GenerateError> {
    let labels: Vec<&str> = plan
        .arms()
        .iter()
        .flat_map(|arm| arm.measures().iter().map(PlannedMeasure::label))
        .collect();
    let mut emitted: Vec<ParamValue> = Vec::new();
    let mut shift = 0_usize;
    let mut branch = |arm: &QueryPlan| -> Result<Expression, GenerateError> {
        let Head {
            mut projection,
            grouping,
            ..
        } = super::head(arm, dialect)?;
        for &label in &labels {
            let expr = arm
                .measures()
                .iter()
                .find(|m| m.label() == label)
                .map_or_else(builder::null, |measure| {
                    let guard = Guard::build(dialect, measure.guard(), arm.params());
                    measure_expression(measure.measure(), guard.as_ref(), dialect, &mut emitted)
                });
            projection.push(aliased(expr, label)?);
        }
        let grouped = super::grouped(arm, dialect, projection, grouping, &mut emitted)?.build();
        let moved = shifted(grouped, shift, dialect)?;
        shift += arm.params().len();
        Ok(moved)
    };
    let mut stacked = branch(plan.arms().first())?;
    for arm in plan.arms().iter().skip(1) {
        stacked = union_all(stacked, branch(arm)?);
    }

    let first = plan.arms().first();
    let mut grouping: Vec<Expr> = first.keys().iter().map(|key| builder::col(key.label())).collect();
    let bucket = builder::col(first.bucket().label());
    grouping.push(bucket.clone());
    let mut projection = grouping.clone();
    // Every label overwrites it, so it ends as the last metric's measure, which is what the
    // in-process merge ranks a `top` by metric by.
    let mut last = builder::null();
    for &label in &labels {
        last = builder::max_(builder::col(label));
        projection.push(aliased(last.clone(), label)?);
    }
    let tiebreak: Vec<Expr> = grouping.iter().cloned().map(ordered_nulls_last).collect();
    let (ordering, limit) = match plan.top() {
        Some(requested) => (
            top::ordering(requested, last, bucket, tiebreak),
            usize::try_from(requested.n().get()).unwrap_or(usize::MAX),
        ),
        None => (tiebreak, usize::try_from(plan.row_limit()).unwrap_or(usize::MAX)),
    };
    let ast = builder::select(projection)
        .from_expr(builder::subquery_expr(stacked, STACKED))
        .group_by(grouping)
        .order_by(ordering)
        .limit(limit)
        .build();
    let params = match dialect.placeholder_style() {
        PlaceholderStyle::Question | PlaceholderStyle::Colon => emitted,
        PlaceholderStyle::Numbered => plan.params().to_vec(),
    };
    let sql = super::render(&ast, dialect)?;
    super::assert_placeholder_count(&ast, dialect, &params)?;
    Ok(GeneratedQuery::new(plan.source().clone(), sql, params))
}

/// `left UNION ALL right`.
fn union_all(left: Expression, right: Expression) -> Expression {
    Expression::Union(Box::new(Union {
        left,
        right,
        all: true,
        distinct: false,
        with: None,
        order_by: None,
        limit: None,
        offset: None,
        distribute_by: None,
        sort_by: None,
        cluster_by: None,
        by_name: false,
        side: None,
        kind: None,
        corresponding: false,
        strict: false,
        on_columns: Vec::new(),
    }))
}

/// `branch` with every positional placeholder moved past the `shift` parameters of the arms before
/// it, so a numbered placeholder names its value in the whole plan's parameters.
///
/// [`super::assert_placeholder_count`] then holds the whole statement's placeholders to them.
fn shifted(branch: Expression, shift: usize, dialect: Dialect) -> Result<Expression, GenerateError> {
    let by = u32::try_from(shift).unwrap_or(u32::MAX);
    if by == 0 {
        return Ok(branch);
    }
    polyglot_sql::traversal::transform_all(branch, &|node| {
        Ok(match node {
            Expression::Parameter(mut parameter) => {
                parameter.index = parameter.index.map(|index| index.saturating_add(by));
                Expression::Parameter(parameter)
            }
            other => other,
        })
    })
    .map_err(|cause| GenerateError::Render { dialect, cause })
}

#[cfg(test)]
mod tests;
