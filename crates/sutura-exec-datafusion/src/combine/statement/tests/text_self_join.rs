//! Second review round on github.com/telekom/sutura#2047's HIGH finding: a self-join's
//! mixed-schema derived table must give a masked column and a same-named unmasked column
//! from the other relation two distinct output names, split out of text.rs to hold the
//! `max-lines` cap.

use super::*;
use sutura_domain::warehouse::Value;

/// Second review round on #2047 (HIGH): a self-join's mixed-schema derived table must
/// give a masked column and a same-named unmasked column from the other relation two
/// distinct output names. Collapsing both into one ambiguous `"sales_area"` risks an
/// engine resolving the masked reference to the unmasked sibling's value instead - a
/// text-policy leak, not just an ORA-00904/ORA-00918 syntax failure.
#[test]
fn a_self_join_keeps_a_masked_column_distinct_from_an_unmasked_same_named_one() {
    /// Oracle: `DBMS_LOB."SUBSTR"`/`REGEXP_REPLACE` belong to a live venue, so the Oracle leg of
    /// this test is captured rather than run; `explicit_source_projection` filters out a leg that
    /// is not yet the source's own fully-resolved projection.
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

    let masked_schema = |dialect: TypeDialect| {
        let policy = if dialect == TypeDialect::Oracle {
            // ponytail: Oracle's bounded-text path routes any `max_text_bytes` policy through
            // `DBMS_LOB."SUBSTR"`, which only a live Oracle venue can run - so the Oracle leg of
            // this test is driven structurally (below), never executed, and needs no bound here.
            r#"{"redact":[{"pattern":"IDENTIFIED BY [^ ]+","replace":"IDENTIFIED BY ***"}]}"#
        } else {
            r#"{"max_text_bytes":128,"redact":[{"pattern":"IDENTIFIED BY [^ ]+","replace":"IDENTIFIED BY ***"}]}"#
        };
        Arc::new(Schema::new(vec![
            Field::new("region", DataType::Utf8, true),
            Field::new("sales_area", DataType::Utf8, true).with_metadata(std::collections::HashMap::from([(
                String::from("sutura:text_policy"),
                String::from(policy),
            )])),
        ]))
    };
    let plain_schema = || {
        Arc::new(Schema::new(vec![
            Field::new("region", DataType::Utf8, true),
            Field::new("sales_area", DataType::Utf8, true),
        ]))
    };
    let sql = r#"select m."sales_area" as masked_val, p."sales_area" as plain_val
               from "dim_region_m" m join "dim_region_p" p on m."region" = p."region"
               where m."region" = 'a'"#;
    // `count(distinct)` forces the join itself into one derived table first (the mixed-schema
    // fallback - #1972's own shape), so both relations' same-named "sales_area" land in ONE
    // wrapped schema before the aggregate - the HIGH-finding collision, not a plain join of two
    // separate table references.
    let oracle_sql = r#"select m."region", count(distinct m."sales_area") as masked_n, count(distinct p."sales_area") as plain_n
               from "dim_region_m" m join "dim_region_p" p on m."region" = p."region"
               where m."region" <> 'missing' group by m."region" limit 50"#;

    for dialect in [TypeDialect::DataFusion, TypeDialect::Cube] {
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Field::new("region", DataType::Utf8, true),
                Field::new("sales_area", DataType::Utf8, true),
            ])),
            vec![
                Arc::new(StringArray::from(vec![Some("a")])),
                Arc::new(StringArray::from(vec![Some("IDENTIFIED BY secret")])),
            ],
        )
        .expect("one rectangular source row");
        let tables = [
            StatementTable::new(
                named("geo"),
                QualifiedTable::parse("dim_region_m").unwrap(),
                dialect,
                masked_schema(dialect),
            ),
            StatementTable::new(
                named("geo"),
                QualifiedTable::parse("dim_region_p").unwrap(),
                dialect,
                plain_schema(),
            ),
        ];
        let mut legs = Engines::new(None);
        drop(legs.at.insert(
            named("geo"),
            holding(&[("dim_region_m", batch.clone()), ("dim_region_p", batch)]),
        ));
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
            panic!("{dialect:?}: {sql}: {answer:?}");
        };
        assert_eq!(
            rows.into_parts().1,
            vec![vec![
                Value::Text(String::from("IDENTIFIED BY ***")),
                Value::Text(String::from("IDENTIFIED BY secret")),
            ]],
            "{dialect:?}: masked_val must stay masked and plain_val must stay the other relation's own, unmasked value"
        );
    }

    // Oracle: the fix is checked on the text the leg above would send - the two same-named
    // "sales_area" columns from "dim_region_m" and "dim_region_p" must land on two distinct
    // output names in the derived table, one still wrapped in the masking rewrite and the other
    // bare, never collapsed into one ambiguous `"sales_area"`.
    let tables = [
        StatementTable::new(
            named("geo"),
            QualifiedTable::parse("dim_region_m").unwrap(),
            TypeDialect::Oracle,
            masked_schema(TypeDialect::Oracle),
        ),
        StatementTable::new(
            named("geo"),
            QualifiedTable::parse("dim_region_p").unwrap(),
            TypeDialect::Oracle,
            plain_schema(),
        ),
    ];
    let statement = RawStatement::parse(oracle_sql).expect("a fixture question");
    let mut legs = Captured(Vec::new());
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
    assert!(matches!(answer, Some(Err(StatementStopped::Leg(_)))), "{answer:?}");
    let [generated] = legs.0.as_slice() else {
        panic!("one derived-table statement must reach the port: {:?}", legs.0);
    };
    assert!(
        generated.contains("REGEXP_REPLACE"),
        "the masked column's rewrite must still be present: {generated}"
    );
    // Every spelling of "sales_area" (bare, or disambiguated with a "_N" suffix) that appears
    // as an identifier anywhere in the generated text; the derived table's own two columns
    // (one from each relation) must use two DIFFERENT ones, never collapse onto the same name.
    let spellings: std::collections::HashSet<&str> = {
        let mut set = std::collections::HashSet::new();
        for (start, _) in generated.match_indices("sales_area") {
            let mut end = start + "sales_area".len();
            if generated.get(end..).is_some_and(|rest| rest.starts_with('_')) {
                end += 1;
                while generated
                    .get(end..)
                    .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
                {
                    end += 1;
                }
            }
            if let Some(spelling) = generated.get(start..end) {
                set.insert(spelling);
            }
        }
        set
    };
    assert!(
        spellings.len() >= 2,
        "the two relations' \"sales_area\" must be retargeted onto two distinct output names, \
         not left as the same ambiguous column name: {generated}"
    );
}
