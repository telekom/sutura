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

use super::oracle::OracleLegs;
use super::*;
use sutura_domain::warehouse::Value;

/// #1972/#2041's own shape: `count(DISTINCT ...)` forces the join itself into one derived table
/// before the aggregate runs, so a self-join's two same-named columns must land on two distinct
/// output names in that wrapped schema - never collapsed into one ambiguous one an engine could
/// silently resolve to the wrong side. Run for real (not just unparsed and inspected) on `DuckDB`,
/// `DataFusion` and Oracle (via `oracle_fetch_to_limit`), over ONE table referenced under two
/// aliases `m`/`p` - a genuine self-join, not two different relations that happen to share a
/// column name.
#[test]
fn a_genuine_self_join_over_one_table_runs_the_count_distinct_fallback_in_every_dialect() {
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
    // `p`'s own filter is asymmetric: it drops the "y" row from `p`'s side of the join only,
    // so `m` still sees both "x" and "y" (n1 = 2) while `p` sees only "x" (n2 = 1). A column
    // swap would report (1, 2) instead of (2, 1) - the row check alone now catches it, on top
    // of the AST assertion on `legs.statements` below.
    let sql = r#"select m."region", count(distinct m."sales_area") as n1, count(distinct p."sales_area") as n2
               from "dim_region_self" m join "dim_region_self" p on m."region" = p."region"
               where m."region" <> 'missing' and p."sales_area" <> 'y' group by m."region" limit 50"#;
    let expect = vec![vec![Value::Text(String::from("a")), Value::Integer(2), Value::Integer(1)]];

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
            panic!("{dialect:?}: {sql}: {answer:?}");
        };
        assert_eq!(
            rows.into_parts().1,
            expect,
            "{dialect:?}: the self-join's two same-named columns must count distinctly"
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
    let Some(Ok(rows)) = answer else {
        panic!("Oracle: {sql}: {answer:?}; source SQL: {:?}", legs.statements);
    };
    assert_eq!(
        rows.into_parts().1,
        expect,
        "Oracle: the self-join's two same-named columns must count distinctly; source SQL: {:?}",
        legs.statements
    );

    // Proving the VALUES come out right is not proof this reached the derived-table
    // fallback at all - a plan that pushed the self-join down untouched could coincide on
    // these same numbers. Assert on the generated text itself: the derived table's own
    // projection must carry the self-joined "sales_area" column under two DISTINCT output
    // names (sutura#1972/#2041's own disambiguation), one per side of the join.
    let [generated] = legs.statements.as_slice() else {
        panic!("exactly one Oracle leg statement must have run: {:?}", legs.statements);
    };
    let projection = derived_table_projection(generated);
    let sales_area_outputs: Vec<&str> = projection
        .iter()
        .filter(|(_, expr)| column_name(expr) == "sales_area")
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(
        sales_area_outputs.len(),
        2,
        "the derived table's own projection must carry both self-joined sides of \"sales_area\": {generated}"
    );
    assert_ne!(
        sales_area_outputs[0], sales_area_outputs[1],
        "the self-joined \"sales_area\" columns must get two distinct output names in the derived \
         table, or the outer n1/n2 can silently resolve to the wrong side: {generated}"
    );
}

