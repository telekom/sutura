//! What the compile refuses, and what it renders.
//!
//! Every case here was reproduced against the dialect layer before it was written down, with
//! sutura's exact feature set. The refusal tests are the load-bearing half: each one is a construct
//! that produces valid-looking SQL and a different number, or reaches data the plan never granted,
//! and **nothing upstream errors on any of them**.

use std::collections::{BTreeMap, BTreeSet};

use sutura_domain::expression::{AuthoredSql, DialectTag, SqlFragment};
use sutura_domain::model::{ColumnName, TableName};

use super::{Construct, ExpressionError, Shape, compile, embed};
use crate::dialect::{ALL, Dialect};

/// The columns every fragment in this file is checked against.
fn columns() -> BTreeSet<ColumnName> {
    ["mrr_eur", "status", "customer_key", "region", "order_date", "churned"]
        .into_iter()
        .map(|raw| ColumnName::parse(raw).expect("a test column is a column"))
        .collect()
}

fn table() -> TableName {
    TableName::parse("fact_subscription").expect("a test table is a table")
}

fn authored(pairs: &[(&str, &str)]) -> AuthoredSql {
    let map: BTreeMap<DialectTag, SqlFragment> = pairs
        .iter()
        .map(|&(dialect, sql)| {
            (
                DialectTag::parse(dialect).expect("a test tag is a tag"),
                SqlFragment::parse(sql).expect("a test fragment is a fragment"),
            )
        })
        .collect();
    AuthoredSql::new(map).expect("a non-empty map is authored sql")
}

/// One portable fragment, compiled. The shape most of this file uses.
fn portable(sql: &str) -> Result<super::CompiledExpression, ExpressionError> {
    compile(&authored(&[("portable", sql)]), &table(), &columns())
}

/// The construct a fragment was refused for, or a panic naming what happened instead.
fn refusal(sql: &str) -> Construct {
    match portable(sql) {
        Err(ExpressionError::Refused { construct, .. }) => construct,
        Err(other) => panic!("{sql:?} was refused, but not for a construct: {other}"),
        Ok(compiled) => panic!("{sql:?} was ACCEPTED: {:?}", compiled.renderings()),
    }
}

#[test]
fn a_valid_fragment_compiles_for_every_dialect_this_build_renders_for() {
    // The whole feature in one assertion: the expression the repo owner asked for twice, the way a
    // wren cube carries it, rendered for all three targets with every identifier quoted and the
    // model's table put on every column.
    let compiled = portable("SUM(CASE WHEN status = 'active' THEN mrr_eur END)").expect("a conditional sum compiles");
    assert_eq!(compiled.renderings().len(), ALL.len());
    for dialect in ALL.iter().copied() {
        let rendering = compiled.for_dialect(dialect).expect("every dialect resolved");
        assert_eq!(rendering.authored_for().as_str(), "portable");
        assert_eq!(
            rendering.sql(),
            "SUM(CASE WHEN \"fact_subscription\".\"status\" = 'active' THEN \"fact_subscription\".\"mrr_eur\" END)",
            "{dialect}"
        );
    }
}

#[test]
fn a_cast_retargets_and_a_guarded_ratio_does_not_move() {
    // Two properties in one test because they are the same claim from both sides. The `DOUBLE` is
    // the dialect layer earning its place - each target spells the type differently and nobody
    // should maintain that by hand - and the ratio is the case where being byte-identical is the
    // correct answer, which is what makes the divisor guard checkable rather than a hope.
    let compiled =
        portable("CAST(SUM(mrr_eur) AS DOUBLE) / NULLIF(COUNT(DISTINCT customer_key), 0)").expect("a guarded ratio compiles");
    let sql = |dialect: Dialect| String::from(compiled.for_dialect(dialect).expect("resolved").sql());
    assert!(sql(Dialect::DuckDb).contains("AS DOUBLE)"), "{}", sql(Dialect::DuckDb));
    assert!(
        sql(Dialect::Postgres).contains("AS DOUBLE PRECISION)"),
        "{}",
        sql(Dialect::Postgres)
    );
    assert!(
        sql(Dialect::ClickHouse).contains("AS Nullable(Float64))"),
        "{}",
        sql(Dialect::ClickHouse)
    );
    for dialect in ALL.iter().copied() {
        assert!(
            sql(dialect).ends_with("/ NULLIF(COUNT(DISTINCT \"fact_subscription\".\"customer_key\"), 0)"),
            "{dialect}: {}",
            sql(dialect)
        );
    }
}

