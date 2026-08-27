//! What the compile refuses, and what it renders.
//!
//! Every case here was reproduced against the dialect layer before it was written down, with
//! sutura's exact feature set. The refusal tests are the load-bearing half: each one is a construct
//! that produces valid-looking SQL and a different number, or reaches data the plan never granted,
//! and **nothing upstream errors on any of them**.

use std::collections::{BTreeMap, BTreeSet};

use sutura_domain::expression::{AuthoredSql, DialectTag, SqlFragment};
use sutura_domain::model::{ColumnName, TableName};

use super::vocabulary::{ALLOWED_FUNCTION_NAMES, Called};
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
fn a_function_name_outside_the_allowed_set_is_refused_and_the_refusal_names_it() {
    // THE REASON THE NAME CHECK IS AN ALLOWLIST, and `docs/adr/0004` records the reversal. A generic
    // call is emitted verbatim into every target with no lowering at all, so its name is unbounded
    // reach. Measured against DuckDB 1.5.5: the rendering of `MAX(getenv('X'))` is
    // `SELECT (MAX(GETENV('X'))) FROM fact_subscription`, and with `SUTURA_SECRET_PROBE` set in the
    // process that statement ANSWERED THE VALUE. Every secret the sutura process holds - a
    // service-account path, a warehouse password, a token - was readable that way, under a certified
    // metric name, out of a catalog file. `Construct::TableReference::why` states the invariant it
    // breaks: "a measure expression may read only the columns its own model declares".
    //
    // A denylist over names cannot bound that space. It is every function every target has, plus
    // every user-defined one, and the list below is a sample of one afternoon's reading of three
    // manuals. The set a MEASURE needs is short, so that is what is enumerated.
    for (raw, name) in [
        // DuckDB: reads the process environment, and reads engine configuration.
        ("MAX(getenv('SUTURA_SECRET_PROBE'))", "getenv"),
        ("SUM(mrr_eur) + LEN(getenv('HOME'))", "getenv"),
        ("MAX(current_setting('temp_directory'))", "current_setting"),
        // Postgres: reads an arbitrary file, lists a directory, writes one to disk.
        ("SUM(mrr_eur) + LENGTH(pg_read_file('/etc/passwd'))", "pg_read_file"),
        ("SUM(mrr_eur) + pg_ls_dir('/')", "pg_ls_dir"),
        ("SUM(mrr_eur) + lo_import('/etc/passwd')", "lo_import"),
        // Postgres: EXECUTES an arbitrary query and returns its rows.
        (
            "SUM(mrr_eur) + LENGTH(query_to_xml('SELECT * FROM secret_table', true, false, ''))",
            "query_to_xml",
        ),
        ("SUM(mrr_eur) + LENGTH(dblink('x', 'y'))", "dblink"),
        // Postgres: mutates a sequence, and holds the connection for as long as it likes.
        ("SUM(nextval('some_sequence'))", "nextval"),
        ("SUM(setval('some_sequence', 1))", "setval"),
        ("SUM(mrr_eur) + pg_sleep(1000000)", "pg_sleep"),
        // ClickHouse: a dictionary lookup, a setting, and its own sleep.
        ("MAX(dictGet('d', 'a', customer_key))", "dictGet"),
        ("MAX(getSetting('s'))", "getSetting"),
        ("SUM(mrr_eur) + sleep(3)", "sleep"),
        // A UDF, which the denylist allowed on the grounds that an author naming a dialect had taken
        // the claim. Portability is the author's claim to make; what the process can reach is not.
        ("SUM(our_own_udf(mrr_eur))", "our_own_udf"),
        // And the two spellings that turn an ARBITRARY name into an `AggregateFunction` rather than a
        // `Function` - measured in the authoring dialect's parser - so a check that looked only at
        // `Function` would have had a bypass here.
        ("getenv('SUTURA_SECRET_PROBE') IGNORE NULLS", "getenv"),
        ("getenv('SUTURA_SECRET_PROBE') WITHIN GROUP (ORDER BY mrr_eur)", "getenv"),
    ] {
        match portable(raw) {
            Err(ExpressionError::UnknownFunction { tag, name: found }) => {
                assert_eq!(found, name, "{raw:?}");
                assert_eq!(tag, "portable", "{raw:?}");
            }
            other => panic!("{raw:?}: expected an unknown function, got {other:?}"),
        }
    }
    // The refusal carries BOTH halves, which is why it is not a bare `Refused`: the name that is not
    // allowed, and the set that is.
    let rendered = portable("MAX(getenv('SUTURA_SECRET_PROBE'))")
        .expect_err("getenv is refused")
        .to_string();
    assert!(rendered.contains("getenv"), "{rendered}");
    assert!(rendered.contains("PERCENTILE_CONT"), "{rendered}");
    assert!(rendered.contains("NULLIF"), "{rendered}");
    // A date name keeps its own refusal, which is the better sentence for whoever has to fix the
    // file: the argument-order defect rather than "not on the list".
    assert_eq!(
        refusal("SUM(CASE WHEN order_date > NOW() THEN mrr_eur END)"),
        Construct::DateTimeFunction
    );
    // And a dotted name is still reported as the schema it reaches, not as an unknown name.
    assert_eq!(refusal("secret.udf(mrr_eur)"), Construct::Opaque);
}