/// Second review round on #2047 (HIGH): a self-join's mixed-schema derived table must
/// give a masked column and a same-named unmasked column from the other relation two
/// distinct output names. Collapsing both into one ambiguous `"sales_area"` risks an
/// engine resolving the masked reference to the unmasked sibling's value instead - a
/// text-policy leak, not just an ORA-00904/ORA-00918 syntax failure.
///
/// Both relations carry TWO distinct raw values under the same region, so the fallback's
/// own correctness is actually exercised, not just its shape: a wrong column mapping (the
/// HIGH-finding collapse) would make `masked_n` read the PLAIN column and come back as 2
/// (two distinct raw secrets), where masking the right column first collapses both secrets
/// to the one redacted spelling, so `masked_n` must be 1 while the unmasked sibling's own
/// `plain_n` stays 2.
#[test]
fn a_self_join_keeps_a_masked_column_distinct_from_an_unmasked_same_named_one() {
    /// Oracle's `max_text_bytes` policy routes through `DBMS_LOB."SUBSTR"`, which only a live
    /// Oracle venue can run, so a bound-output cell stays captured; this one has no bound, so the
    /// rendered text is plain `REGEXP_REPLACE`, which the fake engine itself cannot execute either
    /// (Oracle's 6-argument form - position, occurrence, flags - has no `DataFusion` equivalent), so
    /// the Oracle leg is checked by AST, not by running it.
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
    // one derived table before the aggregate, so both relations' same-named "sales_area" must
    // land in distinct, correctly-paired output columns of that wrapped schema.
    let sql = r#"select m."region", count(distinct m."sales_area") as masked_n, count(distinct p."sales_area") as plain_n
               from "dim_region_m" m join "dim_region_p" p on m."region" = p."region"
               where m."region" <> 'missing' group by m."region" limit 50"#;

    for dialect in [TypeDialect::DuckDb, TypeDialect::DataFusion, TypeDialect::Cube] {
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
        let Some(Ok(rows)) = answer else {
            panic!("{dialect:?}: {sql}: {answer:?}");
        };
        assert_eq!(
            rows.into_parts().1,
            vec![vec![Value::Text(String::from("a")), Value::Integer(1), Value::Integer(2),]],
            "{dialect:?}: masked_n must count the MASKED relation's own column (both secrets \
             collapse to one redacted spelling) and plain_n the other relation's own, unmasked \
             column (two distinct values stay two) - a wrong column mapping would read masked_n \
             as 2"
        );
    }

    // Oracle: checked on the text the leg above would send, by AST rather than substring count -
    // the SAME column name that `count(distinct ...) as masked_n` reads in the outer query must
    // be the derived table's own `REGEXP_REPLACE(...)`-wrapped projection item, not the bare
    // sibling column from the other relation.
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
    assert!(matches!(answer, Some(Err(StatementStopped::Leg(_)))), "{answer:?}");
    let [generated] = legs.0.as_slice() else {
        panic!("one derived-table statement must reach the port: {:?}", legs.0);
    };
    assert_masking_rewrite_is_sound(generated);
}

/// `masked_n` and `plain_n` must each resolve, through the derived table's OWN FROM
/// subquery's projection list only (never any `SELECT` elsewhere in the statement), to a
/// UNIQUE output name there; `masked_n`'s item must be the `REGEXP_REPLACE(...)` rewrite and
/// `plain_n`'s must not be. The broken collapse this guards against puts both a
/// `REGEXP_REPLACE(..) AS "sales_area"` and a bare `"sales_area"` in the SAME projection
/// list, which a check that only asks "does any projection anywhere have this name as a
/// `REGEXP_REPLACE`" cannot tell apart from the fixed shape.
fn assert_masking_rewrite_is_sound(generated: &str) {
    let projection = derived_table_projection(generated);
    let masked_col =
        column_read_by_alias(generated, "masked_n").unwrap_or_else(|| panic!("masked_n must read one plain column: {generated}"));
    let plain_col =
        column_read_by_alias(generated, "plain_n").unwrap_or_else(|| panic!("plain_n must read one plain column: {generated}"));

    let masked_matches: Vec<_> = projection.iter().filter(|(name, _)| *name == masked_col).collect();
    assert_eq!(
        masked_matches.len(),
        1,
        "\"{masked_col}\" must be the UNIQUE output name in the derived table's own projection, \
         not shared with a sibling item: {generated}"
    );
    assert!(
        is_regexp_replace(&masked_matches[0].1),
        "masked_n reads \"{masked_col}\", but that output column of the derived table is \
         not a REGEXP_REPLACE(...) expression - the masking rewrite and the aggregate have come \
         apart: {generated}"
    );

    assert_ne!(
        masked_col, plain_col,
        "masked_n and plain_n must resolve to two DIFFERENT derived-table output columns, not the \
         same one: {generated}"
    );
    let plain_matches: Vec<_> = projection.iter().filter(|(name, _)| *name == plain_col).collect();
    assert_eq!(
        plain_matches.len(),
        1,
        "\"{plain_col}\" must also be the unique output name in the derived table's own projection: {generated}"
    );
    assert!(
        !is_regexp_replace(&plain_matches[0].1),
        "plain_n reads \"{plain_col}\", which the derived table's own projection builds via \
         REGEXP_REPLACE - the masking rewrite leaked onto the unmasked sibling: {generated}"
    );
}

