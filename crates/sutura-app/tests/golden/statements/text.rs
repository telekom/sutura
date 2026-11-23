//! The same protected physical text at every native engine and every SQL renderer.

use super::*;

fn protected(path: QualifiedTable, dialect: TypeDialect, bytes: u32) -> StatementTable {
    protected_replacing(path, dialect, bytes, "masked")
}

fn protected_replacing(path: QualifiedTable, dialect: TypeDialect, bytes: u32, replace: &str) -> StatementTable {
    let policy = serde_json::json!({
        "max_text_bytes": bytes,
        "redact": [{"pattern": "(?i)north(_east)?", "replace": replace}]
    })
    .to_string();
    StatementTable::new(
        source(),
        path,
        dialect,
        Arc::new(Schema::new(vec![
            Field::new("region", DataType::Utf8, true),
            Field::new("sales_area", DataType::Utf8, true).with_metadata(std::collections::HashMap::from([(
                String::from("sutura:text_policy"),
                policy,
            )])),
        ])),
    )
}

type Rows = Vec<Vec<Value>>;

/// A question over a set of tables, answered with the rows or the way it failed.
type Asked<'a> = dyn Fn(&[StatementTable], &str) -> Result<Rows, String> + 'a;

/// `Ok` where `actual` is `expected`; the comparison the calling cell asserts on.
fn same<T>(actual: &T, expected: &T, what: &str) -> Result<(), String>
where
    T: PartialEq + std::fmt::Debug,
{
    if actual == expected {
        Ok(())
    } else {
        Err(format!("{what}: actual={actual:?}; expected={expected:?}"))
    }
}

fn native<W>(warehouse: &W, sql: &str, scans: &BTreeSet<QualifiedTable>, dialect: TypeDialect) -> RawRows
where
    W: DataSystemUnderTest,
{
    let dialect = match dialect {
        TypeDialect::Oracle => polyglot_sql::DialectType::Oracle,
        TypeDialect::BigQuery => polyglot_sql::DialectType::BigQuery,
        TypeDialect::ClickHouse => polyglot_sql::DialectType::ClickHouse,
        TypeDialect::DuckDb => polyglot_sql::DialectType::DuckDB,
        TypeDialect::Postgres => polyglot_sql::DialectType::PostgreSQL,
        TypeDialect::Foundry => polyglot_sql::DialectType::Spark,
        TypeDialect::DataFusion | TypeDialect::Cube => polyglot_sql::DialectType::Generic,
    };
    let sql = polyglot_sql::transpile(sql, polyglot_sql::DialectType::Generic, dialect)
        .expect("the independent native reference transpiles");
    let [sql] = sql.as_slice() else {
        panic!("one native reference")
    };
    warehouse
        .execute_planned_statement(
            &RawStatement::parse(sql.clone()).expect("one bounded reference"),
            scans,
            &presented(),
            deadline(),
        )
        .expect("the engine accepts planned SQL")
        .unwrap_or_else(|cause| panic!("{}: {cause:?}: {sql}", W::NAME))
}