#[test]
fn the_allowlist_is_the_one_place_a_callable_name_is_written_down() {
    // Three properties of the list itself, because it is the whole boundary now.
    //
    // Sorted, and strictly: the order is what makes a diff to it readable, and a duplicate carrying
    // two different marks would be decided by whichever line came first, silently.
    for (earlier, later) in ALLOWED_FUNCTION_NAMES.iter().zip(ALLOWED_FUNCTION_NAMES.iter().skip(1)) {
        assert!(earlier.0 < later.0, "{:?} is not sorted before {:?}", earlier.0, later.0);
    }
    for &(name, _) in ALLOWED_FUNCTION_NAMES {
        // Upper case, because the compare is `eq_ignore_ascii_case` and a lower-case entry would
        // read as if case mattered here.
        assert_eq!(name, name.to_uppercase(), "{name} is not written in upper case");
        // And unqualified, because a schema on a name is `Construct::QualifiedFunctionName` and
        // would never reach the list anyway.
        assert!(!name.contains('.'), "{name} is schema-qualified");
    }
    // And the refusal NAMES the allowed set, generated from the list rather than restated beside it:
    // a name added above appears in the message with no second edit, which is the whole reason the
    // list is declared through a macro.
    let why = Construct::UnknownFunction.why();
    for &(name, _) in ALLOWED_FUNCTION_NAMES {
        assert!(why.contains(name), "{name} is not in the refusal message: {why}");
    }
}

#[test]
fn an_aggregate_mark_agrees_with_the_dialect_layer_wherever_that_layer_has_an_opinion() {
    // The mark is DERIVED rather than kept by hand. For every name the allowlist calls an aggregate,
    // `NAME(mrr_eur)` is parsed in the authoring dialect and the dialect layer's own
    // `contains_aggregate` is asked - so where the two agree the mark restates nothing, and the
    // aggregate node kinds are written down in exactly one place, which is upstream.
    //
    // Where they disagree the name is listed here, so the exception is enumerated rather than
    // implicit. There is one, and it is the finding the mark exists for: `uniqExact` is a real
    // ClickHouse aggregate the dialect layer has no node for, so it arrived as a plain `Function`,
    // `contains_aggregate` said false, and a per-dialect variant nobody could write any other way
    // was refused as `NotAggregated`.
    const THE_DIALECT_LAYER_HAS_NO_NODE_FOR: &[&str] = &["UNIQEXACT"];
    let authoring = polyglot_sql::dialects::Dialect::get(polyglot_sql::DialectType::DuckDB);
    for &(name, called) in ALLOWED_FUNCTION_NAMES {
        if called != Called::Aggregate {
            continue;
        }
        let statements = authoring
            .parse(&format!("SELECT {name}(mrr_eur)"))
            .unwrap_or_else(|err| panic!("{name}(mrr_eur) should parse in the authoring dialect: {err}"));
        let upstream_knows = statements.iter().any(polyglot_sql::traversal::contains_aggregate);
        assert_eq!(
            upstream_knows,
            !THE_DIALECT_LAYER_HAS_NO_NODE_FOR.contains(&name),
            "{name}: the dialect layer says aggregate={upstream_knows}, and the exception list disagrees"
        );
    }
    // The scalar half is load-bearing in the other direction, so the mark is not decoration: a
    // scalar call over a bare column aggregates nothing, and every question about a metric is
    // grouped.
    assert_eq!(refusal("ROUND(mrr_eur, 2)"), Construct::NotAggregated);
    assert_eq!(refusal("GREATEST(mrr_eur, 0)"), Construct::NotAggregated);
    assert_eq!(refusal("ABS(mrr_eur)"), Construct::NotAggregated);
}

