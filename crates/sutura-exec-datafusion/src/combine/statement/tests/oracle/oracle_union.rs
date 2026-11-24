//! A UNION branch's own `ORDER BY`/`LIMIT`/`OFFSET` must stay scoped to that branch, not leak onto
//! the whole statement - split out of `oracle.rs` (which declares this as a submodule) so neither
//! file crosses the line-count gate. `OracleLegs`/`sorted_rows` are shared with that file rather
//! than duplicated.

use super::super::*;
use super::{OracleLegs, sorted_rows};

#[test]
fn a_union_branch_keeps_its_own_order_by_and_limit_off_its_sibling_in_every_dialect() {
    // Same bug family as `apache/datafusion`'s unparser via `spiceai/spiceai#14737`: a shared
    // `QueryBuilder` let the right branch's own `ORDER BY ... LIMIT 1` write onto the statement
    // instead of staying on that branch, so the generated SQL returned the wrong rows.
    use datafusion::datasource::{MemTable, provider_as_source};
    use datafusion::logical_expr::{LogicalPlanBuilder, col, lit};

    let batch = regions();
    let source = provider_as_source(Arc::new(
        MemTable::try_new(batch.schema(), vec![vec![batch]]).expect("a source table"),
    ));
    let left = LogicalPlanBuilder::scan("dim_region", Arc::clone(&source), None)
        .expect("a left scan")
        .filter(col("sales_area").eq(lit("emea")))
        .expect("a left filter")
        .project([col("region")])
        .expect("a left projection")
        .build()
        .expect("a left plan");
    let right = LogicalPlanBuilder::scan("dim_region", source, None)
        .expect("a right scan")
        .filter(col("sales_area").eq(lit("emea")))
        .expect("a right filter")
        .project([col("region")])
        .expect("a right projection")
        .sort([col("region").sort(true, false)])
        .expect("a right sort")
        .limit(0, Some(1))
        .expect("a right limit over the sort")
        .build()
        .expect("a right plan");
    let plan = LogicalPlanBuilder::from(left)
        .union(right)
        .expect("a union of same-shaped branches")
        .build()
        .expect("a union plan");
    for dialect in [
        TypeDialect::DuckDb,
        TypeDialect::DataFusion,
        TypeDialect::Cube,
        TypeDialect::Postgres,
        TypeDialect::ClickHouse,
        TypeDialect::BigQuery,
        TypeDialect::Oracle,
    ] {
        let sql = super::super::super::unparsed(&plan, dialect).expect("the union plan unparses");
        let mut legs = OracleLegs {
            engines: Engines::new(None),
            statements: Vec::new(),
        };
        let rows = legs
            .run(&named("geo"), &RawStatement::parse(&sql).expect("a source statement"))
            .unwrap_or_else(|cause| panic!("{dialect:?}: {cause}: {sql}"));
        // Both branches filter to the same 2-row "emea" slice; only the right one orders and
        // limits itself to 1. A shared builder leaks that LIMIT 1 onto the whole UNION ALL,
        // collapsing the 3 rows the branches actually produce down to 1.
        assert_eq!(
            rows.rows().len(),
            3,
            "{dialect:?}: the right branch's own ORDER BY/LIMIT leaked onto the union: {sql}"
        );
        assert_eq!(
            sorted_rows(rows),
            sorted_rows(legs.engines.as_one(
                "select region from dim_region where sales_area = 'emea' \
                 union all (select region from dim_region where sales_area = 'emea' order by region limit 1)"
            )),
            "{dialect:?}: {sql}"
        );
    }
}