#[test]
fn oracle_lob_expressions_use_full_redacted_text_and_only_bound_text_outputs() {
    use sutura_exec_oracle::OracleWarehouse;

    if !runs_here::<OracleWarehouse>() {
        return;
    }
    let warehouse: OracleWarehouse = open(&load_on::<OracleWarehouse>());
    warehouse
        .load_text_policy_fixture()
        .expect("the Oracle policy fixture is created");
    let policy = r#"{"max_text_bytes":128,"redact":[{"pattern":"secret","replace":"masked"}]}"#;
    let lob = |name, kind| {
        Field::new(name, DataType::Utf8, true).with_metadata(std::collections::HashMap::from([
            (String::from("sutura:oracle_lob"), String::from(kind)),
            (String::from("sutura:text_policy"), String::from(policy)),
        ]))
    };
    let path = QualifiedTable::parse("policy_text").expect("a fixture table");
    let tables = [StatementTable::new(
        source(),
        path.clone(),
        TypeDialect::Oracle,
        Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("label", DataType::Utf8, true),
            lob("script", "clob"),
            lob("national", "nclob"),
            Field::new("native_number", DataType::Utf8, true).with_metadata(std::collections::HashMap::from([(
                String::from("sutura:unbounded_number"),
                String::from("utf8"),
            )])),
        ])),
    )];
    let scans = BTreeSet::from([path]);
    // Expected text is constructed independently, without invoking the production policy renderer.
    let reference = r#"(SELECT "id", "label", "native_number", CASE WHEN "id" = 1 THEN TO_CLOB(RPAD('x', 4000, 'x')) || TO_CLOB(' masked END tail') ELSE TO_CLOB('masked END') END AS "script", CASE WHEN "id" = 1 THEN TO_NCLOB(RPAD('x', 4000, 'x')) || TO_NCLOB(' masked END tail') ELSE TO_NCLOB('masked END') END AS "national" FROM "policy_text") r"#;
    for (projection, expected_projection, tail) in [
        (
            r#"CASE WHEN r."id" > 0 THEN r."label" ELSE r."label" END AS label"#,
            r#"r."label""#,
            r#"ORDER BY r."id""#,
        ),
        (
            r#"CASE r."id" WHEN 0 THEN r."label" ELSE r."label" END AS label"#,
            r#"r."label""#,
            r#"ORDER BY r."id""#,
        ),
        (
            r#"r."label", length(r."script") AS n"#,
            r#"r."label", length(r."script") AS n"#,
            r#"ORDER BY n DESC, r."id" LIMIT 3"#,
        ),
        (
            r#"r."label", substr(r."script", 4001, 120)"#,
            r#"r."label", CAST(substr(r."script", 4001, 120) AS VARCHAR2(120))"#,
            r#"ORDER BY r."id""#,
        ),
        (
            r#"r."label", instr(upper(r."script"), 'END') AS p"#,
            r#"r."label", instr(upper(r."script"), 'END') AS p"#,
            r#"ORDER BY p DESC, r."id" LIMIT 3"#,
        ),
        (
            r#"r."label""#,
            r#"r."label""#,
            r#"WHERE upper(r."script") LIKE '%MASKED%' ORDER BY length(r."script") DESC, r."id""#,
        ),
        (
            r#"r."label""#,
            r#"r."label""#,
            r#"WHERE upper(r."script") LIKE '%SECRET%' ORDER BY r."id""#,
        ),
        (
            r#"substr(upper(r."national"), 4001, 120), length(r."national") AS n"#,
            r#"CAST(substr(upper(r."national"), 4001, 120) AS NVARCHAR2(120)), length(r."national") AS n"#,
            r#"ORDER BY n DESC, r."id" LIMIT 3"#,
        ),
        (
            r#"(SELECT min("native_number") FROM "policy_text") AS lo, substr(r."script", 4001, 120) AS late"#,
            r#"(SELECT min("native_number") FROM "policy_text") AS lo, CAST(substr(r."script", 4001, 120) AS VARCHAR2(120)) AS late"#,
            r#"WHERE r."id" = 1"#,
        ),
    ] {
        let sql = format!(r#"SELECT {projection} FROM "policy_text" r {tail}"#);
        let statement = RawStatement::parse(&sql).expect("a fixture query");
        let actual = DataFusionCombiner::new().expect("a combiner").answer_statement(
            StatementQuestion::new(&statement, &tables, false, WORKING_SET, deadline()),
            &mut Executing {
                warehouse: &warehouse,
                expected_scans: vec![scans.clone()],
                observed_scans: Vec::new(),
            },
        );
        let Some(Ok(actual)) = actual else {
            panic!("{sql}: {actual:?}")
        };
        let expected = native(
            &warehouse,
            &format!("SELECT {expected_projection} FROM {reference} {tail}"),
            &scans,
            TypeDialect::Oracle,
        );
        assert_eq!(actual.into_parts().1, expected.into_parts().1, "{sql}");
    }
}

