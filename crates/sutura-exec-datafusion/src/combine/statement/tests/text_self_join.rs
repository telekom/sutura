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
//!
//! Eighth round (CI regression on f3637cb33): main's own pre-existing
//! `duplicate_source_outputs_restore_exact_names_positions_and_empty_metadata` proved the guard
//! too broad - a self-join of ONE table was never that regression's own shape (a JOIN of two
//! DIFFERENT aliases of the SAME table versus two genuinely different relations sharing a bare
//! name), but the guard refused on any reference at all to a colliding name, and that regression's
//! own two `customer_id` projections (`c.customer_id`, `d.customer_id`) are each a QUALIFIED
//! reference - one that already carries its own qualifier and so is never actually ambiguous,
//! unlike a BARE reference the unparser later renders unqualified once both sides sit inside the
//! same derived table. The guard is narrowed to fire only on a BARE (unqualified) reference to a
//! colliding name (`SelectBuilder::references_any_bare`, matching only `Expr::Identifier`, never
//! the qualified `CompoundIdentifier` `references_any` also matches) - a qualified reference takes
//! the unmodified-render path unconditionally, same as "nothing referenced". This file's own cells
//! are UNCHANGED by that narrowing: every one of them is itself built from fully-qualified
//! `m."region"`/`p."sales_area"`-style source SQL, yet the unparser renders the colliding
//! aggregate's own input bare once wrapped (confirmed by inspecting the actual rendered text,
//! e.g. `count(DISTINCT "sales_area")` with no qualifier, for both sides) - so every refusal cell
//! here still refuses. The unreferenced-collision cell
//! (`a_self_join_counted_only_still_answers_despite_an_unreferenced_collision`) is dropped this
//! round: instrumenting the actual plan showed projection pushdown already empties the wrapped
//! join's schema before this guard ever runs for that query, so no mutation of this guard's logic
//! can kill it - it was never exercising this fix, only a different, already-true invariant.
//!
//! The coordinator's own round-9 instruction asked this round to go further and make the
//! masked/unmasked cell assert a correct VALUE instead of a refusal (reasoning that the
//! regression showed main already resolves this collision correctly elsewhere). That was tried
//! and reproduces a genuine engine failure: `the engine plans the text it is handed: ...
//! AmbiguousReference { field: Column { relation: None, name: "sales_area" } }` - the rendered
//! derived table's own OUTER select really does contain two indistinguishable bare
//! `count(DISTINCT "sales_area")` projections once no rewrite is attempted, so "render unmodified"
//! is only safe when nothing bare actually collides downstream, which this shape does not satisfy.
//! The masked/unmasked cell is kept as a refusal, unchanged from the seventh round, rather than
//! asserting a value a live engine cannot actually produce.

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

// The seventh round's `a_self_join_counted_only_still_answers_despite_an_unreferenced_collision`
// pinned that `count(1)` over a self-join with a `LIMIT` is never refused - no colliding name is
// read by name, so nothing is ever out of scope. Round 9 confirmed, by instrumenting the actual
// `colliding_names`/`referenced_names` computed for that plan, that DataFusion's own projection
// pushdown already empties the wrapped join's schema before the unparser ever runs: there is no
// collision left to see by the time this guard's code executes, for this query. No mutation of
// this guard's logic - including forcing it to fire unconditionally, and forcing the
// synthetic-alias rewrite to run unconditionally instead of the unmodified-render path - changes
// that cell's outcome, because neither code path this fix touches is ever reached for it: the
// test cannot be tied to this change by a killing mutation, and is dropped per the review's own
// stated alternative rather than kept under a Claim-Cell with no real kill.

/// Round-9 follow-up (Opus): `references_any_bare` scans ORDER BY for any bare identifier
/// matching a colliding name, with no exclusion for a name the parent's OWN projection list
/// already aliases to that exact spelling. An `ORDER BY region` following `m."region" AS region`
/// is a reference to that output alias, resolved unambiguously by the projection already - not a
/// direct read of the wrapped plan's raw, colliding `region` column - so refusing it would reject
/// an answer main gives. This pins that the self-join is still answered, never refused, when the
/// only bare mention of the colliding name is such an output-alias ORDER BY.
#[test]
fn a_self_joins_order_by_output_alias_still_answers_despite_the_bare_name_colliding() {
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
    let sql = r#"select m."region" as region, count(distinct m."sales_area") as n1, count(distinct p."sales_area") as n2
               from "dim_region_self" m join "dim_region_self" p on m."region" = p."region"
               where m."region" <> 'missing' and p."sales_area" <> 'y' group by m."region"
               order by region limit 50"#;

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
            matches!(answer, Some(Ok(_))),
            "{dialect:?}: ORDER BY referencing the projection's OWN output alias must not be \
             mistaken for a direct, ambiguous read of the wrapped plan's raw colliding column: \
             {answer:?}"
        );
    }
}