#[test]
fn a_union_under_a_top_level_limit_runs_in_every_dialect() {
    // `PushDownLimit` pushes a copy of the outer `LIMIT` below each UNION branch's own
    // `Projection`, where it renders as a plain inner `derived_limit` subquery nested inside that
    // branch's own SELECT - a shape this PR's wrapper never sees, since the branch's own query
    // builder never gets a row window written onto it (`has_own_row_window()` stays `false`). It
    // does not remove the original `Limit` node above the `Union`, though - that one keeps writing
    // onto the statement's own (shared) query builder, same as before this PR. This cell pins that
    // the top-level LIMIT still bounds the whole union, not the wrapper mechanism: a mutation that
    // drops the outer write (or skips it specifically for a `Union` input) would let both
    // now-unbounded branches through and sum to more rows than the statement's own LIMIT allows.
    // It is a plain regression cell, not a red-on-base reproduction - it passes on both trees, and
    // is pinned by a killing mutation rather than causality.
    let sql = r#"select "region" from "dim_region" where "sales_area" = 'emea' union all select "region" from "dim_region" where "sales_area" = 'apac' limit 1"#;
    for dialect in [
        TypeDialect::DuckDb,
        TypeDialect::DataFusion,
        TypeDialect::Cube,
        TypeDialect::Postgres,
        TypeDialect::ClickHouse,
        TypeDialect::BigQuery,
        TypeDialect::Oracle,
    ] {
        let tables = [StatementTable::new(
            named("geo"),
            QualifiedTable::parse("dim_region").expect("a fixture table"),
            dialect,
            regions().schema(),
        )];
        let mut legs = OracleLegs {
            engines: Engines::new(None),
            statements: Vec::new(),
        };
        let statement = RawStatement::parse(sql).expect("a bounded fixture query");
        let answer = DataFusionCombiner::new().expect("a combiner builds").answer_statement(
            StatementQuestion::new(
                &statement,
                &tables,
                true,
                UNBOUNDED,
                opened(Duration::ZERO, Duration::from_secs(60)),
            ),
            &mut legs,
        );
        let Some(Ok(rows)) = answer else {
            panic!(
                "{dialect:?}: the top-level-limited union must run: {answer:?}: {:?}",
                legs.statements
            );
        };
        assert_eq!(
            legs.statements.len(),
            1,
            "{dialect:?}: one pushed statement: {:?}",
            legs.statements
        );
        assert_eq!(
            rows.rows().len(),
            1,
            "{dialect:?}: the top-level LIMIT 1 must still bound the whole union: {:?}",
            legs.statements
        );
        // Pin the corrected PushDownLimit mechanism itself, not just its row-count effect:
        // `PushDownLimit` pushes a copy of this top-level LIMIT below each branch, and by the
        // time it reaches each branch's own `Limit` node the optimizer has already eliminated
        // that branch's `Projection` (the scan already returns exactly the needed column), so
        // the branch is not yet `already_projected` and the pushed fetch writes straight onto
        // the branch's own builder - engaging this PR's wrapper for BOTH branches.
        assert!(
            legs.statements[0].contains("derived_union_branch_0") && legs.statements[0].contains("derived_union_branch_1"),
            "{dialect:?}: a top-level LIMIT pushed below each unprojected branch must engage the \
             branch-wrapping mechanism for both branches: {:?}",
            legs.statements
        );
    }
}

#[test]
fn a_leftmost_windowed_union_branch_runs_in_every_dialect() {
    // The wrapper engages for the LEFTMOST branch too, not just a non-leftmost one: this was the
    // shape a synthetic `c0..cN` alias column list broke, since the wrapped leftmost branch's own
    // "region" output then had a renamed source and no longer resolved as "region" to the caller.
    let sql = r#"(select "region" from "dim_region" where "sales_area" = 'emea' order by "region" limit 1) union all select "region" from "dim_region" where "sales_area" = 'apac'"#;
    for dialect in [
        TypeDialect::DuckDb,
        TypeDialect::DataFusion,
        TypeDialect::Cube,
        TypeDialect::Postgres,
        TypeDialect::ClickHouse,
        TypeDialect::BigQuery,
        TypeDialect::Oracle,
    ] {
        let tables = [StatementTable::new(
            named("geo"),
            QualifiedTable::parse("dim_region").expect("a fixture table"),
            dialect,
            regions().schema(),
        )];
        let mut legs = OracleLegs {
            engines: Engines::new(None),
            statements: Vec::new(),
        };
        let statement = RawStatement::parse(sql).expect("a bounded fixture query");
        let answer = DataFusionCombiner::new().expect("a combiner builds").answer_statement(
            StatementQuestion::new(
                &statement,
                &tables,
                true,
                UNBOUNDED,
                opened(Duration::ZERO, Duration::from_secs(60)),
            ),
            &mut legs,
        );
        let Some(Ok(rows)) = answer else {
            panic!(
                "{dialect:?}: the leftmost-windowed union must run: {answer:?}: {:?}",
                legs.statements
            );
        };
        assert_eq!(
            legs.statements.len(),
            1,
            "{dialect:?}: one pushed statement: {:?}",
            legs.statements
        );
        assert_eq!(
            sorted_rows(rows),
            sorted_rows(legs.engines.as_one(sql)),
            "{dialect:?}: the leftmost branch's own ORDER BY/LIMIT leaked onto the whole UNION: {:?}",
            legs.statements
        );
    }
}

