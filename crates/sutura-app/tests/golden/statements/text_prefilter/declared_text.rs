//! Ordinary text siblings use the production schema pin, not a hand-built Utf8 assertion.

use sutura_domain::catalog::{Column, Definitions, Description, Model};
use sutura_domain::model::ColumnName;
use sutura_domain::warehouse::schema::PinnedSchemas;

use super::*;

fn fixture_csv() -> std::path::PathBuf {
    let root = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "text-prefilter-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&root).expect("a private fixture directory");
    let path = root.join("rows.csv");
    let mut file = std::io::BufWriter::new(std::fs::File::create(&path).expect("a fixture file"));
    writeln!(file, "body,cohort,env,is_deleted").expect("a header");
    for _ in 0..ROWS - RARE.len() - 2 {
        writeln!(file, "unrelated,filler,E,N").expect("a filler row");
    }
    for (body, cohort) in RARE {
        let env = if *body == "caf\u{e9} TERM" { "W" } else { "E" };
        let deleted = if *body == "term" { "Y" } else { "N" };
        writeln!(file, "{body},{cohort},{env},{deleted}").expect("a rare row");
    }
    writeln!(file, "{} TERM,rare,E,N", "a".repeat(4100)).expect("a real wide text cell");
    writeln!(file, "{} secret_TERM,hidden,E,N", "a".repeat(4100)).expect("a late hidden term");
    file.flush().expect("all actual fixture rows reach disk");
    path
}

pub(super) fn native<W>()
where
    W: TextFixture + Sync,
    W::Error: Send,
{
    if !runs_here::<W>() {
        return;
    }
    let setup = Instant::now();
    let csv = fixture_csv();
    let (warehouse, path) = W::open_text_fixture(csv.clone());
    let dialect = warehouse.type_dialect().expect("a registered native dialect");
    observe_population(&warehouse, &path, dialect, setup);
    let base = serde_json::json!({"max_text_bytes":128,"redact":[{"pattern":"secret_[^ ]+","replace":"[masked]"}]});
    verify(&warehouse, &path, dialect, &base);
    warehouse.close_text_fixture(&path);
    std::fs::remove_dir_all(csv.parent().expect("a private directory")).expect("remove the generated CSV");
}

fn table(path: &QualifiedTable, dialect: TypeDialect, policy: &serde_json::Value) -> StatementTable {
    let text = match dialect {
        TypeDialect::Oracle => "VARCHAR2(4000)",
        TypeDialect::ClickHouse => "String",
        TypeDialect::BigQuery | TypeDialect::Cube | TypeDialect::Foundry => "STRING",
        TypeDialect::DuckDb | TypeDialect::DataFusion | TypeDialect::Postgres => "text",
    };
    let columns = ["body", "cohort", "env", "is_deleted"]
        .into_iter()
        .map(|name| {
            let declared = if name == "body" && dialect == TypeDialect::Oracle {
                "CLOB"
            } else {
                text
            };
            let column = Column::from_metadata(
                ColumnName::parse(name).expect("a fixture column"),
                Some(declared),
                None,
                Some(true),
            )
            .expect("a declared fixture column")
            .with_collation(Collation::Binary);
            if name == "body" {
                column.with_text_policy(serde_json::from_value(policy.clone()).expect("a fixture text policy"))
            } else {
                column
            }
        })
        .collect::<Vec<_>>();
    let name = ModelName::parse("notes").expect("a fixture model");
    let definitions = Definitions::assemble(
        vec![Model::new(
            name.clone(),
            source(),
            path.clone(),
            columns,
            Description::default(),
        )],
        vec![],
        vec![],
    )
    .expect("a fixture definition");
    let schemas = PinnedSchemas::pin(&definitions, &BTreeMap::from([(source(), dialect)]))
        .expect("the actual production schema pin accepts the declarations");
    StatementTable::new(
        source(),
        path.clone(),
        dialect,
        Arc::clone(schemas.of(&name).expect("the fixture schema pins")),
    )
}

pub(super) fn verify<W>(warehouse: &W, path: &QualifiedTable, dialect: TypeDialect, base: &serde_json::Value)
where
    W: DataSystemUnderTest + Sync,
    W::Error: Send,
{
    for upper in [false, true] {
        let expression = if upper { "UPPER(\"body\")" } else { "\"body\"" };
        let sql = format!(
            "SELECT \"body\" FROM {} WHERE \"env\" = 'E' AND \"is_deleted\" = 'N' AND {expression} LIKE '%TERM%'",
            quoted(path)
        );
        let mut answers = Vec::new();
        let mut searches = Vec::new();
        for enabled in [false, true] {
            let mut policy = base.clone();
            if enabled {
                policy["raw_prefilter"] =
                    serde_json::json!({"acknowledged_because":"this fixture accepts secret-dependent timing"});
            }
            let started = Instant::now();
            let (rows, planned) = execute_recorded(warehouse, table(path, dialect, &policy), &sql)
                .unwrap_or_else(|cause| panic!("{} ordinary-text-siblings enabled={enabled} upper={upper}: {cause}", W::NAME));
            searches.push(searches_in(&planned, "TERM"));
            let expected = prefilter_capable(dialect) && (!upper || dialect == TypeDialect::Oracle);
            if dialect == TypeDialect::BigQuery {
                assert_backticked([&[], &planned], path, "TERM", enabled && expected);
            }
            if dialect == TypeDialect::ClickHouse {
                assert_eq!(
                    stored_searches_in(&planned, dialect, path, "TERM"),
                    usize::from(enabled && expected)
                );
            }
            println!(
                "raw-prefilter-query: engine={} profile=ordinary-text-siblings upper={upper} mode={} scope=env-and-is-deleted elapsed_ms={} returned_rows={}",
                W::NAME,
                if enabled { "opted-in" } else { "baseline" },
                started.elapsed().as_millis(),
                rows.rows().len()
            );
            answers.push(sorted(rows));
        }
        assert_eq!(
            answers[0],
            answers[1],
            "exact typed rows and duplicate multiplicity: {} upper={upper}",
            W::NAME
        );
        let mut expected = vec![vec![Value::Text(String::from("visible TERM"))]; 2];
        if dialect == TypeDialect::Oracle {
            expected.insert(0, vec![Value::Text(format!("{}...[truncated]", "a".repeat(114)))]);
        }
        assert_eq!(
            answers[0].1,
            expected,
            "ordinary siblings exclude the other visible rows; Oracle searches the full CLOB before clipping output: {} upper={upper}",
            W::NAME
        );
        assert!(searches[0] > 0, "the baseline search reached the native source");
        assert_eq!(
            searches[1] - searches[0],
            usize::from(prefilter_capable(dialect) && (!upper || dialect == TypeDialect::Oracle)),
            "the native statement carries the raw search beside two declared ordinary text siblings: {} upper={upper}",
            W::NAME
        );
    }
}
