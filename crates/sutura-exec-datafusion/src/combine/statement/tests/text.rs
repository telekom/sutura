//! Execute the boundary, including multibyte truncation and correlated uses.

use super::*;
use sutura_domain::warehouse::Value;

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