#[test]
fn a_windowed_union_branch_wraps_without_a_table_alias_column_list() {
    // Oracle, BigQuery and ClickHouse all refuse a table alias with an explicit column list
    // (`supports_column_alias_in_table_alias() == false`); the wrapper must project the branch's
    // own field names back out of a bare `AS "derived_union_branch_<i>"` instead of renaming them
    // through such a list. `sqlparser`'s own `TableAlias` only ever prints `(...)` after the alias
    // name when its `columns` vec is non-empty, so a wrapped branch's alias must never be followed
    // by `(`.
    let sql = r#"(select "region" from "dim_region" where "sales_area" = 'emea' order by "region" limit 1) union all select "region" from "dim_region" where "sales_area" = 'apac'"#;
    for dialect in [TypeDialect::Oracle, TypeDialect::BigQuery, TypeDialect::ClickHouse] {
        let tables = [StatementTable::new(
            named("geo"),
            QualifiedTable::parse("dim_region").expect("a fixture table"),
            dialect,
            regions().schema(),
        )];
        let mut legs = OracleLegs {
            engines: Engines::new(None),
            statements: Vec::new(),
        };
        let statement = RawStatement::parse(sql).expect("a bounded fixture query");
        let answer = DataFusionCombiner::new().expect("a combiner builds").answer_statement(
            StatementQuestion::new(
                &statement,
                &tables,
                true,
                UNBOUNDED,
                opened(Duration::ZERO, Duration::from_secs(60)),
            ),
            &mut legs,
        );
        assert!(
            matches!(answer, Some(Ok(_))),
            "{dialect:?}: the leftmost-windowed union must run: {answer:?}: {:?}",
            legs.statements
        );
        assert_eq!(
            legs.statements.len(),
            1,
            "{dialect:?}: one pushed statement: {:?}",
            legs.statements
        );
        for captured in &legs.statements {
            assert!(
                captured.contains("derived_union_branch_0"),
                "{dialect:?}: the branch must be wrapped as a derived table: {captured}"
            );
            let after_alias = captured
                .split("derived_union_branch_0")
                .nth(1)
                .expect("just asserted present")
                .trim_start_matches(['"', '`'])
                .trim_start();
            assert!(
                !after_alias.starts_with('('),
                "{dialect:?}: the derived table alias must carry no column list: {captured}"
            );
        }
    }
}

#[test]
fn a_windowed_union_branch_with_duplicate_output_names_refuses_to_unparse() {
    // `union_branch_as_derived_table`'s outer SELECT projects a wrapped branch's own output
    // field names (`input.schema().fields()`) back out as explicit identifiers; two of them
    // colliding would make that projection ambiguous, so the wrapper refuses up front with an
    // internal error instead of emitting it. `field_names` is built from bare `field.name()`,
    // dropping the table qualifier `DFSchema` itself disambiguates by - so the reachable
    // collision is an unprojected CROSS JOIN of two sources that each have a column named
    // "region" (`customers` and `regions`, both already fixtures in this module): `DFSchema`
    // is content with "customers"."region" next to "dim_region"."region", `Projection`'s own
    // uniqueness check never runs because there is no `Projection` node here at all, and
    // `union_branch_as_derived_table`'s bare-name collision guard is the first place that cares.
    use datafusion::datasource::{MemTable, provider_as_source};
    use datafusion::logical_expr::{LogicalPlan, LogicalPlanBuilder, col, lit};

    let customers_batch = customers();
    let customers_source = provider_as_source(Arc::new(
        MemTable::try_new(customers_batch.schema(), vec![vec![customers_batch]]).expect("a customers source table"),
    ));
    let regions_batch = regions();
    let regions_source = provider_as_source(Arc::new(
        MemTable::try_new(regions_batch.schema(), vec![vec![regions_batch]]).expect("a regions source table"),
    ));
    let joined_regions = |alias: &str| -> LogicalPlan {
        LogicalPlanBuilder::scan(format!("customers_{alias}"), Arc::clone(&customers_source), None)
            .expect("a customers scan")
            .filter(col(format!("customers_{alias}.region")).eq(lit("north")))
            .expect("a customers filter")
            .cross_join(
                LogicalPlanBuilder::scan(format!("dim_region_{alias}"), Arc::clone(&regions_source), None)
                    .expect("a regions scan")
                    .build()
                    .expect("a regions plan"),
            )
            .expect("a cross join keeping both same-named region columns")
            .build()
            .expect("a joined plan")
    };
    let left = joined_regions("left");
    let right = LogicalPlanBuilder::from(joined_regions("right"))
        .limit(0, Some(1))
        .expect("a right limit over the unprojected, duplicate-named join")
        .build()
        .expect("a right plan");
    let plan = LogicalPlanBuilder::from(left)
        .union(right)
        .expect("a union of same-shaped branches")
        .build()
        .expect("a union plan");
    for dialect in [
        TypeDialect::DuckDb,
        TypeDialect::DataFusion,
        TypeDialect::Cube,
        TypeDialect::Postgres,
        TypeDialect::ClickHouse,
        TypeDialect::BigQuery,
        TypeDialect::Oracle,
    ] {
        let cause = super::super::super::unparsed(&plan, dialect).expect_err(
            "a windowed branch with duplicate output names must refuse to unparse, not emit \
             an ambiguous projection",
        );
        assert!(
            cause.to_string().contains("duplicate output column names"),
            "{dialect:?}: {cause}"
        );
    }
}
