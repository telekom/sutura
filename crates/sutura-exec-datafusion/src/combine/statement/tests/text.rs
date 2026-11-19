//! Execute the boundary, including multibyte truncation and correlated uses.

use super::*;
use sutura_domain::warehouse::Value;

#[test]
fn oracle_lob_output_contains_no_statement_delimiter() {
    use datafusion::sql::sqlparser::dialect::GenericDialect;
    use datafusion::sql::sqlparser::parser::Parser;

    struct Captured(Vec<String>);
    impl StatementLegs for Captured {
        type Stop = SourceName;

        fn open(&mut self, _: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
            Ok(())
        }

        fn run(&mut self, source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
            if !explicit_source_projection(statement.as_str()) {
                return Err(source.clone());
            }
            self.0.push(String::from(statement.as_str()));
            Err(source.clone())
        }
    }

    let schema = Schema::new(vec![
        Field::new("ROW_ID", DataType::Int64, false),
        Field::new("SCRIPT", DataType::Utf8, true).with_metadata(std::collections::HashMap::from([
            (String::from("sutura:oracle_lob"), String::from("clob")),
            (String::from("sutura:text_policy"), String::from(r#"{"max_text_bytes":4000}"#)),
        ])),
    ]);
    let tables = [StatementTable::new(
        named("geo"),
        QualifiedTable::parse("TEXT_EVENTS").unwrap(),
        TypeDialect::Oracle,
        Arc::new(schema),
    )];
    let statement = RawStatement::parse(r#"SELECT "SCRIPT", "ROW_ID" FROM "TEXT_EVENTS" ORDER BY "ROW_ID""#).unwrap();
    let mut legs = Captured(Vec::new());
    let answer = DataFusionCombiner::new().unwrap().answer_statement(
        StatementQuestion::new(
            &statement,
            &tables,
            false,
            UNBOUNDED,
            opened(Duration::ZERO, Duration::from_secs(60)),
        ),
        &mut legs,
    );
    assert!(matches!(answer, Some(Err(StatementStopped::Leg(_)))), "{answer:?}");
    let [sql] = legs.0.as_slice() else {
        panic!("one bounded LOB projection must reach the port: {:?}", legs.0);
    };
    assert!(
        sql.contains("DBMS_LOB.\"SUBSTR\"") && sql.contains("...[truncated]"),
        "the emitted LOB output must be bounded: {sql}"
    );
    assert!(
        sql.contains("CHR(59)"),
        "the generated ASCII pattern must retain its semicolon without a delimiter: {sql}"
    );
    assert!(
        !sql.contains(';'),
        "the Oracle raw reader refuses a delimiter even inside the generated pattern: {sql}"
    );
    assert_eq!(
        Parser::parse_sql(&GenericDialect, sql)
            .expect("the generated pattern remains a SQL expression")
            .len(),
        1
    );
}

#[test]
fn oracle_native_numeric_extrema_stay_exact_beside_bounded_lob_outputs() {
    use datafusion::sql::sqlparser::ast::{SelectItem, SetExpr, Statement};
    use datafusion::sql::sqlparser::dialect::GenericDialect;
    use datafusion::sql::sqlparser::parser::Parser;

    struct Captured(Vec<String>);
    impl StatementLegs for Captured {
        type Stop = SourceName;

        fn open(&mut self, _: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
            Ok(())
        }

        fn run(&mut self, source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
            if !explicit_source_projection(statement.as_str()) {
                return Err(source.clone());
            }
            self.0.push(String::from(statement.as_str()));
            Err(source.clone())
        }
    }

    let metadata = |pairs: &[(&str, &str)]| {
        pairs
            .iter()
            .map(|(key, value)| (String::from(*key), String::from(*value)))
            .collect()
    };
    let schema = Schema::new(vec![
        Field::new("NATIVE_NUMBER", DataType::Utf8, false).with_metadata(metadata(&[("sutura:unbounded_number", "utf8")])),
        Field::new("SCRIPT", DataType::Utf8, true).with_metadata(metadata(&[
            ("sutura:oracle_lob", "clob"),
            (
                "sutura:text_policy",
                r#"{"max_text_bytes":4000,"redact":[{"pattern":"secret","replace":"***"}]}"#,
            ),
        ])),
        Field::new("SCRIPT_N", DataType::Utf8, true).with_metadata(metadata(&[
            ("sutura:oracle_lob", "nclob"),
            (
                "sutura:text_policy",
                r#"{"max_text_bytes":4000,"redact":[{"pattern":"secret","replace":"***"}]}"#,
            ),
        ])),
    ]);
    let tables = [StatementTable::new(
        named("geo"),
        QualifiedTable::parse("TEXT_EVENTS").unwrap(),
        TypeDialect::Oracle,
        Arc::new(schema),
    )];
    for common in [
        r#"WITH numbers AS (SELECT MIN("NATIVE_NUMBER") AS lo, MAX("NATIVE_NUMBER") AS hi FROM "TEXT_EVENTS")"#,
        r#"WITH first_values AS (SELECT MIN("NATIVE_NUMBER") AS lo, MAX("NATIVE_NUMBER") AS hi FROM "TEXT_EVENTS"), numbers AS (SELECT MAX(lo) AS lo, MIN(hi) AS hi FROM first_values)"#,
    ] {
        // Output aliases deliberately exchange numeric and LOB names.
        let statement = RawStatement::parse(format!(
            r#"{common} SELECT numbers.lo AS "SCRIPT", numbers.hi AS "SCRIPT_N", events."SCRIPT" AS "NATIVE_NUMBER", events."SCRIPT_N" AS "windowed" FROM "TEXT_EVENTS" events CROSS JOIN numbers ORDER BY numbers.lo"#
        ))
        .unwrap();
        let mut legs = Captured(Vec::new());
        let answer = DataFusionCombiner::new().unwrap().answer_statement(
            StatementQuestion::new(
                &statement,
                &tables,
                true,
                UNBOUNDED,
                opened(Duration::ZERO, Duration::from_secs(60)),
            ),
            &mut legs,
        );
        assert!(matches!(answer, Some(Err(StatementStopped::Leg(_)))), "{answer:?}");
        let [sql] = legs.0.as_slice() else {
            panic!("one native statement must reach the port: {:?}", legs.0);
        };
        let parsed = Parser::parse_sql(&GenericDialect, sql).unwrap();
        let [Statement::Query(query)] = parsed.as_slice() else {
            panic!("one generated query: {sql}");
        };
        let SetExpr::Select(select) = query.body.as_ref() else {
            panic!("the output is a projection: {sql}");
        };
        assert_eq!(select.projection.len(), 4, "{sql}");
        for (index, item) in select.projection.iter().enumerate() {
            let SelectItem::ExprWithAlias { expr, .. } = item else {
                panic!("output labels remain explicit: {sql}");
            };
            let expression = expr.to_string();
            if index < 2 {
                assert_eq!(
                    expression,
                    format!("\"sutura_lob_output\".\"c{index}\""),
                    "native NUMBER extrema must not become LOB expressions: {sql}"
                );
            } else {
                assert!(
                    expression.contains("DBMS_LOB.\"SUBSTR\""),
                    "LOB output must remain bounded: {sql}"
                );
                assert!(expression.contains("...[truncated]"), "LOB output retains its marker: {sql}");
                let payload = if index == 2 { "3986" } else { "1986" };
                assert!(expression.contains(payload), "LOB output retains its byte ceiling: {sql}");
            }
        }
        assert!(
            sql.contains("REGEXP_REPLACE"),
            "source redaction must remain before operators: {sql}"
        );
        assert!(!sql.contains(';'), "the native statement cannot contain a delimiter: {sql}");
    }
}

#[test]
fn a_text_policy_replaces_every_use_before_source_operators_and_bounds_utf8() {
    for dialect in [TypeDialect::DataFusion, TypeDialect::Cube] {
        let wide = "\u{e9}".repeat(100);
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Field::new("region", DataType::Utf8, true),
                Field::new("sales_area", DataType::Utf8, true),
            ])),
            vec![
                Arc::new(StringArray::from(vec![Some("a"), Some("b"), Some("c"), Some("d")])),
                Arc::new(StringArray::from(vec![
                    Some("IDENTIFIED BY secret"),
                    Some("IDENTIFIED BY another"),
                    Some(wide.as_str()),
                    None,
                ])),
            ],
        )
        .expect("rectangular source rows");
        let schema = |bytes| {
            Arc::new(Schema::new(vec![
            Field::new("region", DataType::Utf8, true),
            Field::new("sales_area", DataType::Utf8, true).with_metadata(std::collections::HashMap::from([(
                String::from("sutura:text_policy"),
                format!(
                    r#"{{"max_text_bytes":{bytes},"redact":[{{"pattern":"IDENTIFIED BY [^ ]+","replace":"IDENTIFIED BY ***"}}]}}"#
                ),
            )])),
        ]))
        };
        let table = |bytes| {
            StatementTable::new(
                named("geo"),
                QualifiedTable::parse("dim_region").expect("a table"),
                dialect,
                schema(bytes),
            )
        };
        let run = |sql: &str, bytes| {
            let mut legs = Engines::new(None);
            drop(legs.at.insert(named("geo"), holding(&[("dim_region", batch.clone())])));
            let tables = [table(bytes)];
            let statement = RawStatement::parse(sql).expect("a fixture question");
            let answer = DataFusionCombiner::new().expect("a combiner").answer_statement(
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
                panic!("{sql}: {answer:?}");
            };
            rows.into_parts().1
        };
        let masked = vec![vec![Value::Text(String::from("IDENTIFIED BY ***"))]];
        for sql in [
            "select sales_area from dim_region where region = 'a'",
            "select min(sales_area) from dim_region where region in ('a', 'b')",
            "select r.sales_area from dim_region r where r.sales_area = 'IDENTIFIED BY ***' order by r.sales_area, r.region limit 1",
            "select r.sales_area from dim_region r where r.region = 'a' and exists (select 1 from dim_region s where s.sales_area = r.sales_area and s.region = 'b')",
        ] {
            assert_eq!(run(sql, 128), masked, "{sql}");
        }
        for prefix in ["s", "se", "sec", "secr", "secre", "secret"] {
            assert!(
                run(
                    &format!("select region from dim_region where sales_area like '%IDENTIFIED BY {prefix}%'"),
                    128
                )
                .is_empty(),
                "{prefix}"
            );
        }
        assert_eq!(
            run("select sales_area from dim_region where region = 'c'", 32),
            vec![vec![Value::Text(String::from("\u{e9}\u{e9}\u{e9}\u{e9}...[truncated]"))]]
        );
        assert_eq!(
            run("select sales_area from dim_region where region = 'd'", 32),
            vec![vec![Value::Null]]
        );
    }
}