pub(super) fn pins(name: &str, dialect: TypeDialect) {
    let _logging = tracing::subscriber::set_default(
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_test_writer()
            .finish(),
    );
    let path = QualifiedTable::parse("dim_region").expect("a fixture table");
    let tables = [protected(path, dialect, 128)];
    let combiner = DataFusionCombiner::new().expect("a combiner");
    let mut output = String::new();
    for sql in [
        "select \"sales_area\" from \"dim_region\" where \"sales_area\" like 'n%' order by \"sales_area\"",
        "select min(\"sales_area\") from \"dim_region\"",
        "select r.\"sales_area\" from \"dim_region\" r where r.\"sales_area\" = 'masked' order by r.\"sales_area\" limit 1",
    ] {
        let statement = RawStatement::parse(sql).expect("a fixture question");
        let mut recorded = Recorded::default();
        let answer = combiner.answer_statement(
            StatementQuestion::new(&statement, &tables, false, WORKING_SET, deadline()),
            &mut recorded,
        );
        assert!(matches!(answer, Some(Ok(_))), "{dialect:?}: {sql}: {answer:?}");
        assert_eq!(recorded.statements.len(), 1);
        writeln!(output, "{}\n", recorded.statements[0]).expect("a String accepts SQL");
    }
    let mut snapshot_settings = settings(name);
    snapshot_settings.set_snapshot_path("../../snapshots");
    snapshot_settings.bind(|| insta::assert_snapshot!("bounded_redacted_text_sql", output));
}

#[expect(
    clippy::unwrap_in_result,
    reason = "a fixture that does not load or parse is a broken venue, not a contract violation to report"
)]
pub(super) fn executes<W>(
    warehouse: &W,
    pinned: &sutura_domain::pinned::PinnedDefinitions,
    dialect: TypeDialect,
) -> Result<(), String>
where
    W: DataSystemUnderTest + Sync,
    W::Error: Send,
{
    let customer = pinned
        .definitions()
        .model(&ModelName::parse("customers").expect("a model name"))
        .expect("the customer fixture");
    let dialects = pinned
        .definitions()
        .models()
        .values()
        .map(|model| (model.source().clone(), dialect))
        .collect();
    let schemas = sutura_domain::warehouse::schema::PinnedSchemas::pin(pinned.definitions(), &dialects)
        .expect("disjoint policies pin beside certified metrics");
    let tables = [StatementTable::new(
        customer.source().clone(),
        customer.table().clone(),
        dialect,
        Arc::clone(schemas.of(customer.name()).expect("the customer schema")),
    )];
    let statement = RawStatement::parse(format!(
        "select \"customer_id\" from {} where \"customer_id\" = 'masked' limit 1",
        quoted(customer.table())
    ))
    .expect("a protected statement");
    let protected_answer = DataFusionCombiner::new().expect("a combiner").answer_statement(
        StatementQuestion::new(&statement, &tables, false, WORKING_SET, deadline()),
        &mut Executing {
            warehouse,
            expected_scans: vec![BTreeSet::from([customer.table().clone()])],
            observed_scans: Vec::new(),
        },
    );
    let Some(Ok(protected_rows)) = protected_answer else {
        return Err(format!("{}: {protected_answer:?}", W::NAME));
    };
    same(
        &protected_rows.into_parts().1,
        &vec![vec![Value::Text(String::from("masked"))]],
        "a protected customer key",
    )?;

    let model = pinned
        .definitions()
        .model(&ModelName::parse("regions").expect("a model name"))
        .expect("the regions fixture");
    let path = quoted(model.table());
    let tables = [protected(model.table().clone(), dialect, 128)];
    let combiner = DataFusionCombiner::new().expect("a combiner");
    let run = |sql: String, tables: &[StatementTable]| -> Result<Rows, String> {
        let statement = RawStatement::parse(sql).expect("a fixture question");
        let answer = combiner.answer_statement(
            StatementQuestion::new(&statement, tables, false, WORKING_SET, deadline()),
            &mut Executing {
                warehouse,
                expected_scans: vec![BTreeSet::from([model.table().clone()])],
                observed_scans: Vec::new(),
            },
        );
        let Some(Ok(rows)) = answer else {
            return Err(format!("{}: {answer:?}", W::NAME));
        };
        Ok(rows.into_parts().1)
    };
    let text = |value: &str| Value::Text(value.to_owned());
    same(
        &run(
            format!("select \"sales_area\" from {path} where \"region\" = 'north'"),
            &tables,
        )?,
        &vec![vec![text("masked")]],
        "a redacted value",
    )?;
    for prefix in ["n", "no", "nor", "nort", "north", "north_", "north_e"] {
        let found = run(
            format!("select \"region\" from {path} where \"sales_area\" like '{prefix}%'"),
            &tables,
        )?;
        same(&found, &Vec::new(), &format!("a search for the redacted prefix {prefix}"))?;
    }
    same(
        &run(
            format!("select min(\"sales_area\") from {path} where \"region\" = 'north'"),
            &tables,
        )?,
        &vec![vec![text("masked")]],
        "a redacted minimum",
    )?;
    same(
        &run(
            format!("select \"region\" from {path} where \"sales_area\" = 'masked' order by \"sales_area\", \"region\""),
            &tables,
        )?,
        &vec![vec![text("east")], vec![text("north")]],
        "a search for the redacted value",
    )?;
    let bounded = [protected(model.table().clone(), dialect, 32)];
    same(
        &run(
            format!("select \"sales_area\" from {path} where \"region\" = 'south'"),
            &bounded,
        )?,
        &vec![vec![text("sout...[truncated]")]],
        "a bounded value",
    )
}

