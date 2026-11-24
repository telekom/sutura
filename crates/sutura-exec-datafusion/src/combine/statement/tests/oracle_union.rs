//! A UNION branch's own `ORDER BY`/`LIMIT`/`OFFSET` must stay scoped to that branch, not leak onto
//! the whole statement - isolated from `oracle.rs` so neither file crosses the line-count gate.
//! `OracleLegs`/`sorted_rows` are shared with that file rather than duplicated.

use super::oracle::{OracleLegs, sorted_rows};
use super::*;

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
        let sql = super::super::unparsed(&plan, dialect).expect("the union plan unparses");
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
fn a_raw_union_statement_keeps_its_own_order_by_and_limit_in_every_dialect() {
    // End-to-end through `DataFusionCombiner::answer_statement`: DataFusion's own
    // planner/optimizer builds the right branch's row window as a `Limit` over `Sort(fetch)`
    // (the merged shape `combine_limit` folds before rendering), which the hand-built plan in
    // the sibling test above skips - this proves the bug is reachable from a caller's raw
    // statement, not just from a hand-built plan.
    let sql = r#"select "region" from "dim_region" where "sales_area" = 'emea' union all (select "region" from "dim_region" where "sales_area" = 'emea' order by "region" limit 1)"#;
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
            panic!("{dialect:?}: the single-source union must run: {answer:?}");
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
            "{dialect:?}: a branch's own ORDER BY/LIMIT leaked onto the whole UNION: {:?}",
            legs.statements
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
