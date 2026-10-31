//! Several metrics on one data system, as one logical plan: each arm's grouped plan, a typed null
//! under every other arm's measure label, stacked with a union and grouped again on the keys and
//! the bucket, each measure its `MAX` (`docs/adr/20261031173920-several-metrics-on-one-data-system.md`).
//!
//! The SQL path's shape (`sutura_sql::generate_stacked`), built as nodes, so a `top` and the row
//! limit apply once, after the arms join.

use datafusion::common::{Column, ScalarValue};
use datafusion::functions_aggregate::expr_fn::max;
use datafusion::logical_expr::{Expr, LogicalPlan, LogicalPlanBuilder, lit};
use sutura_domain::plan::StackedPlan;

use crate::DataFusionError;
use crate::collect::outputs;
use crate::top;

/// The stacked plan over `first` and `rest`, each arm's grouped plan projected to its own result
/// labels, in arm order.
pub(crate) fn logical(plan: &StackedPlan, first: LogicalPlan, rest: Vec<LogicalPlan>) -> Result<LogicalPlan, DataFusionError> {
    let build = |cause| DataFusionError::Build { cause };
    let labels = plan.result_labels();
    let group_count = plan.arms().first().keys().len().saturating_add(1);
    let grouped: Vec<Expr> = labels.iter().take(group_count).map(|label| column(label)).collect();
    // Each measure's type, off the arm that projects it, so the null under its label in every other
    // arm has that type and the union has one type per column.
    let measures: Vec<(String, ScalarValue)> = core::iter::once(&first)
        .chain(&rest)
        .flat_map(|arm| arm.schema().fields().iter().skip(group_count))
        .map(|field| Ok((field.name().clone(), ScalarValue::try_from(field.data_type())?)))
        .collect::<Result<_, datafusion::error::DataFusionError>>()
        .map_err(build)?;
    let branch = |arm: LogicalPlan| {
        let own: Vec<String> = arm.schema().fields().iter().map(|field| field.name().clone()).collect();
        let mut projection = grouped.clone();
        for (label, null) in &measures {
            projection.push(if own.iter().any(|name| name == label) {
                column(label)
            } else {
                lit(null.clone()).alias(label)
            });
        }
        LogicalPlanBuilder::from(arm).project(projection).map_err(build)
    };
    let mut stacked = branch(first)?;
    for arm in rest {
        stacked = stacked.union(branch(arm)?.build().map_err(build)?).map_err(build)?;
    }
    let aggregates = measures.iter().map(|(label, _)| max(column(label)).alias(label));
    let builder = stacked.aggregate(grouped, aggregates).map_err(build)?;
    let (projection, tiebreak) = outputs(builder.schema(), &labels, group_count)?;
    let (sorts, limit) = top::sort_and_limit(plan.top(), &labels, group_count, tiebreak, plan.max_rows());
    builder
        .project(projection)
        .and_then(|projected| projected.sort(sorts))
        .and_then(|sorted| sorted.limit(0, Some(limit)))
        .and_then(LogicalPlanBuilder::build)
        .map_err(build)
}

fn column(label: &str) -> Expr {
    Expr::Column(Column::new_unqualified(label))
}

#[cfg(test)]
mod tests;
