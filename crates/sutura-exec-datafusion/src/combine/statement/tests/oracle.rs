//! The planner's Oracle leg, executed by a fake source engine; actual Oracle belongs to its venue.

use datafusion::arrow::array::ArrayRef;

use super::*;

#[test]
fn oracle_temporal_constants_use_ansi_literals_without_session_format_casts() {
    use datafusion::common::ScalarValue;
    use datafusion::logical_expr::{LogicalPlanBuilder, lit};

    let plan = LogicalPlanBuilder::empty(true)
        .project(vec![
            lit(ScalarValue::Date32(Some(20_454))).alias("day"),
            lit(ScalarValue::Date64(Some(1_767_225_600_000))).alias("millis"),
            lit(ScalarValue::TimestampMicrosecond(Some(1_767_225_600_123_456), None)).alias("micros"),
        ])
        .expect("literal columns project")
        .build()
        .expect("a literal plan builds");
    let sql = super::super::oracle::unparsed(&plan).expect("the Oracle constants unparse");
    for literal in [
        "DATE '2026-01-01'",
        "TIMESTAMP '2026-01-01 00:00:00'",
        "TIMESTAMP '2026-01-01 00:00:00.123456'",
    ] {
        assert!(sql.contains(literal), "{literal} is missing: {sql}");
    }
    assert!(
        !sql.contains("CAST("),
        "constant temporal values must not depend on NLS: {sql}"
    );
}

/// Normalizes a generated Oracle leg's `FETCH` back to `LIMIT` before running it against the
/// shared fake engine, so an Oracle dialect cell can be executed, not just captured as text.
/// `pub(super)` so a sibling test module can run a self-join cell through a real Oracle leg too.
pub(super) struct OracleLegs {
    pub(super) engines: Engines,
    pub(super) statements: Vec<String>,
}

impl StatementLegs for OracleLegs {
    type Stop = SourceName;

    fn open(&mut self, sources: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
        self.engines.open(sources)
    }

    fn run(&mut self, source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
        if !explicit_source_projection(statement.as_str()) {
            return Err(source.clone());
        }
        if *source == named("geo") {
            self.statements.push(String::from(statement.as_str()));
            eprintln!("source SQL: {}", statement.as_str());
            // ponytail: only the fake normalizes FETCH; the live venue executes Oracle spelling.
            let Some(normalized) = super::oracle_fetch_to_limit(statement.as_str()) else {
                return Err(source.clone());
            };
            let statement = RawStatement::parse(normalized).map_err(|_cause| source.clone())?;
            return self.engines.run(source, &statement);
        }
        self.engines.run(source, statement)
    }
}

#[test]
fn an_oracle_part_preserves_quoted_aliases_and_fetch_limits_alone_and_in_a_federation() {
    let tables = vec![
        StatementTable::new(
            named("crm"),
            QualifiedTable::parse("dim_customer").expect("a fixture table"),
            TypeDialect::DuckDb,
            customers().schema(),
        ),
        StatementTable::new(
            named("geo"),
            QualifiedTable::parse("dim_region").expect("a fixture table"),
            TypeDialect::Oracle,
            regions().schema(),
        ),
    ];
    for sql in [
        "select r.\"region\" from \"dim_region\" r order by r.\"region\" limit 1 offset 1",
        "select r.\"sales_area\", count(*) as customers from dim_customer c join \"dim_region\" r on c.region = r.\"region\" \
         group by r.\"sales_area\" order by r.\"sales_area\"",
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
                true,
                UNBOUNDED,
                opened(Duration::ZERO, Duration::from_secs(60)),
            ),
            &mut legs,
        );
        let Some(Ok(rows)) = answer else {
            panic!("the Oracle part must run: {answer:?}");
        };
        assert_eq!(rows, legs.engines.as_one(sql), "Oracle SQL: {:?}", legs.statements);
        assert_eq!(legs.statements.len(), 1);
        let text = &legs.statements[0];
        assert!(!text.contains("LIMIT"), "{text}");
        assert!(!text.contains("AS \"r\""), "a table alias has no AS: {text}");
        if sql.contains("offset") {
            assert!(text.contains("OFFSET 1 ROWS FETCH FIRST 1 ROWS ONLY"), "{text}");
        } else {
            assert!(
                text.contains("AS \"c0\""),
                "the local combine needs the quoted column label: {text}"
            );
        }
    }
    let mut legs = Engines::new(None);
    let statement = RawStatement::parse("select region from \"dim_region\"").expect("a bounded fixture query");
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
        matches!(answer, Some(Err(StatementStopped::UnknownColumn))),
        "unquoted Oracle columns fold up: {answer:?}"
    );
    assert_eq!(legs.opened.len(), 0);
}