#[test]
fn a_window_function_and_a_percentile_are_the_reason_this_exists() {
    // Neither has a `Measure`, and neither can get one without turning the closed vocabulary into
    // an expression language. Both compile.
    for sql in [
        "SUM(mrr_eur) OVER (PARTITION BY region)",
        "PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY mrr_eur)",
        "MAX(mrr_eur) - MIN(mrr_eur)",
        "COALESCE(SUM(mrr_eur), 0)",
    ] {
        let compiled = portable(sql).unwrap_or_else(|err| panic!("{sql:?} should compile: {err}"));
        assert_eq!(compiled.renderings().len(), ALL.len(), "{sql:?}");
    }
}

#[test]
fn empty_whitespace_and_comment_only_input_is_refused_and_does_not_panic() {
    // THE ABORT RISK. The obvious fragment API - `Parser::new(dialect.tokenize(x))` then
    // `parse_expressions()` - panics on an empty token list, which is what all of these produce, in
    // every one of our dialects. `panic = "abort"` would make a blank line in a catalog file end
    // the process. Two things close it, and both are asserted: the domain refuses the blank ones
    // before they arrive, and the `SELECT` wrapper turns a comment-only fragment into a projection
    // count of zero rather than an empty token list.
    for raw in ["", " ", "   ", "\t", "\n"] {
        assert!(SqlFragment::parse(raw).is_err(), "{raw:?} is not a fragment");
    }
    for raw in ["-- nothing here", "/* nothing here */", "/*a*/ -- b"] {
        let fragment = SqlFragment::parse(raw).expect("a comment is text, so it reaches the compile");
        let err = compile(
            &AuthoredSql::new(BTreeMap::from([(DialectTag::portable(), fragment)])).expect("one fragment"),
            &table(),
            &columns(),
        )
        .expect_err("a comment is not an expression");
        assert!(
            matches!(
                err,
                ExpressionError::NotOneExpression {
                    shape: Shape::ManyExpressions,
                    ..
                }
            ),
            "{raw:?}: {err}"
        );
    }
}

#[test]
fn a_fragment_that_escapes_its_own_parentheses_is_refused_naming_a_position() {
    // The injection-shaped inputs, and the reason the authoring dialect may not be `ClickHouse`:
    // measured, ClickHouse's parser ACCEPTS `SUM(x))` and `x) FROM secret --`, silently dropping
    // the tail. DuckDB rejects both, and the reported column is moved back off the `SELECT `
    // wrapper so it points into the author's own line.
    for raw in [
        "x) FROM secret_table --",
        "SUM(mrr_eur))",
        "SUM(mrr_eur) garbage garbage",
        "1; DROP TABLE fact_subscription",
    ] {
        match portable(raw) {
            Err(ExpressionError::Unparsable { line, column, .. }) => {
                assert_eq!(line, 1, "{raw:?}");
                assert!(column >= 1, "{raw:?}: column {column} is off the author's line");
                assert!(column <= raw.len() + 1, "{raw:?}: column {column} is past the fragment");
            }
            Err(ExpressionError::NotOneExpression { shape, .. }) => {
                assert_eq!(shape, Shape::ManyStatements, "{raw:?}");
            }
            Err(other) => panic!("{raw:?}: unexpected refusal {other}"),
            Ok(_) => panic!("{raw:?} was ACCEPTED"),
        }
    }
}

