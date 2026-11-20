//! The cut: where a source's sub-plan becomes a leaf planned over the rows its leg answers, and which
//! sub-plans may be cut at all.

use std::sync::{Arc, OnceLock};

use datafusion::arrow::array::RecordBatch;
use datafusion::arrow::datatypes::SchemaRef;
use datafusion::catalog::Session;
use datafusion::common::tree_node::{Transformed, TreeNode as _, TreeNodeRecursion};
use datafusion::common::{ExprSchema as _, plan_err};
use datafusion::datasource::memory::MemorySourceConfig;
use datafusion::error::Result as Planned;
use datafusion::logical_expr::{Expr, Extension, Filter, LogicalPlan, Projection};
use datafusion::optimizer::{OptimizerConfig, OptimizerRule};
use datafusion::physical_plan::ExecutionPlan;
use datafusion_federation::{FederatedPlanNode, FederationPlanner};
use sutura_domain::warehouse::schema::TypeDialect;

use super::pushdown;

/// Each part's plan beside the rows its leg came back with.
pub(super) type Leaves = Vec<(LogicalPlan, Vec<RecordBatch>)>;

/// Each part's plan beside the rows its leg came back with, set once every leg has run.
#[derive(Debug, Default)]
pub(super) struct Fetched(pub(super) OnceLock<Leaves>);

#[async_trait::async_trait]
impl FederationPlanner for Fetched {
    async fn plan_federation(&self, node: &FederatedPlanNode, _: &dyn Session) -> Planned<Arc<dyn ExecutionPlan>> {
        let Some((_, batches)) = self
            .0
            .get()
            .and_then(|parts| parts.iter().find(|(plan, _)| plan == node.plan()))
        else {
            return plan_err!("a part of the plan was planned before its leg ran");
        };
        let leaf: Arc<dyn ExecutionPlan> =
            MemorySourceConfig::try_new_exec(core::slice::from_ref(batches), Arc::clone(node.plan().schema().inner()), None)?;
        Ok(leaf)
    }
}

/// The rule a source's sub-plan is handed to: it makes the sub-plan a leaf, planned over its leg's rows.
#[derive(Debug)]
pub(super) struct Cutter(pub(super) Arc<Fetched>, pub(super) TypeDialect, pub(super) bool);

pub(super) fn number_text(schema: &SchemaRef) -> bool {
    schema
        .fields()
        .iter()
        .any(|field| field.metadata().get("sutura:unbounded_number").map(String::as_str) == Some("utf8"))
}

/// Passthrough projections preserve the mapped representation. Value-dependent uses cannot run
/// locally as text; inspect inputs because their column can be absent from the output.
pub(super) fn number_text_safe(plan: &LogicalPlan) -> Planned<bool> {
    if let LogicalPlan::Distinct(datafusion::logical_expr::Distinct::All(input)) = plan {
        return Ok(!number_text(input.schema().inner()));
    }
    let mut safe = true;
    plan.apply_expressions(|expression| {
        if matches!(plan, LogicalPlan::Projection(_))
            && (matches!(expression, Expr::Column(_))
                || matches!(expression, Expr::Alias(alias) if matches!(alias.expr.as_ref(), Expr::Column(_))))
        {
            return Ok(TreeNodeRecursion::Continue);
        }
        expression.apply(|part| {
            if let Expr::Column(column) = part {
                for input in plan.inputs() {
                    safe &= input
                        .schema()
                        .field_from_column(column)
                        .or_else(|_| input.schema().field_with_unqualified_name(&column.name))
                        .map_or_else(
                            |_| !number_text(input.schema().inner()),
                            |field| field.metadata().get("sutura:unbounded_number").map(String::as_str) != Some("utf8"),
                        );
                }
            }
            Ok(TreeNodeRecursion::Continue)
        })?;
        Ok(TreeNodeRecursion::Continue)
    })?;
    Ok(safe)
}

impl OptimizerRule for Cutter {
    fn name(&self) -> &'static str {
        "sutura_statement_cut"
    }

    fn rewrite(&self, plan: LogicalPlan, _: &dyn OptimizerConfig) -> Planned<Transformed<LogicalPlan>> {
        let mut already_cut = false;
        plan.apply_with_subqueries(|node| {
            already_cut |= super::part_of(node).is_some();
            Ok(TreeNodeRecursion::Continue)
        })?;
        if already_cut {
            return Ok(Transformed::no(plan));
        }
        let leaf = |plan| {
            let planner: Arc<dyn FederationPlanner> = Arc::<Fetched>::clone(&self.0);
            Transformed::yes(LogicalPlan::Extension(Extension {
                node: Arc::new(FederatedPlanNode::new(plan, planner)),
            }))
        };
        plan.transform_down_with_subqueries(|node| {
            let filtered_scan =
                matches!(&node, LogicalPlan::Filter(filter) if matches!(filter.input.as_ref(), LogicalPlan::TableScan(_)));
            if self.2 && !filtered_scan && !matches!(node, LogicalPlan::TableScan(_)) {
                return Ok(Transformed::no(node));
            }
            let mut safe = true;
            let mut scans = false;
            node.apply_with_subqueries(|input| {
                scans |= matches!(input, LogicalPlan::TableScan(_));
                safe &= number_text_safe(input)?;
                // Keep Cube joins local while the same cut pushes their supported children down.
                if self.1 == TypeDialect::Cube && matches!(input, LogicalPlan::Join(_)) {
                    safe = false;
                }
                Ok(if safe {
                    TreeNodeRecursion::Continue
                } else {
                    TreeNodeRecursion::Stop
                })
            })?;
            if !safe || !scans {
                return Ok(Transformed::no(node));
            }
            if self.2
                && let LogicalPlan::Filter(filter) = &node
            {
                // The policy runs above the leaf, over what Cube answers, so a conjunct that reads no
                // policy column narrows the rows it sees. The whole filter stays above, as a check.
                let Some(pushed) = pushdown::answered_by_cube(&filter.predicate, filter.input.schema()) else {
                    return Ok(Transformed::no(node));
                };
                let scan = LogicalPlan::Filter(Filter::try_new(pushed, Arc::clone(&filter.input))?);
                let columns = scan.schema().columns().into_iter().map(Expr::Column).collect();
                let cut = leaf(LogicalPlan::Projection(Projection::try_new(columns, Arc::new(scan))?));
                let kept = LogicalPlan::Filter(Filter::try_new(filter.predicate.clone(), Arc::new(cut.data))?);
                return Ok(Transformed::new(kept, true, TreeNodeRecursion::Jump));
            }
            let node = if matches!(&node, LogicalPlan::TableScan(_)) {
                let columns = node.schema().columns().into_iter().map(Expr::Column).collect();
                LogicalPlan::Projection(Projection::try_new(columns, Arc::new(node))?)
            } else {
                node
            };
            let cut = leaf(node);
            Ok(Transformed::new(cut.data, true, TreeNodeRecursion::Jump))
        })
    }
}
