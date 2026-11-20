//! Cube narrows a real million-row source before its capped answer reaches the local text policy.

use sutura_domain::warehouse::Warehouse as _;
use sutura_exec_cube::CubeWarehouse;
use sutura_exec_cube::http::{Endpoint, HttpCube, ReadBounds};

use super::*;

fn protected(path: &QualifiedTable, acknowledged: bool, collation: Option<&str>) -> StatementTable {
    let mut policy = serde_json::json!({
        "max_text_bytes": 128,
        "redact": [{"pattern": "secret_[^ ]+", "replace": "[masked]"}]
    });
    if acknowledged {
        policy["raw_prefilter"] = serde_json::json!({"acknowledged_because": "accepted fixture timing"});
    }
    let mut metadata = HashMap::from([(String::from("sutura:text_policy"), policy.to_string())]);
    if let Some(collation) = collation {
        metadata.insert(String::from("sutura:collation"), String::from(collation));
    }
    StatementTable::new(
        source(),
        path.clone(),
        TypeDialect::Cube,
        Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("body", DataType::Utf8, true).with_metadata(metadata),
            Field::new("cohort", DataType::Utf8, false),
        ])),
    )
}

pub(super) fn native() {
    let setup = Instant::now();
    let endpoint = sutura_dev::provisioned::here(std::path::Path::new(env!("CARGO_MANIFEST_DIR")), "cube-text-search");
    let endpoint = endpoint
        .endpoint()
        .expect("the Cube venue provisions the isolated text-search model");
    let warehouse = CubeWarehouse::of(
        source(),
        crate::adapters::posture(),
        sutura_exec_cube::fixture::tier_token().expect("the fixture deployment token signs"),
        HttpCube::new(
            Endpoint::parse(&format!("http://{endpoint}")).expect("the provisioned endpoint parses"),
            ReadBounds::parse(30, 1 << 20).expect("the existing Cube transport bounds"),
            None,
        ),
    );
    let path = QualifiedTable::parse("text_search_notes").expect("the isolated fixture cube");
    let scans = BTreeSet::from([path.clone()]);
    let direct = |sql| {
        warehouse
            .execute_planned_statement(
                &RawStatement::parse(sql).expect("one fixture read"),
                &scans,
                &presented(),
                deadline(),
            )
            .expect("Cube accepts planned SQL")
    };
    let count = direct("SELECT COUNT(*) FROM text_search_notes").expect("Cube counts the actual source rows");
    assert_eq!(count.rows(), &[vec![Value::Integer(1_000_000)]]);
    println!(
        "raw-prefilter-native: engine=cube actual_rows=1000000 fixture_verification_ms={} policy=local",
        setup.elapsed().as_millis()
    );

    let start = Instant::now();
    let broad = direct("SELECT id, body, cohort FROM text_search_notes")
        .expect_err("a bare million-row scan crosses Cube's answer bounds");
    assert!(
        warehouse.result_did_not_fit(&broad),
        "the unchanged result bounds refuse the broad scan: {broad:?}"
    );
    println!(
        "raw-prefilter-query: engine=cube mode=bare-scan outcome=result-too-large elapsed_ms={}",
        start.elapsed().as_millis()
    );
    let start = Instant::now();
    let broad = execute_recorded(
        &warehouse,
        protected(&path, false, None),
        "SELECT id, body FROM text_search_notes WHERE body LIKE '%TERM%'",
    )
    .expect_err("a protected search without a policy-free filter still crosses the source bounds");
    assert!(
        matches!(
            broad,
            FixtureQueryError::Stopped {
                cause: StatementStopped::Leg(_)
            }
        ),
        "{broad:?}"
    );
    println!(
        "raw-prefilter-query: engine=cube mode=unnarrowed-policy-search outcome=source-refusal elapsed_ms={}",
        start.elapsed().as_millis()
    );
    assert_eq!(
        direct("SELECT id, body FROM text_search_notes WHERE cohort = 'hidden' ORDER BY id")
            .expect("the source holds actual hidden terms")
            .rows(),
        &[
            vec![Value::Text(String::from("0")), Value::Text(String::from("secret_other"))],
            vec![Value::Text(String::from("1")), Value::Text(String::from("secret_TERM"))],
        ]
    );
    let row = |id: &str, body: Option<&str>| {
        vec![
            Value::Text(String::from(id)),
            body.map_or(Value::Null, |body| Value::Text(String::from(body))),
        ]
    };
    for (case, projection, predicate, expected) in [
        (
            "duplicates",
            "body",
            "cohort IN ('rare', 'hidden') AND body LIKE '%TERM%'",
            vec![vec![Value::Text(String::from("visible TERM"))]; 2],
        ),
        ("hidden", "id, body", "cohort = 'hidden' AND body LIKE '%TERM%'", vec![]),
        (
            "replacement",
            "id, body",
            "cohort = 'hidden' AND body = '[masked]'",
            vec![row("0", Some("[masked]")), row("1", Some("[masked]"))],
        ),
        ("null", "id, body", "cohort = 'rare' AND body IS NULL", vec![row("4", None)]),
    ] {
        let sql = format!("SELECT {projection} FROM text_search_notes WHERE {predicate} ORDER BY id");
        let columns: Vec<_> = projection.split(", ").map(String::from).collect();
        for collation in [None, Some("binary"), Some("named:case_insensitive")] {
            let mut answers = Vec::new();
            for acknowledged in [false, true] {
                let start = Instant::now();
                let (rows, planned) = execute_recorded(&warehouse, protected(&path, acknowledged, collation), &sql)
                    .unwrap_or_else(|cause| panic!("Cube {case} acknowledged={acknowledged} collation={collation:?}: {cause}"));
                println!(
                    "raw-prefilter-query: engine=cube profile={case} acknowledged={acknowledged} pinned_collation={collation:?} elapsed_ms={} returned_rows={}",
                    start.elapsed().as_millis(),
                    rows.rows().len()
                );
                assert_eq!(rows.columns(), columns);
                assert_eq!(rows.rows(), expected, "{case}: the independent redacted answer");
                let [statement] = planned.as_slice() else {
                    panic!("one native Cube leg: {planned:?}")
                };
                assert!(
                    statement.contains("WHERE"),
                    "the policy-free filter reached Cube: {statement}"
                );
                assert_eq!(searches_in(&planned, "TERM"), 0, "the policy-column search stays local");
                answers.push(rows);
            }
            assert_eq!(answers[0], answers[1], "Cube never adds a raw condition on a policy column");
        }
    }
}