#[test]
fn two_expressions_a_from_and_an_alias_are_each_their_own_refusal() {
    // Three different mistakes with three different fixes, so they get three different words. A
    // comma is an argument separator somebody put at the top level; a `FROM` is a whole query pasted
    // into a measure; an alias is a habit from writing `SELECT` lists.
    let shape = |raw: &str| match portable(raw) {
        Err(ExpressionError::NotOneExpression { shape, .. }) => shape,
        Err(other) => panic!("{raw:?}: unexpected refusal {other}"),
        Ok(_) => panic!("{raw:?} was ACCEPTED"),
    };
    assert_eq!(shape("SUM(mrr_eur), COUNT(customer_key)"), Shape::ManyExpressions);
    assert_eq!(shape("SUM(mrr_eur) FROM fact_subscription"), Shape::CarriedFrom);
    assert_eq!(shape("SUM(mrr_eur) AS revenue"), Shape::CarriedAlias);
}

#[test]
fn a_clause_that_taking_the_projection_would_discard_is_refused() {
    // The second hole the four shape guards do not close, and the worse of the two. `SELECT 1 WHERE
    // true` is legal in the authoring dialect with no `FROM`, so `SUM(x) WHERE secret = 1` is one
    // statement, one projection, no FROM and no alias - and taking `expressions[0]` DROPS the
    // `WHERE`. The metric would be certified as `SUM(x)` over a predicate its author wrote, silently.
    // Measured for every clause below: each parses, and each is dropped.
    let shape = |raw: &str| match portable(raw) {
        Err(ExpressionError::NotOneExpression { shape, .. }) => shape,
        Err(other) => panic!("{raw:?}: unexpected refusal {other}"),
        Ok(compiled) => panic!("{raw:?} was ACCEPTED as {:?}", compiled.renderings()),
    };
    for raw in [
        "SUM(mrr_eur) WHERE status = 'active'",
        "SUM(mrr_eur) GROUP BY region",
        "SUM(mrr_eur) HAVING SUM(mrr_eur) > 1",
        "SUM(mrr_eur) QUALIFY 1 = 1",
        "SUM(mrr_eur) ORDER BY region",
        "SUM(mrr_eur) LIMIT 1",
        "SUM(mrr_eur) WINDOW w AS (PARTITION BY region)",
        "DISTINCT SUM(mrr_eur)",
    ] {
        assert_eq!(shape(raw), Shape::CarriedClause, "{raw:?}");
    }
    // A set operation is not a `SELECT` at all, so it is caught one guard earlier.
    assert_eq!(shape("SUM(mrr_eur) UNION SELECT 1"), Shape::NotASelect);
}

#[test]
fn a_comment_inside_a_fragment_is_refused() {
    // A `--` comment is re-emitted as `/* .. */` INTO the statement, so catalog text would sit
    // between our own generated tokens. The generator does escape a closing `*/` into `* /` -
    // measured, so this is not an injection - but a measure has no reason to carry prose that the
    // document around it cannot hold instead.
    assert_eq!(refusal("SUM(mrr_eur) -- only the active ones"), Construct::Comment);
    assert_eq!(refusal("SUM(mrr_eur) -- */ , 1 AS injected /*"), Construct::Comment);
}

#[test]
fn the_four_denylisted_constructs_are_refused_and_the_refusal_names_them() {
    // Each of these renders as valid-looking SQL and nothing upstream errors on any of them. That is
    // the whole reason the list exists: `unsupported_level` defaults to `Warn`, which returns
    // `Ok(sql)` and pushes a diagnostic that `transpile` discards - and `Raise` errors on every
    // non-count aggregate targeting ClickHouse while staying silent on all four of these.
    assert_eq!(
        refusal("SUM(mrr_eur) FILTER (WHERE status = 'active')"),
        Construct::AggregateFilter
    );
    assert_eq!(refusal("COUNT(DISTINCT customer_key, region)"), Construct::RowConstructor);
    assert_eq!(refusal("DATE_TRUNC('month', order_date)"), Construct::DateTimeFunction);
    assert_eq!(refusal("SUM(mrr_eur) / COUNT(customer_key)"), Construct::UnguardedDivision);

    // And the fifth, which `docs/architecture.md` already recorded as not portable.
    assert_eq!(refusal("SUM(CASE WHEN churned IS TRUE THEN 1 END)"), Construct::IsTrue);
}