#[test]
fn a_dialect_aggregate_the_dialect_layer_has_no_node_for_still_counts_as_aggregating() {
    // `uniqExact` is ClickHouse's exact distinct count and has no portable spelling, which makes it
    // precisely the case a per-dialect variant exists for - and it was refused as "no aggregate",
    // because the classifier that answers that question works by node kind and there is no node.
    let compiled = compile(
        &authored(&[
            ("portable", "COUNT(DISTINCT customer_key)"),
            ("clickhouse", "uniqExact(customer_key)"),
        ]),
        &table(),
        &columns(),
    )
    .expect("a ClickHouse-only aggregate compiles for ClickHouse");
    let click = compiled.for_dialect(Dialect::ClickHouse).expect("clickhouse resolved");
    assert_eq!(click.authored_for().as_str(), "clickhouse");
    assert_eq!(click.sql(), "uniqExact(\"fact_subscription\".\"customer_key\")");
    // And it is still only a NAME on the allowlist: every other check is unchanged, so the column it
    // reads is checked against the model exactly as any other fragment's is.
    assert!(matches!(
        portable("uniqExact(sales_territory)"),
        Err(ExpressionError::UnknownColumn { .. })
    ));
    assert_eq!(refusal("uniqExact(*)"), Construct::Star);
}

#[test]
fn a_fragment_that_nests_deeper_than_the_checks_walk_is_refused_rather_than_ending_the_process() {
    // FOUR ORDINARY FRAGMENTS, each under the domain's own length bound, each of which ABORTED the
    // process. Two walks over a parsed fragment are ours and neither was guarded: `Expression`'s
    // derived `Clone`, run by `projection.clone()` at the end of `parse`, and `serde_json::to_value`
    // inside `carries`. A stack overflow is not a panic - `panic = "abort"` is beside the point,
    // there is nothing to catch - so the process simply died. Measured in a debug build on a 2 MiB
    // stack, which is what a tokio worker thread and a spawned std thread both have; in a release
    // build the same abort needs only 128 KiB, which is musl's default main-thread stack.
    for raw in [
        format!("SUM(mrr_eur){}", "+1".repeat(500)),
        format!("SUM({}1{})", "[".repeat(500), "]".repeat(500)),
        format!("SUM({}1)", "NOT ".repeat(250)),
        format!("SUM({}mrr_eur{})", "(".repeat(505), ")".repeat(505)),
    ] {
        assert!(
            SqlFragment::parse(&raw).is_ok(),
            "the domain accepts it, at {} characters",
            raw.chars().count()
        );
        match portable(&raw) {
            Err(ExpressionError::TooDeep { tag, depth, limit }) => {
                assert_eq!(tag, "portable");
                assert_eq!(limit, super::MAX_DEPTH);
                assert!(depth > limit, "{depth} is not deeper than {limit}");
            }
            other => panic!("expected a depth refusal, got {other:?}"),
        }
    }
    // The bound acts at the number written down rather than near it. Thirty parentheses is a tree of
    // depth 32 - the wrapper's `SELECT` and the `SUM` are the other two levels - and thirty-one is
    // 33, so one level under the limit compiles and one level over does not.
    let nest = |count: usize| format!("SUM({}mrr_eur{})", "(".repeat(count), ")".repeat(count));
    portable(&nest(30)).expect("a fragment at the limit compiles");
    assert!(matches!(
        portable(&nest(31)),
        Err(ExpressionError::TooDeep {
            depth: 33,
            limit: 32,
            ..
        })
    ));
}

