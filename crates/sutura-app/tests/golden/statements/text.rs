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
                expected_scans: vec![BTreeSet::from([model.table().clone()])],
                observed_scans: Vec::new(),
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