/// Character functions over a value with a multibyte character, against values spelled out here:
/// a byte-counting or ASCII-only engine cannot pass for a character-counting one, and no reference
/// engine or translation stands between the cell and its expectation.
fn non_ascii_expressions<W>(ask: &Asked, table: &QualifiedTable, dialect: TypeDialect) -> Result<(), String>
where
    W: DataSystemUnderTest,
{
    let tables = [protected_replacing(table.clone(), dialect, 128, "mask\u{e9}")];
    let path = quoted(table);
    let regions = ["central", "east", "north", "south", "west"];
    let value = |region: &str| match region {
        "east" | "north" => "mask\u{e9}",
        "central" => "central",
        _ => "south_west",
    };
    let initcap = |text: &str| {
        let mut previous = false;
        text.chars()
            .map(|character| {
                let alphanumeric = character.is_alphanumeric();
                let mapped = if alphanumeric && !previous {
                    character.to_uppercase().collect::<String>()
                } else {
                    character.to_lowercase().collect()
                };
                previous = alphanumeric;
                mapped
            })
            .collect::<String>()
    };
    // Case mapping follows each engine's locale where it is not Unicode-aware by definition, so
    // the venues known to map by character carry it; `initcap` is not a DuckDB function, `left`
    // and `right` are not Oracle's, and Oracle's `reverse` is undocumented.
    let maps_case = matches!(
        dialect,
        TypeDialect::DataFusion | TypeDialect::DuckDb | TypeDialect::ClickHouse | TypeDialect::BigQuery | TypeDialect::Oracle
    );
    let not_oracle = dialect != TypeDialect::Oracle;
    #[expect(
        clippy::type_complexity,
        reason = "a table of one projection and the function that spells its expectation"
    )]
    let cases: [(bool, &str, Box<dyn Fn(&str) -> String>); 11] = [
        (
            true,
            "length(r.\"sales_area\")",
            Box::new(|text| text.chars().count().to_string()),
        ),
        (
            true,
            "substr(r.\"sales_area\", 4, 3)",
            Box::new(|text| text.chars().skip(3).take(3).collect()),
        ),
        (
            true,
            "position('\u{e9}' IN r.\"sales_area\")",
            Box::new(|text| {
                text.chars()
                    .position(|character| character == '\u{e9}')
                    .map_or(0, |at| at + 1)
                    .to_string()
            }),
        ),
        (
            not_oracle,
            "left(r.\"sales_area\", 4)",
            Box::new(|text| text.chars().take(4).collect()),
        ),
        (
            not_oracle,
            "right(r.\"sales_area\", 2)",
            Box::new(|text| text.chars().skip(text.chars().count().saturating_sub(2)).collect()),
        ),
        (
            not_oracle,
            "reverse(r.\"sales_area\")",
            Box::new(|text| text.chars().rev().collect()),
        ),
        (maps_case, "upper(r.\"sales_area\")", Box::new(str::to_uppercase)),
        (maps_case, "lower(upper(r.\"sales_area\"))", Box::new(str::to_lowercase)),
        (
            maps_case && dialect != TypeDialect::DuckDb,
            "initcap(r.\"sales_area\")",
            Box::new(move |text| initcap(text)),
        ),
        (
            true,
            "translate(r.\"sales_area\", 'k\u{e9}', 'KE')",
            Box::new(|text| text.replace('k', "K").replace('\u{e9}', "E")),
        ),
        (true, "r.\"sales_area\"", Box::new(str::to_owned)),
    ];
    for (runs, projection, expectation) in &cases {
        if !*runs {
            continue;
        }
        let sql = format!("SELECT r.\"region\", {projection} FROM {path} r ORDER BY r.\"region\"");
        let actual: Vec<Vec<String>> = ask(&tables, &sql)?
            .iter()
            .map(|row| row.iter().map(Value::render).collect())
            .collect();
        let expected: Vec<Vec<String>> = regions
            .iter()
            .map(|region| vec![(*region).to_owned(), expectation(value(region))])
            .collect();
        same(&actual, &expected, &format!("{}: {projection}", W::NAME))?;
    }
    Ok(())
}

