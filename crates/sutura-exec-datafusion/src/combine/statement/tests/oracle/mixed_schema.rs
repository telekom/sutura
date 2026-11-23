//! A mixed-schema wrapped plan - more than one qualifier survives into the derived table's own
//! schema - exercised across every dialect, against the shared fake Oracle-normalizing leg. Kept
//! separate from `oracle.rs`'s own parameterized cell so that file's pre-existing lines stay
//! untouched (`git diff origin/main -- .../tests/oracle.rs` is empty): these shapes and their
//! sorted-row comparison are additions, not edits to a cell the base tree already ran.

use super::*;

#[test]
fn mixed_qualifier_derived_tables_still_answer_every_shape_in_every_dialect() {
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
        for sql in [
            // github.com/telekom/sutura#2041: count(DISTINCT) expands into an extra GROUP BY
            // level, and the outer level's qualifier must resolve through the inner one's alias.
            r#"select "region", count(distinct "sales_area") as n from "dim_region" where "region" <> 'missing' group by "region" limit 50"#,
            // #2041: a repeated expression inside OR is hoisted into a common-subexpression
            // wrapper; the wrapper's own output and every sibling column must stay in scope above it.
            r#"select "region", "sales_area" from "dim_region" where upper("region") like '%no%' or upper("region") like '%st%' limit 100"#,
            // #1972's own shape: a self-join makes the wrapped plan's schema MIXED (qualifiers
            // "r" and "x" both present), so the single-uniform-qualifier branch can't apply and
            // the parent's own qualified `r."region"` must be rewritten onto the wrapper's alias.
            r#"select r."region", count(distinct x."sales_area") as n from "dim_region" r join "dim_region" x on r."region" = x."region" where r."region" <> 'missing' group by r."region" limit 50"#,
            r#"select r."region", r."sales_area" from "dim_region" r join "dim_region" x on r."region" = x."region" where upper(r."region") like '%no%' or upper(r."region") like '%st%' limit 100"#,
            // Second review round on #2047 (HIGH): the mixed-schema fallback must not collapse
            // two differently-qualified SAME-NAMED columns onto one output name - `r."region"`
            // and `x."region"` projected together must stay two distinct, correctly-paired
            // columns of the derived table, not one ambiguous `region` an engine could silently
            // resolve to the wrong side.
            r#"select r."region" as r_region, x."region" as x_region from "dim_region" r join "dim_region" x on r."region" <> x."region" order by r_region, x_region limit 5"#,
        ] {
            let mut legs = OracleLegs {
                engines: Engines::new(None),
                statements: Vec::new(),
            };
            let statement = RawStatement::parse(sql).expect("a bounded fixture query");
            let answer = DataFusionCombiner::new().expect("a combiner builds").answer_statement(
                StatementQuestion::new(
                    &statement,
                    &tables,
                    false,
                    UNBOUNDED,
                    opened(Duration::ZERO, Duration::from_secs(60)),
                ),
                &mut legs,
            );
            let Some(Ok(rows)) = answer else {
                panic!("{dialect:?}: {sql}: {answer:?}; source SQL: {:?}", legs.statements);
            };
            // A `sql` with no `order by` of its own makes no promise about row order - a bare
            // `GROUP BY`'s row order is the engine's own hash-aggregation order, not a property of
            // the plan this test holds the unparser to - so compare as a sorted multiset there, or
            // this would flake on row order rather than on anything this fix changes.
            if sql.contains("order by") {
                assert_eq!(
                    rows,
                    legs.engines.as_one(sql),
                    "{dialect:?}: {sql}; source SQL: {:?}",
                    legs.statements
                );
            } else {
                let sort_by_text = |raw: &RawRows| {
                    let mut rows: Vec<String> = raw.rows().iter().map(|row| format!("{row:?}")).collect();
                    rows.sort();
                    (raw.columns().to_vec(), rows)
                };
                assert_eq!(
                    sort_by_text(&rows),
                    sort_by_text(&legs.engines.as_one(sql)),
                    "{dialect:?}: {sql}; source SQL: {:?}",
                    legs.statements
                );
            }
            // Cube cannot push a join: a self-join over "dim_region" reaches it as two legs, one
            // per relation, same as an EXISTS/IN subquery's own separate leg.
            let parts = if dialect == TypeDialect::Cube && sql.contains("join") {
                2
            } else {
                1
            };
            assert_eq!(legs.statements.len(), parts, "{dialect:?}: {sql}");
        }
    }
}