#[test]
fn a_date_or_time_function_is_refused_however_it_is_spelled() {
    // Two spellings, two mechanisms. `EXTRACT` parses to a typed node and is caught by kind;
    // `DATE_TRUNC`, `NOW` and `STRFTIME` parse to a GENERIC function node - measured - which the
    // generator emits verbatim into every target with no lowering at all, so the name is checked.
    for raw in [
        "SUM(CASE WHEN EXTRACT(YEAR FROM order_date) = 2026 THEN mrr_eur END)",
        "SUM(CASE WHEN order_date > NOW() THEN mrr_eur END)",
        "SUM(CASE WHEN STRFTIME(order_date, '%Y') = '2026' THEN mrr_eur END)",
        "SUM(CASE WHEN order_date = CURRENT_DATE THEN mrr_eur END)",
    ] {
        assert_eq!(refusal(raw), Construct::DateTimeFunction, "{raw:?}");
    }
}

#[test]
fn a_subquery_is_refused_even_though_every_shape_guard_passes_it() {
    // The hole the four shape guards do not close, and the one worth having found: a scalar subquery
    // in the projection is one statement, one expression, no FROM and no alias, so it passes all
    // four - and it reads a table the plan never granted. Measured before it was written down: the
    // wrapper accepted `SUM(x) + (SELECT secret FROM secret_table)`.
    assert_eq!(
        refusal("SUM(mrr_eur) + (SELECT 1)"),
        Construct::Query,
        "a subquery with no table is still a query"
    );
    assert_eq!(refusal("SUM(mrr_eur) + (SELECT secret FROM secret_table)"), Construct::Query);
    assert_eq!(
        refusal("SUM(CASE WHEN region IN (SELECT r FROM secret) THEN mrr_eur END)"),
        Construct::Query
    );
    assert_eq!(refusal("SUM(mrr_eur) + (SELECT MAX(x) FROM other)"), Construct::Query);
}

#[test]
fn a_star_a_placeholder_and_a_qualified_reference_are_refused() {
    // `COUNT(*)` is the one that needs its own arm: the star is a `bool` FIELD on the count node and
    // projects no `Star` node at all, measured, so a check that looked only for the node would pass
    // over the single spelling anybody writes.
    assert_eq!(refusal("COUNT(*)"), Construct::Star);
    assert_eq!(refusal("SUM(mrr_eur) / NULLIF(COUNT(*), 0)"), Construct::Star);
    assert_eq!(refusal("SUM(*)"), Construct::Star);
    // A placeholder would shift every parameter the plan bound after it.
    assert_eq!(
        refusal("SUM(CASE WHEN status = ? THEN mrr_eur END)"),
        Construct::BindParameter
    );
    // A hand-written qualifier can name a table the plan did not join. The compile qualifies every
    // column itself, against the metric's own model.
    assert_eq!(refusal("SUM(fact_subscription.mrr_eur)"), Construct::QualifiedColumn);
    assert_eq!(refusal("SUM(secret_table.mrr_eur)"), Construct::QualifiedColumn);
    // A schema-qualified call reaches a schema the model does not declare, and it is refused - as a
    // `dot`, because that is what the authoring dialect parses `secret.udf(x)` into rather than a
    // function node carrying a dotted name. The name check in `name_refusal` is the other arm and no
    // input reaches it in `DuckDB` today; it stays because the parse of a qualified call is an
    // upstream detail and the two arms cost one line each.
    assert_eq!(refusal("secret.udf(mrr_eur)"), Construct::Opaque);
    assert_eq!(refusal("SUM(secret.schema.mrr_eur)"), Construct::Opaque);
}

#[test]
fn a_fragment_that_aggregates_nothing_is_refused() {
    // Every question about a metric is grouped, so a bare column in the measure position is a
    // statement the data system rejects. Refusing here names the metric instead.
    assert_eq!(refusal("mrr_eur"), Construct::NotAggregated);
    assert_eq!(refusal("mrr_eur * 2"), Construct::NotAggregated);
}