pub(super) fn expressions<W>(dialect: TypeDialect) -> Result<(), String>
where
    W: DataSystemUnderTest + Send + Sync + 'static,
    W::Error: Send + Sync,
{
    if !runs_here::<W>() {
        return Ok(());
    }
    let started = std::time::Instant::now();
    let progress = |phase: &str, case: &str| {
        if dialect == TypeDialect::BigQuery {
            println!(
                "native-statement-progress: engine={} phase={phase} case={case} elapsed_ms={}",
                W::NAME,
                started.elapsed().as_millis()
            );
        }
    };
    progress("catalog-open", "-");
    let pinned = load_on::<W>();
    let warehouse: W = open(&pinned);
    progress("expressions", "-");
    let model = pinned
        .definitions()
        .model(&ModelName::parse("regions").map_err(|cause| format!("a model name: {cause}"))?)
        .expect("the regions fixture");
    let path = quoted(model.table());
    let combiner = DataFusionCombiner::new().map_err(|cause| format!("a combiner: {cause}"))?;
    let scans = BTreeSet::from([model.table().clone()]);
    let ask = |tables: &[StatementTable], sql: &str| -> Result<Rows, String> {
        let statement = RawStatement::parse(sql).map_err(|cause| format!("a fixture expression query: {cause}"))?;
        let actual = combiner.answer_statement(
            StatementQuestion::new(&statement, tables, false, WORKING_SET, deadline()),
            &mut Executing {
                warehouse: &warehouse,
                expected_scans: vec![scans.clone()],
                observed_scans: Vec::new(),
            },
        );
        let Some(Ok(actual)) = actual else {
            return Err(format!("{}: {sql}: {actual:?}", W::NAME));
        };
        Ok(actual.into_parts().1)
    };
    // Ordinary policies expose the already-truncated value to every operator (the LOB venue
    // has a different contract). The independent view contains only expected safe values.
    let bounded = [protected(model.table().clone(), dialect, 32)];
    let reference = format!(
        "(SELECT \"region\", CASE WHEN \"region\" IN ('east', 'north') THEN 'mask...[truncated]' WHEN \"region\" = 'central' THEN 'cent...[truncated]' ELSE 'sout...[truncated]' END AS \"sales_area\" FROM {path}) r"
    );
    for (projection, tail) in [
        (
            "r.\"region\", length(r.\"sales_area\") AS n",
            "ORDER BY n DESC, r.\"region\" LIMIT 3",
        ),
        ("r.\"region\", substr(r.\"sales_area\", 5, 3)", "ORDER BY r.\"region\""),
        (
            "r.\"region\", position('MASK' IN upper(r.\"sales_area\")) AS p",
            "ORDER BY p DESC, r.\"region\"",
        ),
        (
            "r.\"region\"",
            "WHERE upper(r.\"sales_area\") LIKE 'MASK%' ORDER BY length(r.\"sales_area\") DESC, r.\"region\"",
        ),
    ] {
        let actual = ask(&bounded, &format!("SELECT {projection} FROM {path} r {tail}"))?;
        let expected = native(
            &warehouse,
            &format!("SELECT {projection} FROM {reference} {tail}"),
            &scans,
            dialect,
        )
        .into_parts()
        .1;
        same(&actual, &expected, &format!("{}: {projection} {tail}", W::NAME))?;
    }
    non_ascii_expressions::<W>(&ask, model.table(), dialect)?;
    progress("text-policy", "-");
    executes(&warehouse, &with_disjoint_policy(&pinned, dialect, false), dialect)?;
    progress("served", "-");
    served::<W>(dialect)?;
    progress("complete", "-");
    Ok(())
}

