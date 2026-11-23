//! Second review round on github.com/telekom/sutura#2047's HIGH finding: a self-join's
//! mixed-schema derived table must give a masked column and a same-named unmasked column
//! from the other relation two distinct output names, split out of text.rs to hold the
//! `max-lines` cap.
//!
//! Third round (Opus re-review of 4a78da5f6): the first round's cell joined two DIFFERENT
//! tables with a plain join and no `count(DISTINCT)`, so it likely never reached the
//! mixed-schema fallback at all. This round adds a genuine self-join - one table under two
//! aliases - through the fallback shape, executed on real engines rather than captured text,
//! and replaces the Oracle leg's substring count with an AST assertion that the masked
//! column's own rewritten expression is what the outer aggregate actually reads.
//!
//! Fourth round (Opus re-review of 75abf9df3): the self-join cell asserted only row values
//! that are identical for a correct mapping and a swapped one (`n1 == n2`), and never
//! checked that the Oracle leg's own generated text actually took the derived-table
//! fallback at all - a plan that happened to push the whole self-join down untouched would
//! pass the same assertion. It now also reads `legs.statements` by AST and asserts the
//! derived table's own projection carries two DISTINCT output names for the self-joined
//! column, and an asymmetric filter on the `p` side makes `n1 != n2`, so a swap reddens the
//! row check too. The masked-vs-unmasked cell's AST check is tightened the same way: it used
//! to ask "does ANY projection anywhere carry a `REGEXP_REPLACE` aliased this name", which the
//! broken collapse (both a `REGEXP_REPLACE` and a bare item named `"sales_area"` in the SAME
//! projection) would also satisfy; it now resolves both `masked_n` and `plain_n` through
//! that ONE derived table's own projection list, requires each resolved name to be unique
//! there, and requires only the masked one to be the `REGEXP_REPLACE` item.
//!
//! Sixth round (Opus re-review of 5febf24fb): a self-join suffixes BOTH sides, so a bare
//! unqualified reference the parent built against the wrapped plan can no longer be told
//! which side it meant - and picking one guesses, which for a masked/unmasked pair is a
//! text-policy leak, not just a syntax failure. The disambiguation-renaming machinery these
//! two cells pinned is removed; a self-join whose two sides collide on a bare column name is
//! now refused outright (`AmbiguousDerivedColumn`, surfaced here as `StatementStopped::NotPlanned`)
//! rather than resolved by a guess. Both cells now assert the refusal, across every dialect they
//! used to assert a value for - a self-join is a documented limit, not a value a swap could get
//! wrong undetected.
//!
//! Seventh round (Opus re-review of 19a82b942): the guard fired on collision alone, even when the
//! parent never read either side - removing an answer main gives (`count(1)` over a self-join with
//! a `LIMIT`). The guard now refuses only when the parent actually reads a colliding name;
//! `a_self_join_counted_only_still_answers_despite_an_unreferenced_collision` pins the unreferenced
//! case, and both refusal cells above now also run on Postgres, `ClickHouse` and `BigQuery`
//! (github.com/telekom/sutura#1909), not just `DuckDb`/`DataFusion`/Cube/Oracle. The typed
//! `AmbiguousDerivedColumn` is now downcast end to end into
//! `StatementStopped::UnsupportedSyntax(SqlConstruct::AmbiguousSelfJoinColumn)` rather than the
//! generic `NotPlanned` this file's cells asserted before.

use super::OracleLegs;
use super::*;

/// #1972/#2041's own shape: `count(DISTINCT ...)` forces the join itself into one derived table
/// before the aggregate runs. One table referenced under two aliases `m`/`p` - a genuine
/// self-join, not two different relations that happen to share a column name - exposes the same
/// bare column name under both the wrapped plan's qualifiers; the fallback can no longer rewrite
/// an unqualified outer reference unambiguously onto either side, so this is refused rather than
/// resolved by a guess. Checked for real (not just unparsed and inspected) on `DuckDB`,
/// `DataFusion` and Oracle (via `oracle_fetch_to_limit`).
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

    for dialect in [
        TypeDialect::DuckDb,
        TypeDialect::DataFusion,
        TypeDialect::Cube,
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

/// Opus round 7 (item 1): the collision guard must fire only when the parent actually reads one
/// of the colliding names. A genuine self-join with `count(1)` and a `LIMIT` (no `ORDER BY`, so
/// the join is still wrapped into a derived table) never reads either side's `region`/`sales_area`
/// by name - the wrapped schema collides on both names, but nothing is ever out of scope, so
/// main's own unaliased rendering must stand rather than being refused.
#[test]
fn a_self_join_counted_only_still_answers_despite_an_unreferenced_collision() {
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
    let sql = r#"select count(1) as n from "dim_region_self" m join "dim_region_self" p on m."region" = p."region" limit 50"#;

    for dialect in [TypeDialect::DuckDb, TypeDialect::DataFusion] {
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
        let Some(Ok(rows)) = answer else {
            panic!("{dialect:?}: an unreferenced collision must not remove an answer main gives: {answer:?}");
        };
        // Two source rows, both region "a", self-joined on region: every row pairs with every row,
        // 2 * 2 = 4.
        assert_eq!(
            rows,
            RawRows::of(
                vec![String::from("n")],
                vec![vec![sutura_domain::warehouse::Value::Integer(4)]]
            ),
            "{dialect:?}: {rows:?}"
        );
    }
}