#[test]
fn an_unknown_column_fails_the_load_naming_the_column_and_the_table() {
    // The decision this file exists to record: wren's cube path does not check column references and
    // its own documentation tells the agent to expect a runtime error; its model path does check
    // them, through a schema-driven rewrite. The model path is right, and the difference is a
    // refusal an operator can fix against a stack trace an agent shows a user.
    let err = portable("SUM(CASE WHEN status = 'active' THEN monthly_recurring_revenue END)")
        .expect_err("an undeclared column is not a column");
    match err {
        ExpressionError::UnknownColumn { column, table, .. } => {
            assert_eq!(column, "monthly_recurring_revenue");
            assert_eq!(table, table_named("fact_subscription"));
        }
        other => panic!("expected an unknown column, got {other}"),
    }
    // Including inside a window's PARTITION BY, which is a column reference like any other.
    assert!(matches!(
        portable("SUM(mrr_eur) OVER (PARTITION BY sales_territory)"),
        Err(ExpressionError::UnknownColumn { .. })
    ));
    // And a quoted name the model does not declare, which is how a column with a space would arrive.
    assert!(matches!(
        portable("SUM(\"weird col\")"),
        Err(ExpressionError::UnknownColumn { .. })
    ));
}

fn table_named(raw: &str) -> TableName {
    TableName::parse(raw).expect("a test table is a table")
}

#[test]
fn a_dialect_word_that_is_not_one_fails_the_load_rather_than_being_never_chosen() {
    // The failure mode this closes is the quiet one: with a `portable` fragment beside a misspelled
    // `postgresql`, resolution would hand Postgres the portable text and nothing anywhere would say
    // that the variant the author wrote for it was never read.
    let err = compile(
        &authored(&[("portable", "SUM(mrr_eur)"), ("postgresql", "SUM(mrr_eur)::double precision")]),
        &table(),
        &columns(),
    )
    .expect_err("postgresql is not a dialect this build renders for");
    match err {
        ExpressionError::UnknownDialect { tag, choices, .. } => {
            assert_eq!(tag, "postgresql");
            assert!(choices.contains("postgres"), "{choices}");
            assert!(choices.contains("clickhouse"), "{choices}");
        }
        other => panic!("expected an unknown dialect, got {other}"),
    }
}

#[test]
fn a_per_dialect_variant_is_chosen_over_portable_and_recorded() {
    // The escape hatch inside the escape hatch, and the traceability half. `sumIf` exists in
    // ClickHouse and nowhere else, `COUNT_IF` renders verbatim into Postgres where it does not
    // exist - which is exactly the class of thing a per-dialect variant is for.
    let compiled = compile(
        &authored(&[
            ("portable", "SUM(CASE WHEN status = 'active' THEN mrr_eur END)"),
            ("clickhouse", "sumIf(mrr_eur, status = 'active')"),
        ]),
        &table(),
        &columns(),
    )
    .expect("both variants compile");

    let click = compiled.for_dialect(Dialect::ClickHouse).expect("clickhouse resolved");
    assert_eq!(click.authored_for().as_str(), "clickhouse");
    assert!(click.sql().starts_with("sumIf("), "{}", click.sql());
    for dialect in [Dialect::DuckDb, Dialect::Postgres] {
        let rendering = compiled.for_dialect(dialect).expect("resolved");
        assert_eq!(rendering.authored_for().as_str(), "portable", "{dialect}");
        assert!(rendering.sql().starts_with("SUM(CASE"), "{}", rendering.sql());
    }
}

#[test]
fn a_dialect_with_no_variant_and_no_portable_fragment_is_refused_not_guessed() {
    // Where this departs from the importer it copies. Wren's OSI reader falls back to the first
    // non-empty variant, which hands a Postgres query a ClickHouse expression because it happened
    // to be listed first: a number computed by a definition nobody chose, under a certified name.
    let err = compile(
        &authored(&[("clickhouse", "sumIf(mrr_eur, status = 'active')")]),
        &table(),
        &columns(),
    )
    .expect_err("nothing was authored for duckdb or postgres");
    match err {
        ExpressionError::NoFragment {
            dialect, authored_for, ..
        } => {
            assert!(!matches!(dialect, Dialect::ClickHouse), "{dialect} was authored for");
            assert_eq!(authored_for, "clickhouse");
        }
        other => panic!("expected a missing fragment, got {other}"),
    }
}