/// The exact source column name (bare or disambiguated with a numeric suffix) that the
/// projection item aliased `alias` reads, when that item is `<fn>(<ident>)` - e.g. the single
/// argument of `count(DISTINCT "c1")`. `None` if no such alias, or its argument, is found.
fn column_read_by_alias(generated: &str, alias: &str) -> Option<String> {
    use datafusion::sql::sqlparser::ast::{
        Expr, Function, FunctionArg, FunctionArgExpr, FunctionArguments, Select, SelectItem, Visit as _, Visitor,
    };
    use datafusion::sql::sqlparser::dialect::GenericDialect;
    use datafusion::sql::sqlparser::parser::Parser;
    use std::ops::ControlFlow;

    struct Find<'a> {
        alias: &'a str,
        found: Option<String>,
    }
    impl Visitor for Find<'_> {
        type Break = ();

        fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<()> {
            for item in &select.projection {
                let SelectItem::ExprWithAlias {
                    expr:
                        Expr::Function(Function {
                            args: FunctionArguments::List(list),
                            ..
                        }),
                    alias: item_alias,
                } = item
                else {
                    continue;
                };
                if item_alias.value != self.alias {
                    continue;
                }
                let [FunctionArg::Unnamed(FunctionArgExpr::Expr(inner))] = list.args.as_slice() else {
                    continue;
                };
                self.found = match inner {
                    Expr::CompoundIdentifier(parts) => parts.last().map(|part| part.value.clone()),
                    Expr::Identifier(ident) => Some(ident.value.clone()),
                    _ => None,
                };
            }
            ControlFlow::Continue(())
        }
    }

    let statements = Parser::parse_sql(&GenericDialect, generated).ok()?;
    let mut finder = Find { alias, found: None };
    let _result: ControlFlow<()> = statements.visit(&mut finder);
    finder.found
}

/// The `(output_name, expr)` pairs in the derived table's OWN projection - the outer
/// statement's `FROM` subquery, not any `SELECT` anywhere else in the statement. Scoping to
/// this one list is what makes "is this name unique and is it the `REGEXP_REPLACE` item"
/// mean something: a check that walks the whole statement tree cannot tell a fixed
/// disambiguated projection from a broken collapse that happens to carry a same-named
/// `REGEXP_REPLACE` item somewhere else.
fn derived_table_projection(generated: &str) -> Vec<(String, datafusion::sql::sqlparser::ast::Expr)> {
    use datafusion::sql::sqlparser::ast::{SelectItem, SetExpr, Statement, TableFactor};
    use datafusion::sql::sqlparser::dialect::GenericDialect;
    use datafusion::sql::sqlparser::parser::Parser;

    let statements = Parser::parse_sql(&GenericDialect, generated).unwrap_or_else(|cause| panic!("{generated}: {cause}"));
    let [Statement::Query(outer)] = statements.as_slice() else {
        panic!("expected exactly one Query statement: {generated}");
    };
    let SetExpr::Select(outer_select) = outer.body.as_ref() else {
        panic!("outer statement is not a plain SELECT: {generated}");
    };
    let Some(table) = outer_select.from.first() else {
        panic!("outer SELECT has no FROM: {generated}");
    };
    let TableFactor::Derived { subquery, .. } = &table.relation else {
        panic!("outer SELECT's FROM is not a derived table: {generated}");
    };
    let SetExpr::Select(inner_select) = subquery.body.as_ref() else {
        panic!("derived table's own body is not a plain SELECT: {generated}");
    };
    inner_select
        .projection
        .iter()
        .map(|item| match item {
            SelectItem::ExprWithAlias { expr, alias } => (alias.value.clone(), expr.clone()),
            SelectItem::UnnamedExpr(expr) => (column_name(expr), expr.clone()),
            other => panic!("unexpected derived-table projection item {other:?}: {generated}"),
        })
        .collect()
}

/// The bare column name a plain column-reference expression reads - panics on anything else,
/// since the derived table's own projection items are always plain (possibly qualified)
/// column references or their `REGEXP_REPLACE` masking rewrite.
fn column_name(expr: &datafusion::sql::sqlparser::ast::Expr) -> String {
    use datafusion::sql::sqlparser::ast::Expr;
    match expr {
        Expr::CompoundIdentifier(parts) => parts.last().map_or_else(String::new, |part| part.value.clone()),
        Expr::Identifier(ident) => ident.value.clone(),
        Expr::Function(_) => String::new(),
        other => panic!("expected a plain column reference or a function call, found {other:?}"),
    }
}

/// Whether `expr` is a `REGEXP_REPLACE(...)` call - i.e. the masking rewrite's own output.
fn is_regexp_replace(expr: &datafusion::sql::sqlparser::ast::Expr) -> bool {
    use datafusion::sql::sqlparser::ast::{Expr, Function};
    matches!(expr, Expr::Function(Function { name, .. }) if name.to_string().eq_ignore_ascii_case("REGEXP_REPLACE"))
}