#[test]
fn a_federation_cannot_search_an_oracle_lob_after_a_bounded_leaf() {
    let schema = Arc::new(Schema::new(vec![
        Field::new("region", DataType::Utf8, false),
        Field::new("sales_area", DataType::Utf8, false).with_metadata(std::collections::HashMap::from([
            (String::from("sutura:text_policy"), String::from(r#"{"max_text_bytes":4000}"#)),
            (String::from("sutura:oracle_lob"), String::from("clob")),
        ])),
    ]));
    let tables = [
        StatementTable::new(
            named("geo"),
            QualifiedTable::parse("dim_region").unwrap(),
            TypeDialect::Oracle,
            schema,
        ),
        StatementTable::new(
            named("crm"),
            QualifiedTable::parse("dim_customer").unwrap(),
            TypeDialect::DataFusion,
            customers().schema(),
        ),
    ];
    let mut legs = Engines::new(Some("geo"));
    let statement = RawStatement::parse(
        r#"select c.customer_id from dim_customer c join "dim_region" r on c.region = r."region" where r."sales_area" like '%late-note%'"#,
    )
    .unwrap();
    let answer = DataFusionCombiner::new().unwrap().answer_statement(
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
        legs.opened.is_empty(),
        "a local operator must not consume a clipped LOB: {answer:?}"
    );
    assert!(matches!(answer, Some(Err(StatementStopped::NotPlanned))), "{answer:?}");
    assert!(legs.ran.is_empty(), "no source should run: {:?}", legs.ran);
}
