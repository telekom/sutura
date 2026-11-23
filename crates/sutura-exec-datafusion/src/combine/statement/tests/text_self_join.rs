//! A self-join through the mixed-schema derived-table fallback: one table under two aliases, and a
//! masked column beside a same-named unmasked one. Once both sides sit inside one derived table, a
//! BARE reference to the colliding name cannot say which side it means, and guessing would be a
//! text-policy leak for a masked/unmasked pair. So the unparser refuses it
//! (`AmbiguousDerivedColumn`, surfaced as
//! `StatementStopped::UnsupportedSyntax(SqlConstruct::AmbiguousSelfJoinColumn)`), in every dialect.
//! A QUALIFIED reference is never ambiguous and renders unmodified.
//!
//! The cells read the generated statement by AST, so a plan that pushed the whole self-join down
//! untouched, or that swapped the two sides, does not pass. Limit: a `count(1)`-only self-join is
//! not covered here - projection pushdown empties the wrapped schema before the guard runs, so no
//! mutation of the guard changes that outcome. Rendering a value instead of a refusal for the
//! masked/unmasked shape is not possible: the engine rejects the two identical bare
//! `count(DISTINCT "sales_area")` projections as an ambiguous reference.

use super::oracle::OracleLegs;
use super::*;

/// #1972/#2041's own shape: `count(DISTINCT ...)` forces the join itself into one derived table
/// before the aggregate runs. One table referenced under two aliases `m`/`p` - a genuine
/// self-join, not two different relations that happen to share a column name - exposes the same
/// bare column name under both the wrapped plan's qualifiers; the fallback can no longer rewrite
/// an unqualified outer reference unambiguously onto either side, so this is refused rather than
/// resolved by a guess. Checked for real (not just unparsed and inspected) on `DuckDB`,
/// `DataFusion` and Oracle (via the Oracle leg's `FETCH` normalisation).
#[test]
fn a_genuine_self_join_over_one_table_is_refused_in_every_dialect() {
    let table = || {
        RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Field::new("region", DataType::Utf8, true),
                Field::new("sales_area", DataType::Utf8, true),
            ])),
            vec![
                Arc::new(StringArray::from(vec![Some("a"), Some("a")])),
                Arc::new(StringArray::from(vec![Some("x"), Some("y")])),
            ],
        )
        .expect("two rectangular source rows")
    };
    let schema = || {
        Arc::new(Schema::new(vec![
            Field::new("region", DataType::Utf8, true),
            Field::new("sales_area", DataType::Utf8, true),
        ]))
    };
    let sql = r#"select m."region", count(distinct m."sales_area") as n1, count(distinct p."sales_area") as n2
               from "dim_region_self" m join "dim_region_self" p on m."region" = p."region"
               where m."region" <> 'missing' and p."sales_area" <> 'y' group by m."region" limit 50"#;

    for dialect in [
        TypeDialect::DuckDb,
        TypeDialect::DataFusion,
        TypeDialect::Postgres,
        TypeDialect::ClickHouse,
        TypeDialect::BigQuery,
    ] {
        let tables = [StatementTable::new(
            named("geo"),
            QualifiedTable::parse("dim_region_self").unwrap(),
            dialect,
            schema(),
        )];
        let mut legs = Engines::new(None);
        drop(legs.at.insert(named("geo"), holding(&[("dim_region_self", table())])));
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
        assert!(
            matches!(
                answer,
                Some(Err(StatementStopped::UnsupportedSyntax(
                    SqlConstruct::AmbiguousSelfJoinColumn
                )))
            ),
            "{dialect:?}: a self-join colliding on a bare column name must be refused, never \
             silently resolved to either side: {answer:?}"
        );
    }

    let tables = [StatementTable::new(
        named("geo"),
        QualifiedTable::parse("dim_region_self").unwrap(),
        TypeDialect::Oracle,
        schema(),
    )];
    let mut legs = OracleLegs {
        engines: Engines::new(None),
        statements: Vec::new(),
    };
    drop(legs.engines.at.insert(named("geo"), holding(&[("dim_region_self", table())])));
    let statement = RawStatement::parse(sql).expect("a fixture question");
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
    assert!(
        matches!(
            answer,
            Some(Err(StatementStopped::UnsupportedSyntax(
                SqlConstruct::AmbiguousSelfJoinColumn
            )))
        ),
        "Oracle: a self-join colliding on a bare column name must be refused, never silently \
         resolved to either side: {answer:?}; source SQL: {:?}",
        legs.statements
    );
    assert!(
        legs.statements.is_empty(),
        "a refused statement must never reach the port: {:?}",
        legs.statements
    );
}

