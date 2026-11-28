//! `physically_qualified` is reached through the full combiner only via plain `Expr::Column`
//! references: every SQL shape this combiner accepts that produces a correlated subquery -
//! `EXISTS`/`IN`/a scalar subquery, with or without an equi-predicate - is decorrelated by
//! `DataFusion`'s own optimizer into a join of ordinary columns before a part's plan is cut and
//! rewritten, so no reachable SQL text leaves a surviving `Expr::OuterReferenceColumn` for the
//! rewrite to see. This unit-tests the rewrite directly instead of through SQL, building the one
//! shape the planner itself never produces here but that `rename_expressions` must still handle
//! defensively, the same way `tests::oracle` calls straight into the private `oracle` module.

use datafusion::common::{Column, TableReference};
use datafusion::functions_aggregate::expr_fn::count;
use datafusion::logical_expr::{Expr, Filter, LogicalPlan, LogicalPlanBuilder, col, lit, table_scan};

use super::super::{resolve, rewrite};
use super::*;

fn colliding_tables() -> Vec<StatementTable> {
    let schema = Arc::new(Schema::new(vec![Field::new("shipment_id", DataType::Int64, false)]));
    vec![
        StatementTable::new(
            named("crm"),
            QualifiedTable::parse("reporting.shipments").expect("a dataset-qualified table"),
            TypeDialect::DuckDb,
            Arc::clone(&schema),
        ),
        StatementTable::new(
            named("geo"),
            QualifiedTable::parse("reporting.shipments").expect("a dataset-qualified table"),
            TypeDialect::Postgres,
            schema,
        ),
    ]
}

/// A correlated subquery's own outer-reference column, naming a colliding table's mangled
/// registration leaf, must rewrite to that table's physical path exactly as a plain qualified
/// column does - `rename_expressions` used to match only `Expr::Column`, leaving
/// `Expr::OuterReferenceColumn` free to carry the internal leaf straight into a leg's own SQL text.
#[test]
fn an_outer_reference_column_into_a_colliding_table_rewrites_to_the_physical_path() {
    let tables = colliding_tables();
    let mangled = resolve::registered_leaf(&tables, &tables[0]);
    assert_eq!(
        mangled, "crm\u{1}shipments",
        "crm's own reporting.shipments collides with geo's"
    );

    let predicate = Expr::OuterReferenceColumn(
        Arc::new(Field::new("shipment_id", DataType::Int64, false)),
        Column::new(Some(TableReference::partial("reporting", mangled.as_str())), "shipment_id"),
    )
    .gt(lit(0_i64));
    let input = LogicalPlanBuilder::empty(false).build().expect("an empty relation builds");
    let plan =
        LogicalPlan::Filter(Filter::try_new(predicate, input.into()).expect("a boolean predicate over an outer reference"));

    let rewritten = rewrite::physically_qualified(&plan, &tables).expect("the rewrite succeeds");
    let LogicalPlan::Filter(filter) = rewritten else {
        panic!("{rewritten:?}");
    };
    let Expr::BinaryExpr(binary) = filter.predicate else {
        panic!("{:?}", filter.predicate);
    };
    let Expr::OuterReferenceColumn(_, column) = *binary.left else {
        panic!("{:?}", binary.left);
    };
    assert_eq!(
        column.relation,
        Some(TableReference::partial("reporting", "shipments")),
        "the mangled registration leaf must resolve to crm's own physical path, not stay internal"
    );
}

/// An unaliased computed column's own schema field name is derived from the plan's `Display`, not
/// kept as separate, rewritable relation metadata - `map_expressions` rewrites an `Aggregate`'s own
/// expressions but never recomputes its *stored* schema, so a caller reading `plan.schema()` after
/// only `rename_expressions` still sees the pre-rewrite, mangled field name even though every
/// `Expr::Column` inside it was already fixed. `physically_qualified` must recompute it.
#[test]
fn an_unaliased_aggregate_over_a_colliding_table_recomputes_its_schema_field_name() {
    let tables = colliding_tables();
    let mangled = resolve::registered_leaf(&tables, &tables[0]);
    assert_eq!(
        mangled, "crm\u{1}shipments",
        "crm's own reporting.shipments collides with geo's"
    );

    let schema = Schema::new(vec![Field::new("shipment_id", DataType::Int64, false)]);
    let scan = table_scan(Some(TableReference::partial("reporting", mangled.as_str())), &schema, None)
        .expect("a table scan over the mangled registration builds");
    let plan = scan
        .aggregate(
            Vec::<Expr>::new(),
            vec![count(col(Column::new(
                Some(TableReference::partial("reporting", mangled.as_str())),
                "shipment_id",
            )))],
        )
        .and_then(LogicalPlanBuilder::build)
        .expect("an unaliased count over the scan aggregates");
    let before = plan.schema().field(0).name().clone();
    assert!(
        before.contains('\u{1}'),
        "the fixture itself must carry the mangled leaf pre-rewrite, or this test proves nothing: {before}"
    );

    let rewritten = rewrite::physically_qualified(&plan, &tables).expect("the rewrite succeeds");
    let after = rewritten.schema().field(0).name().clone();
    assert!(
        !after.contains('\u{1}'),
        "an unaliased computed column's schema field name leaked sutura's internal collision marker: {after}"
    );
    assert_eq!(after, "count(reporting.shipments.shipment_id)");
}
