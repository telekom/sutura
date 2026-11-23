//! The planner's Oracle leg, executed by a fake source engine; actual Oracle belongs to its venue.

use std::ops::ControlFlow;

use datafusion::arrow::array::ArrayRef;

use datafusion::sql::sqlparser::ast::{LimitClause, Query, VisitMut as _, VisitorMut};
use datafusion::sql::sqlparser::dialect::GenericDialect;
use datafusion::sql::sqlparser::parser::Parser;

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

struct OracleLegs {
    engines: Engines,
    statements: Vec<String>,
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
            let mut parsed = Parser::parse_sql(&GenericDialect, statement.as_str()).map_err(|_cause| source.clone())?;
            let [sql] = parsed.as_mut_slice() else {
                return Err(source.clone());
            };
            if sql.visit(&mut OracleLimits).is_break() {
                return Err(source.clone());
            }
            let statement = RawStatement::parse(sql.to_string()).map_err(|_cause| source.clone())?;
            return self.engines.run(source, &statement);
        }
        self.engines.run(source, statement)
    }
}

struct OracleLimits;

impl VisitorMut for OracleLimits {
    type Break = ();

    fn post_visit_query(&mut self, query: &mut Query) -> ControlFlow<Self::Break> {
        let Some(fetch) = query.fetch.take() else {
            return ControlFlow::Continue(());
        };
        if fetch.with_ties || fetch.percent {
            return ControlFlow::Break(());
        }
        let Some(quantity) = fetch.quantity else {
            return ControlFlow::Break(());
        };
        let offset = match query.limit_clause.take() {
            None => None,
            Some(LimitClause::LimitOffset {
                limit: None,
                offset,
                limit_by,
            }) if limit_by.is_empty() => offset,
            Some(_) => return ControlFlow::Break(()),
        };
        query.limit_clause = Some(LimitClause::LimitOffset {
            limit: Some(quantity),
            offset,
            limit_by: Vec::new(),
        });
        ControlFlow::Continue(())
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
            assert_eq!(
                rows,
                legs.engines.as_one(sql),
                "{dialect:?}: {sql}; source SQL: {:?}",
                legs.statements
            );
            let parts = if dialect == TypeDialect::Cube && (sql.contains("exists (") || sql.contains(" in (select")) {
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

/// #2038: a native Oracle dictionary reports UPPERCASE table and column names. A text-policy CLOB
/// with a bare `column: SCRIPT` raw-prefilter rule must still have its raw predicate pushed down for
/// every client spelling Oracle itself accepts, not only the one the dictionary happens to echo back.
#[test]
fn raw_prefilter_reaches_the_oracle_source_for_every_case_spelling_of_a_native_dictionary_clob() {
    let policy = serde_json::json!({
        "max_text_bytes": 128,
        "redact": [{"pattern": "secret_[^ ]+", "replace": "[masked]"}],
        "raw_prefilter": {"acknowledged_because": "#2038 reproduction: a native dictionary CLOB, bare column rule"},
    });
    let metadata = std::collections::HashMap::from([
        (String::from("sutura:text_policy"), policy.to_string()),
        (String::from("sutura:oracle_lob"), String::from("clob")),
    ]);
    let schema = Arc::new(Schema::new(vec![
        Field::new("SCRIPT", DataType::Utf8, true).with_metadata(metadata),
        Field::new("OBJECT_NAME", DataType::Utf8, true),
    ]));
    let tables = vec![StatementTable::new(
        named("geo"),
        QualifiedTable::parse("DOCS").expect("a fixture table"),
        TypeDialect::Oracle,
        Arc::clone(&schema),
    )];
    let physical = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(datafusion::arrow::array::StringArray::from(vec![Some("has_mrc")])),
            Arc::new(datafusion::arrow::array::StringArray::from(vec![Some("obj")])),
        ],
    )
    .expect("the fixture row is rectangular");
    for sql in [
        "SELECT count(OBJECT_NAME) FROM DOCS WHERE SCRIPT LIKE '%MRC%'",
        "SELECT count(OBJECT_NAME) FROM DOCS WHERE UPPER(SCRIPT) LIKE '%MRC%'",
        "SELECT count(OBJECT_NAME) FROM DOCS WHERE SCRIPT LIKE '%MRC%' LIMIT 5",
        "select count(object_name) from docs where script like '%mrc%'",
    ] {
        let mut legs = OracleLegs {
            engines: Engines::new(None),
            statements: Vec::new(),
        };
        // The fake engine's own session normalizes a quoted reference to lowercase before lookup
        // (DataFusion's default `enable_ident_normalization`), so the registered name must be
        // lowercase even though the dictionary and the rendered Oracle statement stay uppercase.
        drop(legs.engines.at.insert(named("geo"), holding(&[("docs", physical.clone())])));
        let statement = RawStatement::parse(sql).expect("an admitted native dictionary query");
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
        let Some(Ok(_rows)) = answer else {
            panic!("#2038's statement must plan and run: {sql} -> {answer:?}");
        };
        let reached = legs.statements.first().expect("one native Oracle source statement");
        assert!(
            reached.to_uppercase().contains("LIKE '%MRC%'") && !reached.to_uppercase().contains("REGEXP_REPLACE"),
            "the raw condition must reach the source before any masking is applied: {sql} -> {reached}"
        );
    }
}
