//! The same protected physical text at every native engine and every SQL renderer.

use super::*;

fn protected(path: QualifiedTable, dialect: TypeDialect, bytes: u32) -> StatementTable {
    let policy = serde_json::json!({
        "max_text_bytes": bytes,
        "redact": [{"pattern": "(?i)north(_east)?", "replace": "masked"}]
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

pub(super) fn executes<W>(warehouse: &W, pinned: &sutura_domain::pinned::PinnedDefinitions, dialect: TypeDialect)
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
            table: customer.table(),
        },
    );
    let Some(Ok(protected_rows)) = protected_answer else {
        panic!("{}: {protected_answer:?}", W::NAME);
    };
    assert_eq!(protected_rows.into_parts().1, vec![vec![Value::Text(String::from("masked"))]]);

    let model = pinned
        .definitions()
        .model(&ModelName::parse("regions").expect("a model name"))
        .expect("the regions fixture");
    let path = quoted(model.table());
    let tables = [protected(model.table().clone(), dialect, 128)];
    let combiner = DataFusionCombiner::new().expect("a combiner");
    let run = |sql: String, tables: &[StatementTable]| {
        let statement = RawStatement::parse(sql).expect("a fixture question");
        let answer = combiner.answer_statement(
            StatementQuestion::new(&statement, tables, false, WORKING_SET, deadline()),
            &mut Executing {
                warehouse,
                table: model.table(),
            },
        );
        let Some(Ok(rows)) = answer else {
            panic!("{}: {answer:?}", W::NAME);
        };
        rows.into_parts().1
    };
    let text = |value: &str| Value::Text(value.to_owned());
    assert_eq!(
        run(
            format!("select \"sales_area\" from {path} where \"region\" = 'north'"),
            &tables
        ),
        vec![vec![text("masked")]]
    );
    for prefix in ["n", "no", "nor", "nort", "north", "north_", "north_e"] {
        assert!(
            run(
                format!("select \"region\" from {path} where \"sales_area\" like '{prefix}%'"),
                &tables
            )
            .is_empty(),
            "{}: {prefix}",
            W::NAME
        );
    }
    assert_eq!(
        run(
            format!("select min(\"sales_area\") from {path} where \"region\" = 'north'"),
            &tables
        ),
        vec![vec![text("masked")]]
    );
    assert_eq!(
        run(
            format!("select \"region\" from {path} where \"sales_area\" = 'masked' order by \"sales_area\", \"region\""),
            &tables
        ),
        vec![vec![text("east")], vec![text("north")]]
    );
    let bounded = [protected(model.table().clone(), dialect, 32)];
    let rows = run(
        format!("select \"sales_area\" from {path} where \"region\" = 'south'"),
        &bounded,
    );
    assert_eq!(rows, vec![vec![text("sout...[truncated]")]]);
}

/// Keep the complete certified catalog alongside a policy on an unused column of a joined table.
pub(super) fn with_disjoint_policy(
    pinned: &sutura_domain::pinned::PinnedDefinitions,
    dialect: TypeDialect,
    other_source: bool,
) -> sutura_domain::pinned::PinnedDefinitions {
    use sutura_domain::catalog::{Definitions, Description, Model, TextPolicy};
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
                if model.name().as_str() == "customers" && column.name().as_str() == "customer_id" {
                    column.clone().with_text_policy(policy.clone())
                } else {
                    column.clone()
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
pub(super) fn served<W>(dialect: TypeDialect)
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
        assert!(
            started.is_ok(),
            "{} placement={other_source}: service must start: {started:?}",
            W::NAME
        );
        let service = started.expect("the service's startup was asserted");
        let certified = service.answer(
            &crate::adapters::a_caller(),
            &crate::shared::question("recurring-revenue-june.yaml"),
            deadline(),
        );
        assert!(certified.is_ok(), "{} placement={other_source}: {certified:?}", W::NAME);
        let Ok(ToolOutcome::Answer { rows, .. }) = certified else {
            panic!("the served metric must answer");
        };
        let rendered: Vec<Vec<_>> = rows
            .rows()
            .iter()
            .map(|row| row.iter().map(Value::render).collect())
            .collect();
        assert_eq!(rendered, vec![vec![String::from("2026-06-01"), String::from("202121")]]);
        let statement = RawStatement::parse(format!(
            "select \"customer_id\" from {path} where \"customer_id\" = 'masked' limit 1"
        ))
        .expect("a protected fixture statement");
        let raw = service.run_sql(&crate::adapters::a_caller(), &statement, deadline());
        assert!(raw.is_ok(), "{} placement={other_source}: {raw:?}", W::NAME);
        let Ok(RawOutcome::Rows { rows, .. }) = raw else {
            panic!(
                "{} placement={other_source}: the protected served statement must answer Rows: {raw:?}",
                W::NAME
            );
        };
        assert_eq!(rows, vec![vec![String::from("masked")]]);
    }
}