#[test]
fn every_variant_is_checked_even_the_ones_a_dialect_would_never_read() {
    // A per-dialect variant is not a way around the checks. Compiling walks every dialect in `ALL`,
    // and the ClickHouse variant here is refused when ClickHouse's turn comes rather than being
    // carried unexamined because DuckDB and Postgres had a portable fragment.
    let err = compile(
        &authored(&[
            ("portable", "SUM(CASE WHEN status = 'active' THEN mrr_eur END)"),
            ("clickhouse", "sumIf(mrr_eur, status = 'active') FILTER (WHERE churned)"),
        ]),
        &table(),
        &columns(),
    )
    .expect_err("the clickhouse variant carries a FILTER");
    match err {
        ExpressionError::Refused { tag, construct } => {
            assert_eq!(tag, "clickhouse");
            assert_eq!(construct, Construct::AggregateFilter);
        }
        other => panic!("expected a refusal, got {other}"),
    }
}

#[test]
fn a_refusal_says_what_it_found_and_why() {
    // The message is the deliverable for whoever has to fix the file, so it is asserted rather than
    // assumed: the dialect word, the construct, and the reason in one line.
    let err = portable("SUM(mrr_eur) FILTER (WHERE status = 'active')").expect_err("FILTER is refused");
    let rendered = err.to_string();
    assert!(rendered.contains("portable"), "{rendered}");
    assert!(rendered.contains("FILTER (WHERE ..)"), "{rendered}");
    assert!(rendered.contains("never read anywhere"), "{rendered}");
    // Every construct carries both halves, so no refusal can be a bare name.
    for construct in [
        Construct::Query,
        Construct::TableReference,
        Construct::SchemaStatement,
        Construct::Star,
        Construct::BindParameter,
        Construct::Opaque,
        Construct::QualifiedColumn,
        Construct::QualifiedFunctionName,
        Construct::DateTimeFunction,
        Construct::AggregateFilter,
        Construct::Comment,
        Construct::RowConstructor,
        Construct::UnguardedDivision,
        Construct::IntegerDivision,
        Construct::IsTrue,
        Construct::NotAggregated,
    ] {
        assert!(!construct.as_str().is_empty(), "{construct:?}");
        assert!(construct.why().len() > 20, "{construct:?}");
        assert_eq!(construct.to_string(), construct.as_str());
    }
}

#[test]
fn a_compiled_expression_embeds_as_the_text_it_was_compiled_to() {
    // The query-path half, and the property that keeps parsing off it: what reaches a statement is
    // the string the compile produced, verbatim, wrapped so it cannot rebind under an operator.
    let compiled = portable("SUM(CASE WHEN status = 'active' THEN mrr_eur END)").expect("compiles");
    let rendering = compiled.for_dialect(Dialect::DuckDb).expect("resolved");
    let statement = polyglot_sql::builder::select(vec![embed(rendering)])
        .from("fact_subscription")
        .build();
    let sql = polyglot_sql::dialects::Dialect::get(polyglot_sql::DialectType::DuckDB)
        .generate(&statement)
        .expect("a statement holding a compiled expression renders");
    assert_eq!(
        sql,
        "SELECT (SUM(CASE WHEN \"fact_subscription\".\"status\" = 'active' THEN \"fact_subscription\".\"mrr_eur\" END)) FROM fact_subscription"
    );
}

#[test]
fn a_divisor_wrapped_in_parentheses_is_still_a_guard() {
    // The guard is structural rather than textual, so the parentheses a person adds for readability
    // do not turn an accepted fragment into a refused one.
    portable("SUM(mrr_eur) / (NULLIF(COUNT(DISTINCT customer_key), 0))").expect("a parenthesised guard is a guard");
    // And a divisor that is merely a literal is still unguarded: integer division truncates in two
    // of the three targets, so `SUM(cents) / 2` is a whole number where a ratio was meant.
    assert_eq!(refusal("SUM(mrr_eur) / 2"), Construct::UnguardedDivision);
}