/// Second review round on #2047 (HIGH): a self-join's mixed-schema derived table would give a
/// masked column and a same-named unmasked column from the other relation the SAME bare output
/// name. Picking one side risks an engine resolving the masked reference to the unmasked
/// sibling's value instead - a text-policy leak, not just an `ORA-00904`/`ORA-00918` syntax
/// failure - so this is refused outright rather than disambiguated by a guess.
#[test]
fn a_self_join_between_a_masked_and_an_unmasked_same_named_column_is_refused() {
    /// Oracle's `max_text_bytes` policy routes through `DBMS_LOB."SUBSTR"`, which only a live
    /// Oracle venue can run, so a bound-output cell stays captured; this one has no bound, so a
    /// successful render would be plain `REGEXP_REPLACE`, which the fake engine itself cannot
    /// execute either - but the statement is refused before any text reaches this port at all.
    struct Captured(Vec<String>);
    impl StatementLegs for Captured {
        type Stop = SourceName;

        fn open(&mut self, _: &BTreeSet<SourceName>) -> Result<(), Self::Stop> {
            Ok(())
        }

        fn run(&mut self, source: &SourceName, statement: &RawStatement) -> Result<RawRows, Self::Stop> {
            self.0.push(String::from(statement.as_str()));
            Err(source.clone())
        }
    }

    let redact_only = r#"{"redact":[{"pattern":"IDENTIFIED BY [^ ]+","replace":"IDENTIFIED BY ***"}]}"#;
    let masked_schema = || {
        Arc::new(Schema::new(vec![
            Field::new("region", DataType::Utf8, true),
            Field::new("sales_area", DataType::Utf8, true).with_metadata(std::collections::HashMap::from([(
                String::from("sutura:text_policy"),
                String::from(redact_only),
            )])),
        ]))
    };
    let plain_schema = || {
        Arc::new(Schema::new(vec![
            Field::new("region", DataType::Utf8, true),
            Field::new("sales_area", DataType::Utf8, true),
        ]))
    };
    // github.com/telekom/sutura#1972's own shape: `count(DISTINCT)` forces the join itself into
    // one derived table before the aggregate, and both relations share the bare column name
    // "sales_area".
    let sql = r#"select m."region", count(distinct m."sales_area") as masked_n, count(distinct p."sales_area") as plain_n
               from "dim_region_m" m join "dim_region_p" p on m."region" = p."region"
               where m."region" <> 'missing' group by m."region" limit 50"#;

    // Cube is excluded here: `statement.rs` keeps a Cube join LOCAL rather than pushing it down
    // (`"Keep Cube joins local while the same cut pushes their supported children down"`), so this
    // self-join's masked/unmasked aggregate runs over the real typed `LogicalPlan` in the local
    // engine, never through the unparser's text-rendering path this refusal guards - there is no
    // bare-identifier ambiguity to resolve because nothing is ever rendered to ambiguous text.
    // Checked below instead: the masking is still applied correctly and the two counts still
    // differ as expected, proving the local path is itself safe (not merely untested).
    for dialect in [
        TypeDialect::DuckDb,
        TypeDialect::DataFusion,
        TypeDialect::Postgres,
        TypeDialect::ClickHouse,
        TypeDialect::BigQuery,
    ] {
        let masked = RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Field::new("region", DataType::Utf8, true),
                Field::new("sales_area", DataType::Utf8, true),
            ])),
            vec![
                Arc::new(StringArray::from(vec![Some("a"), Some("a")])),
                Arc::new(StringArray::from(vec![
                    Some("IDENTIFIED BY secret-one"),
                    Some("IDENTIFIED BY secret-two"),
                ])),
            ],
        )
        .expect("two rectangular masked rows");
        let plain = RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Field::new("region", DataType::Utf8, true),
                Field::new("sales_area", DataType::Utf8, true),
            ])),
            vec![
                Arc::new(StringArray::from(vec![Some("a"), Some("a")])),
                Arc::new(StringArray::from(vec![Some("north"), Some("south")])),
            ],
        )
        .expect("two rectangular plain rows");
        let tables = [
            StatementTable::new(
                named("geo"),
                QualifiedTable::parse("dim_region_m").unwrap(),
                dialect,
                masked_schema(),
            ),
            StatementTable::new(
                named("geo"),
                QualifiedTable::parse("dim_region_p").unwrap(),
                dialect,
                plain_schema(),
            ),
        ];
        let mut legs = Engines::new(None);
        drop(
            legs.at
                .insert(named("geo"), holding(&[("dim_region_m", masked), ("dim_region_p", plain)])),
        );
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
        assert!(
            matches!(
                answer,
                Some(Err(StatementStopped::UnsupportedSyntax(
                    SqlConstruct::AmbiguousSelfJoinColumn
                )))
            ),
            "{dialect:?}: a masked/unmasked self-join colliding on a bare column name must be \
             refused, never silently resolved to either side (which could unmask): {answer:?}"
        );
    }

    // Oracle: same refusal, checked on the leg port rather than a captured rendering - no
    // statement must reach the port at all.
    let tables = [
        StatementTable::new(
            named("geo"),
            QualifiedTable::parse("dim_region_m").unwrap(),
            TypeDialect::Oracle,
            masked_schema(),
        ),
        StatementTable::new(
            named("geo"),
            QualifiedTable::parse("dim_region_p").unwrap(),
            TypeDialect::Oracle,
            plain_schema(),
        ),
    ];
    let statement = RawStatement::parse(sql).expect("a fixture question");
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
    assert!(
        matches!(
            answer,
            Some(Err(StatementStopped::UnsupportedSyntax(
                SqlConstruct::AmbiguousSelfJoinColumn
            )))
        ),
        "Oracle: a masked/unmasked self-join colliding on a bare column name must be refused: {answer:?}"
    );
    assert!(
        legs.0.is_empty(),
        "a refused statement must never reach the port: {:?}",
        legs.0
    );
}