/// Keep the complete certified catalog alongside a policy on an unused column of a joined table.
pub(super) fn with_disjoint_policy(
    pinned: &sutura_domain::pinned::PinnedDefinitions,
    dialect: TypeDialect,
    other_source: bool,
) -> sutura_domain::pinned::PinnedDefinitions {
    use sutura_domain::catalog::{Column, Definitions, Description, Model, TextPolicy};
    use sutura_domain::warehouse::schema::PinnedSchemas;
    let policy: TextPolicy = serde_json::from_value(serde_json::json!({
        "max_text_bytes": 128,
        "redact": [{"pattern": "C[0-9]+", "replace": "masked"}]
    }))
    .expect("a fixture text policy");
    let models = pinned
        .definitions()
        .models()
        .values()
        .map(|model| {
            let columns = model.columns().map(|column| {
                let native_integer = match dialect {
                    TypeDialect::ClickHouse => "Int64",
                    TypeDialect::Oracle => "NUMBER(19,0)",
                    TypeDialect::Cube => "number",
                    _ => "BIGINT",
                };
                let column = if column.data_type().is_some_and(|kind| kind.as_str() == "BIGINT") && native_integer != "BIGINT" {
                    Column::from_metadata(
                        column.name().clone(),
                        Some(native_integer),
                        Some(column.description()),
                        column.nullable(),
                    )
                    .expect("the native fixture column keeps its description")
                    .with_text_policy(column.text_policy().clone())
                } else {
                    column.clone()
                };
                if model.name().as_str() == "customers" && column.name().as_str() == "customer_id" {
                    column.with_text_policy(policy.clone())
                } else {
                    column
                }
            });
            let mut rebuilt = Model::new(
                model.name().clone(),
                if other_source && model.name().as_str() == "customers" {
                    SourceName::parse("notes").expect("a fixture source")
                } else {
                    model.source().clone()
                },
                model.table().clone(),
                columns,
                Description::parse(model.description()).expect("a bounded fixture description"),
            )
            .with_primary_key(model.primary_key().iter().cloned())
            .expect("the original key exists");
            if let Some(audience) = model.audience() {
                rebuilt = rebuilt.with_audience(audience.clone());
            }
            rebuilt
        })
        .collect();
    let definitions = Definitions::assemble(
        models,
        pinned.definitions().relationships().values().cloned().collect(),
        pinned.definitions().metrics().values().cloned().collect(),
    )
    .expect("the catalog keeps its graph");
    let dialects = definitions
        .models()
        .values()
        .map(|model| (model.source().clone(), dialect))
        .collect();
    let schemas = PinnedSchemas::pin(&definitions, &dialects);
    assert!(
        schemas.is_ok(),
        "{dialect:?}: disjoint certified columns must pin: {schemas:?}"
    );
    sutura_domain::pinned::PinnedDefinitions::pin(
        pinned.version().clone(),
        definitions,
        pinned.knowledge().clone(),
        pinned.manifest().clone(),
    )
    .expect("the protected catalog pins")
}

