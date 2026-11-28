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

/// Every mangled registration leaf this question's tables carry, beside the physical reference it
/// stands for - the one list both [`physically_qualified`] and [`unmangled_name`] rewrite from.
pub(super) fn renames_of(tables: &[StatementTable]) -> Vec<(String, TableReference)> {
    tables
        .iter()
        .map(|table| (resolve::registered_leaf(tables, table), table))
        .filter(|(leaf, table)| leaf.as_str() != table.table().name().as_str())
        .map(|(leaf, table)| (leaf, physical_reference(table)))
        .collect()
}

/// `plan` with every mangled registration leaf rewritten back to its table's real physical
/// reference: the source's own planner has no notion of a `source.schema.table` prefix, nor of
/// this collision's disambiguation.
///
/// A `TableScan`'s own `table_name`/`projected_schema` and the column expressions it and its
/// ancestors qualify by that name are rewritten. Subquery scans are reached too
/// (`transform_up_with_subqueries`), and so is a correlated subquery's own outer-reference column
/// (`Expr::OuterReferenceColumn`), rewritten the same way as a plain `Expr::Column`.
pub(super) fn physically_qualified(plan: &LogicalPlan, tables: &[StatementTable]) -> Planned<LogicalPlan> {
    let renames = renames_of(tables);
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
            // `map_expressions` (inside `rename_expressions`) rewrites a node's own expressions but
            // never its STORED schema - a `Projection`/`Aggregate`/`Window`/`Join` keeps the pre-rename
            // qualifier there otherwise. Worse than stale metadata: an unaliased computed column's
            // schema field NAME is derived from the plan's `Display`, so a parent node that refers to
            // it by that name (not by `Expr::Column` relation/name, but as the literal text DataFusion
            // generated) still carries the mangled leaf baked into a string until the schema is
            // recomputed. `recompute_schema` is a no-op for a `TableScan` (already rebuilt above) and
            // for any node whose schema does not depend on its own expressions; for one that does, it
            // is why `rename_expressions` below must also rewrite a column's own NAME, not only its
            // `relation` - otherwise this recompute changes the child's display name out from under a
            // parent's still-mangled textual reference to it.
            rename_expressions(node, &renames)?.map_data(LogicalPlan::recompute_schema)
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
/// rewritten to that table's real physical reference. A correlated subquery's outer-reference
/// column is rewritten the same way, carrying its field reference back through unchanged.
///
/// A column's own NAME is rewritten too, not only its `relation`: an unqualified reference to a
/// child `Aggregate`'s auto-generated display column (a parent `Projection`/`Filter` naming
/// `count(shipment_id)` by that literal text, for instance) carries the mangled leaf directly in
/// the name string rather than as separate relation metadata, and `resolve`'s own column binding
/// matches that text. Leaving it unrewritten here means `recompute_schema` changes the child's
/// name out from under a parent reference that still spells the old, mangled one -
/// `Projection::try_new`/`Filter::try_new` then refuse the plan as an unresolved column.
fn rename_expressions(node: LogicalPlan, renames: &[(String, TableReference)]) -> Planned<Transformed<LogicalPlan>> {
    node.map_expressions(|expr| {
        expr.transform_up(|expr| {
            let (field, mut column) = match expr {
                Expr::Column(column) => (None, column),
                Expr::OuterReferenceColumn(field, column) => (Some(field), column),
                expr => return Ok(Transformed::no(expr)),
            };
            let rebuild = |column| match field.clone() {
                Some(field) => Expr::OuterReferenceColumn(field, column),
                None => Expr::Column(column),
            };
            let relation_changed = if let Some((_, physical)) = column
                .relation
                .as_ref()
                .and_then(|relation| renames.iter().find(|(mangled, _)| mangled.as_str() == relation.table()))
            {
                column.relation = Some(physical.clone());
                true
            } else {
                false
            };
            let unmangled = unmangled_name(&column.name, renames);
            let name_changed = unmangled != column.name;
            if name_changed {
                column.name = unmangled;
            }
            if relation_changed || name_changed {
                Ok(Transformed::yes(rebuild(column)))
            } else {
                Ok(Transformed::no(rebuild(column)))
            }
        })
    })
}

/// `name` with every mangled registration leaf it carries replaced by the physical table name it
/// mangles. An unaliased computed column (an aggregate, a cast, ...) is named from the un-rewritten
/// plan's own `Display`, which bakes the qualifier straight into the field name's text rather than
/// keeping it as separate, rewritable relation metadata - a parent node `physically_qualified`
/// cannot reach once a federation leaf (an [`super::part_of`] extension) has already turned the
/// subtree holding the mangled scan opaque. The schema qualifier ahead of the leaf (a dataset, a
/// project) is left as it already reads: only the leaf token itself is ever mangled.
pub(super) fn unmangled_name(name: &str, renames: &[(String, TableReference)]) -> String {
    renames.iter().fold(name.to_owned(), |name, (mangled, physical)| {
        name.replace(mangled.as_str(), physical.table())
    })
}
