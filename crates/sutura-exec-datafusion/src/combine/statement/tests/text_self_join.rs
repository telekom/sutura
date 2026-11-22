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
    let sql = r#"select m."region", count(distinct m."sales_area") as n1, count(distinct p."sales_area") as n2
               from "dim_region_self" m join "dim_region_self" p on m."region" = p."region"
               where m."region" <> 'missing' group by m."region" limit 50"#;
    let expect = vec![vec![Value::Text(String::from("a")), Value::Integer(2), Value::Integer(2)]];

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
    let read_by_masked_n =
        column_read_by_alias(generated, "masked_n").unwrap_or_else(|| panic!("masked_n must read one plain column: {generated}"));
    assert!(
        alias_is_a_regexp_replace_output(generated, &read_by_masked_n),
        "masked_n reads \"{read_by_masked_n}\", but that output column of the derived table is \
         not a REGEXP_REPLACE(...) expression - the masking rewrite and the aggregate have come \
         apart: {generated}"
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

/// Whether any projection item aliased `name`, anywhere in `generated`, is a `REGEXP_REPLACE`
/// call - i.e. whether `name` is the masking rewrite's own output column.
fn alias_is_a_regexp_replace_output(generated: &str, name: &str) -> bool {
    use datafusion::sql::sqlparser::ast::{Expr, Function, Select, SelectItem, Visit as _, Visitor};
    use datafusion::sql::sqlparser::dialect::GenericDialect;
    use datafusion::sql::sqlparser::parser::Parser;
    use std::ops::ControlFlow;

    struct Find<'a> {
        name: &'a str,
        matched: bool,
    }
    impl Visitor for Find<'_> {
        type Break = ();

        fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<()> {
            for item in &select.projection {
                let SelectItem::ExprWithAlias { expr, alias } = item else {
                    continue;
                };
                if alias.value != self.name {
                    continue;
                }
                if matches!(expr, Expr::Function(Function { name, .. }) if name.to_string().eq_ignore_ascii_case("REGEXP_REPLACE"))
                {
                    self.matched = true;
                }
            }
            ControlFlow::Continue(())
        }
    }

    let Ok(statements) = Parser::parse_sql(&GenericDialect, generated) else {
        return false;
    };
    let mut finder = Find { name, matched: false };
    let _result: ControlFlow<()> = statements.visit(&mut finder);
    finder.matched
}