struct Audit;
impl sutura_domain::audit::AuditSink for Audit {
    fn record(&self, _record: &sutura_domain::audit::CallRecord<'_>) {}
}

/// The transport's service port, over the same native venues as the renderer and row matrix.
#[expect(
    clippy::unwrap_in_result,
    reason = "a fixture that does not load or parse is a broken venue, not a contract violation to report"
)]
pub(super) fn served<W>(dialect: TypeDialect) -> Result<(), String>
where
    W: DataSystemUnderTest + Send + Sync + 'static,
    W::Error: Send + Sync,
{
    use sutura_app::surface::{LocalService, Surface as _};
    use sutura_domain::query::ToolOutcome;
    use sutura_domain::raw::RawOutcome;
    for other_source in [false, true] {
        let original = load_on::<W>();
        let pinned = with_disjoint_policy(&original, dialect, other_source);
        let customer = pinned
            .definitions()
            .model(&ModelName::parse("customers").expect("a model"))
            .expect("the customer model");
        let path = quoted(customer.table());
        let mut warehouses = sutura_app::Warehouses::of(open::<W>(&original));
        if other_source {
            let table = customer.table_name().clone();
            let csv = data_root().join(format!("{table}.csv"));
            warehouses = warehouses
                .and(W::open_on(customer.source().clone(), vec![(table, csv)]))
                .expect("the sources have different names");
        }
        let started = LocalService::start_pinned(
            pinned,
            warehouses,
            Audit,
            crate::adapters::shared_credential(),
            DataFusionCombiner::new().expect("a combiner"),
            WORKING_SET,
        );
        let service = started.map_err(|cause| format!("{} placement={other_source}: service must start: {cause:?}", W::NAME))?;
        let certified = service.answer(
            &crate::adapters::a_caller(),
            &crate::shared::question("recurring-revenue-june.yaml"),
            deadline(),
        );
        let Ok(ToolOutcome::Answer { rows, .. }) = certified else {
            return Err(format!(
                "{} placement={other_source}: the served metric must answer: {certified:?}",
                W::NAME
            ));
        };
        let rendered: Vec<Vec<_>> = rows
            .rows()
            .iter()
            .map(|row| row.iter().map(Value::render).collect())
            .collect();
        same(
            &rendered,
            &vec![vec![String::from("2026-06-01"), String::from("202121")]],
            "the served metric",
        )?;
        let statement = RawStatement::parse(format!(
            "select \"customer_id\" from {path} where \"customer_id\" = 'masked' limit 1"
        ))
        .expect("a protected fixture statement");
        let raw = service.run_sql(&crate::adapters::a_caller(), &statement, deadline());
        let Ok(RawOutcome::Rows { rows, .. }) = raw else {
            return Err(format!(
                "{} placement={other_source}: the protected served statement must answer Rows: {raw:?}",
                W::NAME
            ));
        };
        same(&rows, &vec![vec![String::from("masked")]], "the protected served statement")?;
    }
    Ok(())
}
