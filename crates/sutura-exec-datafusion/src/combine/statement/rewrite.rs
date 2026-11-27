//! Rewrite a part's mangled registration leaves ([`resolve::registered_leaf`]) back to their
//! table's real physical reference before unparsing, so the statement sent to a source never
//! carries a sutura-internal qualifier.

use std::sync::Arc;

use datafusion::common::tree_node::{Transformed, TreeNode as _};
use datafusion::common::{DFSchema, TableReference};
use datafusion::error::Result as Planned;
use datafusion::logical_expr::{Expr, LogicalPlan};
use sutura_domain::plan::StatementTable;

use super::resolve;

/// `plan` with every mangled registration leaf rewritten back to its table's real physical
/// reference: the source's own planner has no notion of a `source.schema.table` prefix, nor of
/// this collision's disambiguation.
///
/// A `TableScan`'s own `table_name`/`projected_schema` and the column expressions it and its
/// ancestors qualify by that name are rewritten. Subquery scans are reached too
/// (`transform_up_with_subqueries`); a correlated subquery's own outer-reference columns
/// (`Subquery::outer_ref_columns`) are not - datafusion exposes no column-rewrite that reaches them,
/// so a correlated reference into a colliding table from inside a part stays a known, narrow gap.
pub(super) fn physically_qualified(plan: &LogicalPlan, tables: &[StatementTable]) -> Planned<LogicalPlan> {
    let renames: Vec<(String, TableReference)> = tables
        .iter()
        .map(|table| (resolve::registered_leaf(tables, table), table))
        .filter(|(leaf, table)| leaf.as_str() != table.table().name().as_str())
        .map(|(leaf, table)| (leaf, physical_reference(table)))
        .collect();
    if renames.is_empty() {
        return Ok(plan.clone());
    }
    plan.clone()
        .transform_up_with_subqueries(|node| {
            let node = if let LogicalPlan::TableScan(scan) = &node
                && let Some((_, physical)) = renames
                    .iter()
                    .find(|(mangled, _)| mangled.as_str() == scan.table_name.table())
            {
                let mut rebuilt = scan.clone();
                rebuilt.table_name = physical.clone();
                rebuilt.projected_schema = Arc::new(DFSchema::try_from_qualified_schema(
                    physical.clone(),
                    scan.projected_schema.as_arrow(),
                )?);
                LogicalPlan::TableScan(rebuilt)
            } else {
                node
            };
            rename_expressions(node, &renames)
        })
        .map(|transformed| transformed.data)
}

/// `table`'s own physical path, as the [`TableReference`] the unparser must see instead of its
/// mangled registration.
fn physical_reference(table: &StatementTable) -> TableReference {
    table.table().qualifier().map_or_else(
        || TableReference::bare(table.table().name().as_str()),
        |qualifier| {
            qualifier.project().map_or_else(
                || TableReference::partial(qualifier.dataset().as_str(), table.table().name().as_str()),
                |project| TableReference::full(project.as_str(), qualifier.dataset().as_str(), table.table().name().as_str()),
            )
        },
    )
}

/// `node`'s own direct expressions, with every column qualified by a mangled registration leaf
/// rewritten to that table's real physical reference.
fn rename_expressions(node: LogicalPlan, renames: &[(String, TableReference)]) -> Planned<Transformed<LogicalPlan>> {
    node.map_expressions(|expr| {
        expr.transform_up(|expr| {
            let Expr::Column(mut column) = expr else {
                return Ok(Transformed::no(expr));
            };
            let mapped = column
                .relation
                .as_ref()
                .and_then(|relation| renames.iter().find(|(mangled, _)| mangled.as_str() == relation.table()));
            let Some((_, physical)) = mapped else {
                return Ok(Transformed::no(Expr::Column(column)));
            };
            column.relation = Some(physical.clone());
            Ok(Transformed::yes(Expr::Column(column)))
        })
    })
}