#[test]
fn mapped_numbers_use_numeric_source_semantics_or_refuse_before_opening() {
    let physical = RecordBatch::try_from_iter([
        (
            "region",
            Arc::new(StringArray::from(vec!["e", "d", "c", "b", "a"])) as ArrayRef,
        ),
        (
            "sales_area",
            Arc::new(Int64Array::from(vec![9, 10, 95, 960, 4000])) as ArrayRef,
        ),
    ])
    .expect("the source stores physical numbers");
    let schema = Arc::new(Schema::new(vec![
        Field::new("region", DataType::Utf8, false),
        Field::new("sales_area", DataType::Utf8, false).with_metadata(std::collections::HashMap::from([(
            String::from("sutura:unbounded_number"),
            String::from("utf8"),
        )])),
    ]));
    for dialect in [
        TypeDialect::Oracle,
        TypeDialect::DuckDb,
        TypeDialect::DataFusion,
        TypeDialect::Postgres,
        TypeDialect::ClickHouse,
        TypeDialect::BigQuery,
        TypeDialect::Cube,
    ] {
        let tables = [
            StatementTable::new(
                named("crm"),
                QualifiedTable::parse("dim_customer").expect("table"),
                TypeDialect::DuckDb,
                customers().schema(),
            ),
            StatementTable::new(
                named("geo"),
                QualifiedTable::parse("dim_region").expect("table"),
                dialect,
                Arc::clone(&schema),
            ),
        ];
        for (sql, expected) in [
            (
                r#"select "sales_area" from "dim_region""#,
                &["9", "10", "95", "960", "4000"][..],
            ),
            (r#"select * from "dim_region""#, &["9", "10", "95", "960", "4000"]),
            (r#"select min("sales_area") from "dim_region""#, &["9"]),
            (r#"select max("sales_area") from "dim_region""#, &["4000"]),
            (
                r#"select "sales_area" from "dim_region" order by "sales_area" desc limit 3"#,
                &["4000", "960", "95"],
            ),
            (
                r#"select "sales_area" from "dim_region" where "sales_area" > 9 order by "sales_area""#,
                &["10", "95", "960", "4000"],
            ),
            (
                r#"select * from "dim_region" where "sales_area" > 9 order by "sales_area""#,
                &["10", "95", "960", "4000"],
            ),
            (
                r#"select r.* from "dim_region" r where r."sales_area" > 9 order by r."sales_area""#,
                &["10", "95", "960", "4000"],
            ),
            (
                r#"with r as (select "sales_area" as n from "dim_region") select * from r order by n"#,
                &["9", "10", "95", "960", "4000"],
            ),
            (
                r#"select "sales_area" from "dim_region" where "sales_area" in (9, 4000) order by "sales_area""#,
                &["9", "4000"],
            ),
            (
                r#"with r as (select "sales_area" as n from "dim_region") select max(n) from r"#,
                &["4000"],
            ),
            (
                r#"select "sales_area" as "region", "region" as "sales_area" from "dim_region" order by "dim_region"."sales_area""#,
                &["e", "d", "c", "b", "a"],
            ),
            (
                r#"select min(r."sales_area") from dim_customer c join "dim_region" r on c.region = r."region""#,
                &[],
            ),
            (
                r#"select r."sales_area" from dim_customer c join "dim_region" r on c.region = r."region" order by r."sales_area""#,
                &[],
            ),
            (
                r#"select max(n) from (select "sales_area" as n from "dim_region" union all select cast(customer_id as varchar) as n from dim_customer where false) z"#,
                &[],
            ),
        ] {
            let projected = sql == r#"select "sales_area" from "dim_region""# || sql == r#"select * from "dim_region""#;
            let refused = !projected && (dialect != TypeDialect::Oracle || sql.contains("dim_customer"));
            let mut legs = OracleLegs {
                engines: Engines::new(None),
                statements: Vec::new(),
            };
            drop(
                legs.engines
                    .at
                    .insert(named("geo"), holding(&[("dim_region", physical.clone())])),
            );
            let statement = RawStatement::parse(sql).expect("a bounded query");
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
            if refused {
                assert!(
                    matches!(
                        answer,
                        Some(Err(StatementStopped::UnsupportedSyntax(SqlConstruct::NumberText)))
                    ),
                    "{dialect:?} {sql}: {answer:?}"
                );
                assert!(
                    legs.engines.opened.is_empty(),
                    "a local numeric operation must not open credentials"
                );
                assert!(legs.statements.is_empty(), "a local numeric operation must not run");
            } else {
                let Some(Ok(rows)) = answer else {
                    panic!("{dialect:?} {sql}: {answer:?}");
                };
                let actual: Vec<_> = rows.rows().iter().map(|row| row.last().expect("a value").render()).collect();
                assert_eq!(actual, expected, "{dialect:?} {sql}: {:?}", legs.statements);
                assert_eq!(legs.statements.len(), 1, "one numeric source statement: {sql}");
            }
        }
    }
}

#[test]
fn mapped_number_tables_push_unrelated_operators_and_safe_children_in_every_dialect() {
    let schema = Arc::new(Schema::new(vec![
        Field::new("region", DataType::Utf8, false),
        Field::new("sales_area", DataType::Utf8, false).with_metadata(std::collections::HashMap::from([(
            String::from("sutura:unbounded_number"),
            String::from("utf8"),
        )])),
    ]));
    for dialect in [
        TypeDialect::DuckDb,
        TypeDialect::DataFusion,
        TypeDialect::Cube,
        TypeDialect::Postgres,
        TypeDialect::ClickHouse,
        TypeDialect::BigQuery,
        TypeDialect::Oracle,
    ] {
        let tables = [
            StatementTable::new(
                named("geo"),
                QualifiedTable::parse("dim_region").expect("a fixture table"),
                dialect,
                Arc::clone(&schema),
            ),
            StatementTable::new(
                named("geo"),
                QualifiedTable::parse("dim_customer").expect("a fixture table"),
                dialect,
                customers().schema(),
            ),
        ];
        for (sql, pushed) in [
            (
                r#"select "region" from "dim_region" where "region" = 'north' order by "region" limit 1"#,
                &["WHERE", "ORDER BY", "FETCH FIRST 1 ROWS ONLY"][..],
            ),
            (
                r#"select "region", count(*) as n from "dim_region" where "region" = 'north' group by "region" order by n limit 1"#,
                &["WHERE", "GROUP BY", "ORDER BY", "FETCH FIRST 1 ROWS ONLY"][..],
            ),
            (
                r#"select "dim_region"."region" from "dim_region" join "dim_customer" on "dim_region"."region" = "dim_customer"."region" where "dim_region"."region" = 'north'"#,
                &["JOIN", "WHERE"][..],
            ),
            (
                r#"select "sales_area" from "dim_region" order by "region""#,
                &["ORDER BY"][..],
            ),
            (
                r#"select "sales_area" from "dim_region" where "region" = 'north' order by "sales_area""#,
                &["WHERE"][..],
            ),
        ] {
            let mut legs = OracleLegs {
                engines: Engines::new(None),
                statements: Vec::new(),
            };
            drop(legs.engines.at.insert(
                named("geo"),
                holding(&[("dim_region", regions()), ("dim_customer", customers())]),
            ));
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
            if sql.contains(r#"order by "sales_area""#) && dialect != TypeDialect::Oracle {
                assert!(
                    matches!(
                        answer,
                        Some(Err(StatementStopped::UnsupportedSyntax(SqlConstruct::NumberText)))
                    ),
                    "{dialect:?} {sql}: {answer:?}"
                );
                assert!(legs.engines.opened.is_empty(), "numeric ordering cannot open local legs");
                continue;
            }
            let Some(Ok(rows)) = answer else {
                panic!("the mapped table must run: {answer:?}");
            };
            assert_eq!(rows, legs.engines.as_one(sql), "{sql}: {:?}", legs.statements);
            let local_join = dialect == TypeDialect::Cube && pushed.contains(&"JOIN");
            assert_eq!(legs.statements.len(), if local_join { 2 } else { 1 }, "{dialect:?}: {sql}");
            for part in &legs.statements {
                for operator in pushed {
                    let operator = if *operator == "FETCH FIRST 1 ROWS ONLY" && dialect != TypeDialect::Oracle {
                        "LIMIT 1"
                    } else {
                        operator
                    };
                    if local_join && operator == "JOIN" {
                        assert!(!part.contains(operator), "Cube joins must stay local: {part}");
                    } else {
                        assert!(part.contains(operator), "{dialect:?}: missing {operator}: {part}");
                    }
                }
                if !sql.contains("sales_area") {
                    assert!(!part.contains("sales_area"), "unused mapped column was fetched: {part}");
                }
            }
        }
    }
}

#[test]
fn a_join_preserves_a_fetch_on_its_direct_right_scan_in_every_dialect() {
    use datafusion::datasource::{MemTable, provider_as_source};
    use datafusion::logical_expr::{JoinType, LogicalPlan, LogicalPlanBuilder, col};

    let batch = regions();
    let source = provider_as_source(Arc::new(
        MemTable::try_new(batch.schema(), vec![vec![batch]]).expect("a source table"),
    ));
    let left = LogicalPlanBuilder::scan("dim_region", Arc::clone(&source), None)
        .expect("a left scan")
        .alias("l")
        .expect("a left alias")
        .build()
        .expect("a left plan");
    let right = LogicalPlanBuilder::scan("dim_region", source, None)
        .expect("a right scan")
        .build()
        .expect("a right plan");
    let LogicalPlan::TableScan(mut right) = right else {
        panic!("a table scan");
    };
    right.fetch = Some(1);
    let right = LogicalPlanBuilder::from(LogicalPlan::TableScan(right))
        .alias("r")
        .expect("a right alias")
        .build()
        .expect("an aliased fetched scan");
    let plan = LogicalPlanBuilder::from(left)
        .join_on(right, JoinType::Inner, [col("l.region").eq(col("r.region"))])
        .expect("an equality join")
        .project(vec![
            col("l.region").alias("left_region"),
            col("r.region").alias("right_region"),
        ])
        .expect("a labelled projection")
        .build()
        .expect("a joined source plan");
    for dialect in [
        TypeDialect::DuckDb,
        TypeDialect::DataFusion,
        TypeDialect::Cube,
        TypeDialect::Postgres,
        TypeDialect::ClickHouse,
        TypeDialect::BigQuery,
        TypeDialect::Oracle,
    ] {
        let sql = super::super::unparsed(&plan, dialect).expect("the joined plan unparses");
        let mut legs = OracleLegs {
            engines: Engines::new(None),
            statements: Vec::new(),
        };
        let rows = legs
            .run(&named("geo"), &RawStatement::parse(&sql).expect("a source statement"))
            .unwrap_or_else(|cause| panic!("{dialect:?}: {cause}: {sql}"));
        assert_eq!(rows.rows().len(), 1, "{dialect:?}: the joined scan lost its fetch: {sql}");
        assert_eq!(
            rows,
            legs.engines
                .as_one("select region as left_region, region as right_region from dim_region limit 1"),
            "{dialect:?}: {sql}"
        );
    }
}

#[test]
fn aliased_limits_and_correlated_subqueries_keep_names_in_scope_in_every_dialect() {
    let _logging = tracing::subscriber::set_default(
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_test_writer()
            .finish(),
    );
    for dialect in [
        TypeDialect::DuckDb,
        TypeDialect::DataFusion,
        TypeDialect::Cube,
        TypeDialect::Postgres,
        TypeDialect::ClickHouse,
        TypeDialect::BigQuery,
        TypeDialect::Oracle,
    ] {
        use datafusion::datasource::{MemTable, provider_as_source};
        use datafusion::logical_expr::{LogicalPlan, LogicalPlanBuilder, col, lit};

        let batch = regions();
        let source = provider_as_source(Arc::new(
            MemTable::try_new(batch.schema(), vec![vec![batch]]).expect("a source table"),
        ));
        let scan = LogicalPlanBuilder::scan("dim_region", Arc::clone(&source), Some(vec![0, 1]))
            .expect("a projected scan")
            .build()
            .expect("a scan plan");
        let LogicalPlan::TableScan(mut scan) = scan else {
            panic!("a table scan");
        };
        scan.fetch = Some(100);
        scan.filters.push(col("dim_region.region").like(lit("%")));
        let plan = LogicalPlanBuilder::from(LogicalPlan::TableScan(scan))
            .alias("r")
            .expect("an alias")
            .limit(0, Some(1))
            .expect("a wrapper limit")
            .project(vec![col("r.region"), col("r.sales_area")])
            .expect("an outer projection")
            .build()
            .expect("a bounded source plan");
        let sql = super::super::unparsed(&plan, dialect).expect("the source plan unparses");
        let mut legs = OracleLegs {
            engines: Engines::new(None),
            statements: Vec::new(),
        };
        let rows = legs
            .run(&named("geo"), &RawStatement::parse(&sql).expect("a source statement"))
            .unwrap_or_else(|cause| panic!("{dialect:?}: {cause}: {sql}"));
        assert_eq!(
            rows,
            legs.engines
                .as_one(r#"select "region", "sales_area" from "dim_region" where "region" like '%' limit 1"#),
            "{dialect:?}: {sql}"
        );
        for (inner, outer) in [
            (col("dim_region.region"), col("dim_region.region")),
            (col("dim_region.region").alias("label"), col("label")),
        ] {
            let sorted = LogicalPlanBuilder::scan("dim_region", Arc::clone(&source), None)
                .expect("a scan")
                .project(vec![inner, col("dim_region.sales_area")])
                .expect("an inner projection")
                .sort(vec![col("dim_region.sales_area").sort(true, true)])
                .expect("a sort below the output projection")
                .project(vec![outer.alias("c0")])
                .expect("a labelled output")
                .build()
                .expect("a sorted source plan");
            let sql = super::super::unparsed(&sorted, dialect).expect("the sorted plan unparses");
            let rows = legs
                .run(&named("geo"), &RawStatement::parse(&sql).expect("a source statement"))
                .unwrap_or_else(|cause| panic!("{dialect:?}: {cause}: {sql}"));
            assert_eq!(
                rows,
                legs.engines
                    .as_one(r#"select "region" as c0 from "dim_region" order by "sales_area""#),
                "{dialect:?}: {sql}"
            );
        }
        let tables = [StatementTable::new(
            named("geo"),
            QualifiedTable::parse("dim_region").expect("a fixture table"),
            dialect,
            regions().schema(),
        )];
        for sql in [
            r#"select r."region" from "dim_region" r where r."region" = 'north' and not exists (select 1 from "dim_region" x where x."region" = r."region" and x."sales_area" = 'missing')"#,
            r#"select r."region", r."sales_area" from "dim_region" r where r."region" like '%north%' limit 1"#,
            r#"select r."region", r."sales_area" from "dim_region" r where upper(r."region") like '%NORTH%' limit 1"#,
            r#"select "region", "sales_area" from "dim_region" where "region" like '%north%' limit 1"#,
            r#"select r."region", r."sales_area" from "dim_region" r where r."region" like '%north%'"#,
            r#"select r."region", r."sales_area" from "dim_region" r where r."region" like '%north%' order by r."region" limit 1"#,
            r#"select r."region" from "dim_region" r where r."region" like '%north%' limit 1"#,
            r#"select r."region" from "dim_region" r where exists (select 1 from "dim_region" x where x."region" = r."region" and x."region" = 'north')"#,
            r#"select r."region" from "dim_region" r where r."region" in (select x."region" from "dim_region" x where x."region" = 'north')"#,
            r#"select r."region", r."sales_area" from "dim_region" r where r."region" in (select x."region" from "dim_region" x where x."region" = 'north') limit 1"#,
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
            // Second review round on #2047 (HIGH): the mixed-schema fallback must not
            // collapse two differently-qualified SAME-NAMED columns onto one output
            // name - `r."region"` and `x."region"` projected together must stay two
            // distinct, correctly-paired columns of the derived table, not one
            // ambiguous `region` an engine could silently resolve to the wrong side.
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
            let parts = if dialect == TypeDialect::Cube
                && (sql.contains("exists (") || sql.contains(" in (select") || sql.contains("join"))
            {
                2
            } else {
                1
            };
            assert_eq!(legs.statements.len(), parts, "{dialect:?}: {sql}");
        }
    }
}

#[test]
fn native_numeric_duplicate_outputs_restore_names_and_distinct_positions() {
    let physical = RecordBatch::try_from_iter([("sales_area", Arc::new(Int64Array::from(vec![9, 10, 20])) as ArrayRef)])
        .expect("physical source numbers");
    let schema = Schema::new(vec![Field::new("sales_area", DataType::Utf8, false).with_metadata(
        std::collections::HashMap::from([(String::from("sutura:unbounded_number"), String::from("utf8"))]),
    )]);
    let tables = [StatementTable::new(
        named("geo"),
        QualifiedTable::parse("dim_region").expect("a table"),
        TypeDialect::Oracle,
        Arc::new(schema),
    )];
    for predicate in [r#"a."sales_area" = 9"#, r#"a."sales_area" = 1000"#] {
        let statement = RawStatement::parse(format!(
            r#"SELECT a."sales_area", b."sales_area" FROM "dim_region" a JOIN "dim_region" b ON a."sales_area" < b."sales_area" WHERE b."sales_area" = 10 AND {predicate}"#
        )).expect("an admitted native numeric query");
        let mut legs = OracleLegs {
            engines: Engines::new(None),
            statements: Vec::new(),
        };
        drop(
            legs.engines
                .at
                .insert(named("geo"), holding(&[("dim_region", physical.clone())])),
        );
        let answer = DataFusionCombiner::new().expect("a combiner").answer_statement(
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
            panic!(
                "native duplicate outputs must run: {answer:?}; opened={:?}; ran={:?}",
                legs.engines.opened, legs.engines.ran
            )
        };
        assert_eq!(rows.columns(), ["sales_area", "sales_area"]);
        assert_eq!(
            source_labels(legs.statements.first().expect("a native source statement")),
            ["c0", "c1"]
        );
        let expected = if predicate.contains("1000") {
            vec![]
        } else {
            vec![vec![
                sutura_domain::warehouse::Value::Integer(9),
                sutura_domain::warehouse::Value::Integer(10),
            ]]
        };
        assert_eq!(rows.rows(), expected);
        assert_eq!(legs.engines.opened, [BTreeSet::from([named("geo")])]);
        assert_eq!(legs.statements.len(), 1);
    }
}