#[test]
fn a_column_the_qualification_rewrite_did_not_reach_is_refused() {
    // The POSTCONDITION, and it is checked because the rewrite could not be trusted to have run.
    // `traversal::transform_map` dispatches on a hardcoded list of node kinds and returns a kind it
    // has no arm for untouched, children and all; the unknown-column check is a `dfs` walk, which
    // has the WIDER coverage. So the check saw columns the rewrite never touched, and each of these
    // eight compiled with a bare column in the output.
    //
    // Measured in DuckDB 1.5.5 for the COLLATE case, against a joined dimension carrying the same
    // column name: `Binder Error: Ambiguous reference to column name "region"` - at query time, for
    // a metric whose load succeeded. On an engine that resolves by precedence instead of erroring it
    // is silently the wrong number, which is what `qualify` exists to prevent.
    for raw in [
        "MAX(mrr_eur COLLATE NOCASE)",
        "SUM(mrr_eur) SIMILAR TO 'x'",
        "SUM(mrr_eur) WITHIN GROUP (ORDER BY region)",
        "ARRAY_AGG(mrr_eur ORDER BY region)",
        "LIST(mrr_eur ORDER BY region)",
        "GROUP_CONCAT(status ORDER BY region)",
        "FIRST_VALUE(mrr_eur IGNORE NULLS) OVER (PARTITION BY region)",
        "LAST_VALUE(mrr_eur) IGNORE NULLS OVER (PARTITION BY region)",
    ] {
        match portable(raw) {
            Err(ExpressionError::NotQualified { tag, column, table }) => {
                assert_eq!(tag, "portable", "{raw:?}");
                assert!(
                    columns().iter().any(|declared| declared.as_str() == column),
                    "{raw:?}: {column} is not one of the model's columns"
                );
                assert_eq!(table, table_named("fact_subscription"), "{raw:?}");
            }
            other => panic!("{raw:?}: expected an unqualified column, got {other:?}"),
        }
    }
    // And because it is a check on the OUTPUT rather than a second denylist of node kinds, every
    // fragment the rewrite DOES reach still compiles - including `WITHIN GROUP`, whose `order_by` is
    // rewritten while its `this` is not, which is why one of the two is here and the other is above.
    for raw in [
        "PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY mrr_eur)",
        "SUM(mrr_eur) OVER (PARTITION BY region)",
        "SUM(CASE WHEN status = 'active' THEN mrr_eur END)",
        "ARRAY_AGG(DISTINCT mrr_eur)",
    ] {
        let compiled = portable(raw).unwrap_or_else(|err| panic!("{raw:?} should still compile: {err}"));
        for dialect in ALL.iter().copied() {
            let sql = compiled.for_dialect(dialect).expect("resolved").sql();
            assert!(!sql.contains("(\"mrr_eur\""), "{raw:?} for {dialect}: {sql}");
        }
    }
}

#[test]
fn a_comment_is_refused_under_every_name_the_ast_spells_one() {
    // `carries(.., "trailing_comments")` asked about ONE field. The AST spells a comment seven ways
    // and three of the others are re-emitted INTO the statement - measured, all three accepted
    // before the question became a suffix match: `SUM(x) /* c */ + 1` through `left_comments`,
    // `SUM(x) + /* c */ 1` through `operator_comments`, and `CASE /* c */ WHEN ..` through
    // `comments`, which moves it to the end of the `CASE`. Up to a thousand characters of catalog
    // prose between our own generated tokens, under a refusal that read as though it held.
    //
    // Not an injection - `*/` and `/*` are both escaped on these paths, tried and confirmed - so
    // what was broken is the refusal and not the quoting.
    for raw in [
        "SUM(mrr_eur) /* left */ + 1",
        "SUM(mrr_eur) + /* operator */ 1",
        "CASE /* leading */ WHEN 1 = 1 THEN SUM(mrr_eur) END",
        "SUM(mrr_eur) -- trailing on a line\n + 1",
        "SUM(mrr_eur) /* trailing */",
    ] {
        assert_eq!(refusal(raw), Construct::Comment, "{raw:?}");
    }
}

#[test]
fn an_unterminated_comment_is_refused_rather_than_discarding_the_rest_of_the_fragment() {
    // `SUM(mrr_eur) /* note */` was refused as a comment. `SUM(mrr_eur) /* note` was ACCEPTED, with
    // everything after the `/*` swallowed by the tokenizer and nothing left in the tree to record
    // that it was there - the same shape as the dropped clause `Shape::CarriedClause` exists for,
    // arrived at through the tokenizer rather than the parser. Which is why the question is asked of
    // the TEXT: by the time there is an AST, the evidence is gone.
    assert_eq!(
        refusal("SUM(mrr_eur) /* actually we want SUM(customer_key) here"),
        Construct::Comment
    );
    assert_eq!(refusal("SUM(mrr_eur) */"), Construct::Comment);
    // The fail-closed caveat, asserted so that it is a decision on the record rather than a surprise
    // in a year: counting delimiters cannot tell a comment from a string literal holding one, so a
    // measure comparing a column against `/*` is refused too. A measure has no reason to.
    assert_eq!(refusal("SUM(CASE WHEN status = '/*' THEN mrr_eur END)"), Construct::Comment);
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
        Construct::UnknownFunction,
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
