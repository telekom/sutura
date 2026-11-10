use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::catalog::{Definitions, Description, JoinKey, JoinKeys, Model, Relationship};
use sutura_domain::model::Aggregate;
use sutura_domain::model::{ColumnName, Grain, JoinType, MetricName, ModelName, RelationshipName, SourceName, TableName};
use sutura_domain::plan::{
    PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin, QueryPlan,
    ResultLabel, StatementTables,
};
use sutura_domain::query::{Top, TopBy, TopDirection, TopN};
use sutura_domain::warehouse::ParamValue;
use sutura_domain::warehouse::cardinality::{DISTINCT_LABEL, DeclaredKey, ROWS_LABEL};

use polyglot_sql::builder;
use polyglot_sql::builder::Expr;
use polyglot_sql::expressions::{Expression, Parameter, ParameterStyle, Placeholder};

use super::{
    GenerateError, assert_placeholder_count, bucket_expression, generate, generate_key_probe, ordered_nulls_last, render,
};
use crate::dialect::{ALL, Dialect};

fn bucket(grain: Grain) -> PlanBucket {
    PlanBucket::new(
        ResultLabel::bucket(),
        grain,
        PlanColumn::new(
            TableName::parse("orders").expect("a test table is a table"),
            ColumnName::parse("order_date").expect("a test column is a column"),
        ),
    )
}

/// One bucket, rendered on its own, for each dialect.
fn rendered_bucket(dialect: Dialect) -> String {
    render(&bucket_expression(&bucket(Grain::Month), dialect).into_inner(), dialect)
        .expect("a bucket renders for every dialect this crate declares")
}

#[test]
fn clickhouse_sum_widens_integer_results_without_changing_float_or_decimal_sums() {
    let col = PlanColumn::new(
        TableName::parse("orders").expect("a test table is a table"),
        ColumnName::parse("amount").expect("a test column is a column"),
    );
    let term = sutura_domain::plan::PlanTerm::Aggregate {
        aggregate: Aggregate::Sum,
        column: col,
    };
    let clickhouse = render(
        &super::term_expression(&term, None, Dialect::ClickHouse, &mut Vec::new()).into_inner(),
        Dialect::ClickHouse,
    )
    .expect("the sum renders for ClickHouse");
    assert!(clickhouse.contains("toTypeName"), "{clickhouse}");
    assert!(!clickhouse.contains("Dynamic"), "{clickhouse}");
    assert!(
        !clickhouse.contains("toString"),
        "a text total would sort lexically in ORDER BY: {clickhouse}"
    );
    assert!(clickhouse.contains("toDecimal256"), "{clickhouse}");
    assert!(clickhouse.contains("Nullable(Decimal(76, 0))"), "{clickhouse}");
    for dialect in [Dialect::DuckDb, Dialect::Postgres, Dialect::BigQuery, Dialect::Oracle] {
        let rendered = render(
            &super::term_expression(&term, None, dialect, &mut Vec::new()).into_inner(),
            dialect,
        )
        .expect("the sum renders for this dialect");
        assert!(!rendered.contains("accurateCastOrNull"), "{dialect}: {rendered}");
    }
}

#[test]
fn a_clickhouse_ratio_reads_its_sums_as_one_double_each_without_a_tuple() {
    let sum_of = |column: &str| sutura_domain::plan::PlanTerm::Aggregate {
        aggregate: Aggregate::Sum,
        column: PlanColumn::new(
            TableName::parse("orders").expect("a test table is a table"),
            ColumnName::parse(column).expect("a test column is a column"),
        ),
    };
    let ratio = sutura_domain::plan::PlanMeasure::Ratio {
        numerator: sum_of("amount"),
        denominator: sum_of("quantity"),
        zero_denominator: sutura_domain::measure::ZeroDenominator::Null,
    };
    let sql = render(
        &super::measure_expression(&ratio, None, Dialect::ClickHouse, &mut Vec::new()).into_inner(),
        Dialect::ClickHouse,
    )
    .expect("the ratio renders for ClickHouse");
    assert!(!sql.contains("tupleElement"), "{sql}");
    assert!(!sql.contains("Dynamic"), "{sql}");
    assert_eq!(sql.matches("toFloat64(sum(accurateCastOrNull(").count(), 2, "{sql}");
    assert_eq!(sql.matches("toFloat64(sum(\"orders\"").count(), 2, "{sql}");
}

#[test]
fn clickhouse_integer_sum_uses_a_cast_that_can_return_null_for_float_rows() {
    let term = sutura_domain::plan::PlanTerm::Aggregate {
        aggregate: Aggregate::Sum,
        column: PlanColumn::new(
            TableName::parse("orders").expect("a test table is a table"),
            ColumnName::parse("amount").expect("a test column is a column"),
        ),
    };
    let sql = render(
        &super::term_expression(&term, None, Dialect::ClickHouse, &mut Vec::new()).into_inner(),
        Dialect::ClickHouse,
    )
    .expect("the sum renders for ClickHouse");
    assert!(
        sql.contains("sum(accurateCastOrNull(\"orders\".\"amount\", 'Int128'))"),
        "{sql}"
    );
    assert!(!sql.contains("toInt128("), "{sql}");
}

#[test]
fn the_layer_quotes_with_the_character_this_crate_declares() {
    // `Dialect::identifier_quote` is read by the golden suite to find the quoted spans in a
    // statement, so a wrong answer there does not fail loudly - it makes the *no identifier
    // reaches the statement unquoted* claim search for a character that is not in the statement,
    // which passes. Measured against a rendering that definitely contains an identifier.
    for &dialect in ALL {
        let sql = rendered_bucket(dialect);
        let quote = dialect.identifier_quote().character();
        let quoted = format!("{quote}orders{quote}");
        assert!(
            sql.contains(&quoted),
            "{dialect} declares {quote:?} as its identifier quote and the layer did not use it:\n{sql}"
        );
    }
}

#[test]
fn bigquery_gets_the_date_first_and_the_grain_as_a_bare_keyword() {
    // The declaration in `DateTruncShape`, asserted on the rendering rather than on the enum -
    // the enum is asserted in `dialect.rs`, and this is the half that says the match arm actually
    // builds what the arm claims.
    let sql = rendered_bucket(Dialect::BigQuery);
    assert!(sql.contains("DATE_TRUNC(`orders`.`order_date`, MONTH)"), "{sql}");
    // And the grain is NOT a string literal, which is the half that would otherwise render as a
    // statement BigQuery misreads rather than rejects.
    assert!(!sql.contains("'month'"), "{sql}");
}

#[test]
fn the_other_three_keep_the_grain_first_as_a_string_literal() {
    // The shape 63 existing goldens already carry. Pinned here too, because the fourth dialect's
    // arm is one edit away from changing all three. Oracle is not in this loop: its own
    // `DateFirstAsQuotedFormat` arm is a fourth shape and neither of the two the fixture below
    // still asserts, and it has its own cell in `oracle_gets_the_date_first_and_a_quoted_format`.
    for dialect in [Dialect::DuckDb, Dialect::Postgres, Dialect::ClickHouse] {
        let sql = rendered_bucket(dialect);
        assert!(sql.contains("'month'"), "{dialect}: {sql}");
        assert!(!sql.contains("MONTH"), "{dialect}: {sql}");
    }
}

#[test]
fn oracle_gets_the_date_first_and_a_quoted_format() {
    // The third shape, asserted on the rendering the same way `bigquery_gets_the_date_first_and_
    // the_grain_as_a_bare_keyword` is: the enum is `dialect.rs`'s claim, this is the claim that the
    // match arm in `bucket_expression` actually builds what the enum promises.
    let sql = rendered_bucket(Dialect::Oracle);
    assert!(sql.contains(r#"TRUNC("orders"."order_date", 'MM')"#), "{sql}");
    // Not `DATE_TRUNC` - the whole reason this shape exists rather than reusing one of the other
    // two, see `DateTruncShape::DateFirstAsQuotedFormat`.
    assert!(!sql.contains("DATE_TRUNC"), "{sql}");
    // And the format model is a STRING, not a bare keyword like BigQuery's - the half that would
    // otherwise be a syntax error at Oracle rather than merely a wrong bucket.
    assert!(!sql.contains(", MM)"), "{sql}");
}

#[test]
fn the_parse_check_cannot_tell_the_two_bucket_shapes_apart() {
    // **The measurement that makes `DateTruncShape` a declaration rather than a check**, and it
    // isolates ONE variable on purpose. The golden suite parses every statement in the dialect it
    // was generated for, which is the closest thing we have to "this is valid there". What that
    // check cannot see is the ARGUMENT ORDER of a function call.
    //
    // So both shapes are rendered for BigQuery - same target, same backtick quoting, differing
    // only in the bucket - and both parse. A BigQuery statement built with the grain-first arm
    // would therefore render, parse and snapshot green here, and be REJECTED by BigQuery on the
    // first real question: `DateTruncShape` carries why it is a rejection rather than a wrong
    // number, which is the better of the two failures and still not one CI would have found.
    //
    // The first version of this test compared the DuckDB-RENDERED string against the BigQuery
    // parser and failed, which looked like the parse check catching the bug and was a different
    // finding entirely: double-quoted identifiers are string literals in `GoogleSQL`, so that
    // string fails on `"orders"."order_date"` before the bucket is reached. Two variables at
    // once, and the quoting one is covered by the test below.
    let wrong_shape_for_bigquery = render(
        &builder::func(
            "DATE_TRUNC",
            vec![builder::lit("month"), super::column(bucket(Grain::Month).column())],
        )
        .cast("DATE")
        .into_inner(),
        Dialect::BigQuery,
    )
    .expect("the grain-first shape renders for bigquery too - that is the problem");
    let right_shape_for_bigquery = rendered_bucket(Dialect::BigQuery);

    assert_ne!(
        wrong_shape_for_bigquery, right_shape_for_bigquery,
        "the two shapes have to differ or this test compares a string with itself"
    );
    for (label, sql) in [
        ("the grain-first shape", &wrong_shape_for_bigquery),
        ("the date-first shape", &right_shape_for_bigquery),
    ] {
        let parsed = polyglot_sql::parse(sql, super::dialect_type(Dialect::BigQuery));
        assert!(
            parsed.is_ok(),
            "{label} did not parse as bigquery, which would mean the parse check has grown teeth \
             this test says it has not - read that as good news and re-scope the claim in \
             AGENTS.md rather than weakening this assertion: {:?}\n{sql}",
            parsed.err()
        );
    }
}

#[test]
fn every_order_by_states_nulls_last() {
    // **The measurement behind the null-placement fix, pinned against the layer rather than
    // taken from it.** The layer OMITS `NULLS LAST` where it is already the target's fixed
    // default (Postgres, `ClickHouse`), so those two keep rendering no keyword even though the
    // generated AST carries `nulls_first: Some(false)` - and `BigQuery`'s default is the other
    // way (nulls first), which is exactly why it is one of the two that must see the keyword.
    //
    // `DuckDB` joined `BigQuery` in the `polyglot-sql` release #589 bumped to: its null ordering
    // moved out of the layer's fixed-default table (`generator.rs`'s own comment: "DuckDB
    // deliberately isn't in the default-elision group... its NULL ordering is configurable at
    // runtime"), so the layer now states the keyword for `DuckDB` too rather than assuming its
    // session default. Measured directly against the version before that bump and the one after:
    // `DuckDB` used to omit it like Postgres and `ClickHouse`, and now does not - same SQL
    // semantics (`DuckDB`'s actual default is still `NULLS LAST`), more explicit text. Saying
    // "all five render it" would still be false about the three that keep omitting, and this test
    // is what keeps the honest claim. Oracle joins the omitting group for an ASCENDING sort - its
    // default is `NULLS LAST` for ASC, same as Postgres - and the layer's own comment names it
    // alongside Postgres and Redshift as the dialects where nulls sort large.
    let column = super::column(&PlanColumn::new(
        TableName::parse("orders").expect("a test table is a table"),
        ColumnName::parse("customer_key").expect("a test column is a column"),
    ));
    let ordered = ordered_nulls_last(column);
    let ast = builder::select(vec![builder::col("customer_key")])
        .from("orders")
        .order_by(vec![ordered])
        .build();
    for &dialect in ALL {
        let sql = render(&ast, dialect).expect("an ORDER BY renders");
        assert!(sql.contains("ORDER BY"), "{dialect}: {sql}");
        match dialect {
            // BigQuery's default puts nulls first; DuckDB's null ordering is no longer treated
            // as fixed by the layer. Foundry uses Spark's nulls-first default too.
            Dialect::BigQuery | Dialect::DuckDb | Dialect::Foundry => {
                assert!(sql.contains("NULLS LAST"), "{dialect}: {sql}");
            }
            // `NULLS LAST` is still these dialects' own fixed default for an ascending sort, so
            // the layer collapses it away. The placement is still stated in the AST
            // (`ordered_nulls_last`) and this arm confirms the collapse is the layer's doing
            // rather than our omission.
            Dialect::Postgres | Dialect::ClickHouse | Dialect::Oracle => {
                assert!(
                    !sql.contains("NULLS"),
                    "{dialect} unexpectedly spelled a null placement: {sql}"
                );
            }
        }
    }
}

#[test]
fn a_week_bucket_asks_bigquery_for_the_monday_based_part() {
    // **The arm no golden covers, and the one where the obvious mapping is a wrong number.** The
    // example corpus asks only `day` and `month`, so nothing in `tests/snapshots` would notice if
    // this changed - which is why it is pinned here by value.
    //
    // BigQuery's `WEEK` begins on SUNDAY. The pinned DuckDB answers
    // `DATE_TRUNC('week', DATE '2026-08-30')` - a Sunday - with 2026-08-24, a Monday, so it puts
    // that Sunday in the previous week. `ISOWEEK` is BigQuery's Monday-based part, and asking for
    // it is what makes the two agree about which period a row belongs to.
    let sql = render(
        &bucket_expression(&bucket(Grain::Week), Dialect::BigQuery).into_inner(),
        Dialect::BigQuery,
    )
    .expect("a week bucket renders");
    assert!(sql.contains("ISOWEEK"), "{sql}");
    assert!(
        !sql.contains(", WEEK)"),
        "a bare WEEK is Sunday-based on this target and disagrees with every other dialect: {sql}"
    );

    // And the other four are unambiguous, so they are spelled plainly. Asserted together so a
    // future edit cannot quietly give one of them the ISO treatment it does not need.
    for (grain, expected) in [
        (Grain::Day, "DAY"),
        (Grain::Month, "MONTH"),
        (Grain::Quarter, "QUARTER"),
        (Grain::Year, "YEAR"),
    ] {
        let rendered = render(
            &bucket_expression(&bucket(grain), Dialect::BigQuery).into_inner(),
            Dialect::BigQuery,
        )
        .expect("a bucket renders");
        assert!(rendered.contains(expected), "{grain:?}: {rendered}");
    }
}

#[test]
fn the_parse_check_does_catch_the_wrong_quote_character() {
    // The other half, and it is the good news the test above must not be read as denying. A
    // statement rendered with double quotes does NOT parse as BigQuery: in `GoogleSQL` a double
    // quote opens a string, so `"orders"."order_date"` is a literal followed by a dot and the
    // parser stops there. So the quoting half of a mis-targeted statement IS caught by the
    // existing corpus, and only the bucket's shape needs a declaration to stand behind it.
    let double_quoted = rendered_bucket(Dialect::DuckDb);
    let parsed = polyglot_sql::parse(&double_quoted, super::dialect_type(Dialect::BigQuery));
    assert!(
        parsed.is_err(),
        "a double-quoted identifier parsed as bigquery, so the corpus is not catching the \
         quoting half either:\n{double_quoted}"
    );
}

/// The key probe, rendered and parse-checked for every dialect this crate compiles for.
///
/// **Here rather than in the golden suite, and the reason is what a golden could add.** A golden
/// pins one statement's TEXT per dialect, which is worth having where a plan has shapes to
/// enumerate; this statement has one shape and no parameters, so what is worth holding is that
/// every dialect renders it, that the target's own parser accepts it, and that the two aliases
/// are the domain's constants rather than this file's literals - an adapter reads the counts back
/// by those names, so a drift between them and the statement is a probe that answers nothing.
///
/// **The limit this shares with every parse check here:** it parses and stops.
/// `the_parse_check_cannot_tell_the_two_bucket_shapes_apart` above is the measurement of how far
/// that reaches, and the `DuckDB` and Postgres cells of `just test` are what execute one.
#[test]
fn a_key_probe_renders_and_parses_for_every_dialect_it_declares() {
    let relationship = Relationship::new(
        RelationshipName::parse("orders_customer").expect("a test relationship is a relationship"),
        ModelName::parse("orders").expect("a test model is a model"),
        ModelName::parse("customers").expect("a test model is a model"),
        JoinType::ManyToOne,
        JoinKeys::of(vec![JoinKey::Equal {
            origin: ColumnName::parse("customer_key").expect("a test column is a column"),
            target: ColumnName::parse("customer_key").expect("a test column is a column"),
        }])
        .expect("a test relationship declares one key"),
    );
    let definitions = Definitions::assemble(
        vec![
            Model::new(
                ModelName::parse("orders").expect("a test model is a model"),
                SourceName::parse("local").expect("a test source is a source"),
                TableName::parse("orders").expect("a test table is a table"),
                std::iter::once("customer_key").map(|c| ColumnName::parse(c).expect("a test column is a column")),
                Description::default(),
            ),
            Model::new(
                ModelName::parse("customers").expect("a test model is a model"),
                SourceName::parse("local").expect("a test source is a source"),
                TableName::parse("dim_customer").expect("a test table is a table"),
                std::iter::once("customer_key").map(|c| ColumnName::parse(c).expect("a test column is a column")),
                Description::default(),
            ),
        ],
        vec![relationship.clone()],
        Vec::new(),
    )
    .expect("two models and one join are consistent");
    let key = DeclaredKey::promised_by(&relationship, &definitions).expect("a many-to-one promises a unique target");

    for &dialect in ALL {
        let query =
            generate_key_probe(&key, dialect).unwrap_or_else(|e| panic!("a key probe would not render for {dialect}: {e}"));
        let sql = query.sql();
        assert!(query.params().is_empty(), "{dialect} bound a parameter into a probe: {sql}");
        assert!(sql.contains(ROWS_LABEL), "{dialect} did not alias the row count: {sql}");
        assert!(
            sql.contains(DISTINCT_LABEL),
            "{dialect} did not alias the distinct count: {sql}"
        );
        assert!(sql.contains("DISTINCT"), "{dialect} counted every value twice: {sql}");
        // The declaration is unconditional, so a probe that narrowed itself would answer a
        // different question than the one the join path spends.
        for absent in ["WHERE", "GROUP BY", "HAVING", "LIMIT"] {
            assert!(!sql.contains(absent), "{dialect} narrowed the probe with {absent}: {sql}");
        }
        let parsed = polyglot_sql::parse(sql, super::dialect_type(dialect));
        assert!(
            parsed.is_ok(),
            "{dialect} did not parse its own probe: {:?}\n{sql}",
            parsed.err()
        );
    }
}

/// A one-metric plan at month grain, bounded by the row cap: `top` is unset.
fn month_plan() -> QueryPlan {
    let col = |name: &str| {
        PlanColumn::new(
            TableName::parse("orders").expect("a test table is a table"),
            ColumnName::parse(name).expect("a test column is a column"),
        )
    };
    let date = |raw: &str| Date::parse(raw).expect("a test date is a date");
    let metric = MetricName::parse("revenue").expect("a test metric is a metric");
    let bindings = PlanBindings::parse(
        vec![
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::AtOrAfter {
                    column: col("order_date"),
                    param: 0,
                },
            ),
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::Before {
                    column: col("order_date"),
                    param: 1,
                },
            ),
        ],
        vec![ParamValue::Date(date("2026-06-01")), ParamValue::Date(date("2026-07-01"))],
    )
    .expect("two range bounds bind in placeholder order");
    QueryPlan::new(
        SourceName::parse("local").expect("a test source is a source"),
        metric.clone(),
        StatementTables::parse(TableName::parse("orders").expect("a test table is a table"), Vec::new())
            .expect("one table is unambiguous"),
        PlanBucket::new(ResultLabel::bucket(), Grain::Month, col("order_date")),
        Vec::new(),
        PlanMeasure::Simple {
            term: PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: col("amount_cents"),
            },
        },
        ResultLabel::measure(&metric),
        bindings,
        TimeRange::new(date("2026-06-01"), date("2026-07-01")).expect("a test range is a range"),
    )
}

/// Oracle ends in `FETCH FIRST n ROWS ONLY` and carries no `LIMIT`; every other dialect ends in
/// `LIMIT n` and carries no `FETCH FIRST`.
fn assert_row_limit_clause(plan: &QueryPlan, n: u32) {
    for &dialect in ALL {
        let query = generate(plan, dialect).unwrap_or_else(|e| panic!("a plan renders for {dialect}: {e}"));
        let sql = query.sql();
        if dialect == Dialect::Oracle {
            assert!(
                sql.ends_with(&format!("FETCH FIRST {n} ROWS ONLY")),
                "Oracle takes FETCH FIRST {n}:\n{sql}"
            );
            assert!(!sql.contains("LIMIT"), "Oracle refuses LIMIT:\n{sql}");
        } else {
            assert!(sql.ends_with(&format!("LIMIT {n}")), "{dialect} keeps LIMIT {n}:\n{sql}");
            assert!(!sql.contains("FETCH FIRST"), "{dialect} takes no FETCH FIRST:\n{sql}");
        }
    }
}

/// Oracle refuses `LIMIT n` (`ORA-03049`), so the row cap's one-past limit renders as
/// `FETCH FIRST`. Red on the base tree, where every whole-plan statement ends in `LIMIT n`.
/// `github.com/telekom/sutura#127`.
#[test]
fn oracle_caps_a_whole_plan_with_fetch_first_not_limit() {
    assert_row_limit_clause(&month_plan(), 10_001);
}

/// The `top` arm sets its own count through the same limit, so it renders as `FETCH FIRST` too.
/// `github.com/telekom/sutura#127`.
#[test]
fn oracle_caps_a_top_plan_with_fetch_first_not_limit() {
    let top = Top::new(
        TopN::parse(3).expect("three is a row count"),
        TopBy::Metric,
        TopDirection::Desc,
    );
    assert_row_limit_clause(&month_plan().with_top(top), 3);
}

/// The relationship and definitions of a two-column `many_to_one` key, whose target is `subscription_snapshot`.
fn compound_usage_subscription() -> (Relationship, Definitions) {
    let relationship = Relationship::new(
        RelationshipName::parse("usage_subscription").expect("a test relationship is a relationship"),
        ModelName::parse("daily_usage").expect("a test model is a model"),
        ModelName::parse("subscription_snapshot").expect("a test model is a model"),
        JoinType::ManyToOne,
        JoinKeys::of(vec![
            JoinKey::Equal {
                origin: ColumnName::parse("subscription_key").expect("a test column is a column"),
                target: ColumnName::parse("subscription_key").expect("a test column is a column"),
            },
            JoinKey::TruncatedEqual {
                origin: ColumnName::parse("usage_date").expect("a test column is a column"),
                grain: Grain::Month,
                target: ColumnName::parse("month").expect("a test column is a column"),
            },
        ])
        .expect("two keys is a non-empty set"),
    );
    let definitions = Definitions::assemble(
        vec![
            Model::new(
                ModelName::parse("daily_usage").expect("a test model is a model"),
                SourceName::parse("local").expect("a test source is a source"),
                TableName::parse("daily_usage").expect("a test table is a table"),
                ["subscription_key", "usage_date"]
                    .into_iter()
                    .map(|c| ColumnName::parse(c).expect("a test column is a column")),
                Description::default(),
            ),
            Model::new(
                ModelName::parse("subscription_snapshot").expect("a test model is a model"),
                SourceName::parse("local").expect("a test source is a source"),
                TableName::parse("subscription_snapshot").expect("a test table is a table"),
                ["subscription_key", "month"]
                    .into_iter()
                    .map(|c| ColumnName::parse(c).expect("a test column is a column")),
                Description::default(),
            ),
        ],
        vec![relationship.clone()],
        Vec::new(),
    )
    .expect("two models and one compound join are consistent");
    (relationship, definitions)
}

/// A compound (two-column) declared key probe parses on `DuckDb`, `Postgres` and `ClickHouse`, and
/// `BigQuery` refuses it by name - the negative half of
/// `a_key_probe_renders_and_parses_for_every_dialect_it_declares`, which only ever built a one-key
/// probe and so never reached the tuple `DISTINCT`. Oracle renders its own shape, held by
/// `an_oracle_compound_key_probe_counts_a_grouped_subquery_not_a_multi_column_distinct`.
#[test]
fn a_compound_key_probe_renders_only_where_a_null_safe_tuple_distinct_exists() {
    let (relationship, definitions) = compound_usage_subscription();
    let key = DeclaredKey::promised_by(&relationship, &definitions).expect("a many-to-one promises a unique target");

    for &dialect in ALL {
        let outcome = generate_key_probe(&key, dialect);
        if dialect == Dialect::Oracle {
            assert!(
                outcome.is_ok(),
                "Oracle renders its grouped-subquery shape, not a refusal: {outcome:?}"
            );
            continue;
        }
        if matches!(dialect, Dialect::BigQuery | Dialect::Foundry) {
            assert!(
                matches!(outcome, Err(GenerateError::CompoundKeyProbeUnsupported { dialect: refused }) if refused == dialect),
                "{dialect} must refuse a compound probe by name, not render one: {outcome:?}"
            );
            continue;
        }
        let query = outcome.unwrap_or_else(|e| panic!("a compound key probe would not render for {dialect}: {e}"));
        let sql = query.sql();
        assert!(
            query.params().is_empty(),
            "{dialect} bound a parameter into a compound probe: {sql}"
        );
        assert!(sql.contains(ROWS_LABEL), "{dialect} did not alias the row count: {sql}");
        assert!(
            sql.contains(DISTINCT_LABEL),
            "{dialect} did not alias the distinct count: {sql}"
        );
        // Both key columns must appear beside DISTINCT, not just the origin's: a probe that
        // dropped the second column would silently validate a plain-equality's worth of
        // uniqueness under a compound key's name.
        assert!(
            sql.contains("subscription_key"),
            "{dialect} dropped the first key column: {sql}"
        );
        assert!(sql.contains("month"), "{dialect} dropped the second key column: {sql}");
        for absent in ["WHERE", "GROUP BY", "HAVING", "LIMIT"] {
            assert!(
                !sql.contains(absent),
                "{dialect} narrowed the compound probe with {absent}: {sql}"
            );
        }
        let parsed = polyglot_sql::parse(sql, super::dialect_type(dialect));
        assert!(
            parsed.is_ok(),
            "{dialect} did not parse its own compound probe: {:?}\n{sql}",
            parsed.err()
        );
    }
}

/// Oracle's `DISTINCT` takes one expression, so a compound key is counted over a grouped subquery:
/// no multi-column `DISTINCT`, both key columns in the `GROUP BY`, a null in either column excluded
/// by the `WHERE` as the other dialects' counts exclude it, and the domain's two labels.
///
/// **The limit:** this reads the rendered text and parses it offline. The `oracle-tier` CI job is the
/// one venue that executes the shape, through `data_systems::oracle::whether_it_counts_a_declared_join_key…`,
/// and only over the example corpus's keys, all duplicate-free. Whether Oracle answers the count a
/// DUPLICATED key table must is not measured by any job.
#[test]
fn an_oracle_compound_key_probe_counts_a_grouped_subquery_not_a_multi_column_distinct() {
    let (relationship, definitions) = compound_usage_subscription();
    let key = DeclaredKey::promised_by(&relationship, &definitions).expect("a many-to-one promises a unique target");
    let query = generate_key_probe(&key, Dialect::Oracle).expect("a compound probe renders for Oracle");
    let sql = query.sql();
    assert!(
        query.params().is_empty(),
        "a parameter was bound into a compound probe: {sql}"
    );
    assert!(!sql.contains("DISTINCT"), "Oracle's DISTINCT takes one expression: {sql}");
    let (first, second) = (
        "\"subscription_snapshot\".\"subscription_key\"",
        "\"subscription_snapshot\".\"month\"",
    );
    assert!(
        sql.contains(&format!(
            "WHERE NOT {first} IS NULL AND NOT {second} IS NULL GROUP BY {first}, {second}"
        )),
        "a tuple with a null column is counted by neither side, and every column is grouped: {sql}"
    );
    assert!(
        sql.contains("SUM(\"key_groups\".\"group_rows\")"),
        "the rows are the groups' sizes: {sql}"
    );
    assert!(
        sql.contains("COUNT(*) AS \"0_key_distinct\""),
        "the distinct count is the groups: {sql}"
    );
    assert!(
        sql.contains(ROWS_LABEL) && sql.contains(DISTINCT_LABEL),
        "the domain's labels: {sql}"
    );
    assert!(sql.contains("FROM (SELECT COUNT(*)"), "the groups are a subquery: {sql}");
    assert!(
        !sql.contains(" AS \"key_groups\""),
        "Oracle takes no AS before a table alias: {sql}"
    );
    let parsed = polyglot_sql::parse(sql, super::dialect_type(Dialect::Oracle));
    assert!(
        parsed.is_ok(),
        "Oracle did not parse its own compound probe: {:?}\n{sql}",
        parsed.err()
    );
}

/// One key column keeps its `COUNT(DISTINCT col)` shape on Oracle: only a compound key needs the
/// grouped subquery, and a probe that regrouped a single key would change a statement the boot check
/// already spends.
#[test]
fn an_oracle_single_key_probe_keeps_its_count_distinct() {
    let relationship = Relationship::new(
        RelationshipName::parse("orders_customer").expect("a test relationship is a relationship"),
        ModelName::parse("orders").expect("a test model is a model"),
        ModelName::parse("customers").expect("a test model is a model"),
        JoinType::ManyToOne,
        JoinKeys::of(vec![JoinKey::Equal {
            origin: ColumnName::parse("customer_key").expect("a test column is a column"),
            target: ColumnName::parse("customer_key").expect("a test column is a column"),
        }])
        .expect("a test relationship declares one key"),
    );
    let model = |name: &str, table: &str| {
        Model::new(
            ModelName::parse(name).expect("a test model is a model"),
            SourceName::parse("local").expect("a test source is a source"),
            TableName::parse(table).expect("a test table is a table"),
            std::iter::once("customer_key").map(|c| ColumnName::parse(c).expect("a test column is a column")),
            Description::default(),
        )
    };
    let definitions = Definitions::assemble(
        vec![model("orders", "orders"), model("customers", "dim_customer")],
        vec![relationship.clone()],
        Vec::new(),
    )
    .expect("two models and one join are consistent");
    let key = DeclaredKey::promised_by(&relationship, &definitions).expect("a many-to-one promises a unique target");
    let query = generate_key_probe(&key, Dialect::Oracle).expect("a one-key probe renders for Oracle");
    assert!(
        query.sql().contains("COUNT(DISTINCT \"dim_customer\".\"customer_key\")"),
        "{}",
        query.sql()
    );
    assert!(!query.sql().contains("GROUP BY"), "{}", query.sql());
}

/// An Oracle ratio's numerator is `TO_BINARY_DOUBLE(..)`, never a cast whose target polyglot writes as
/// `DOUBLE PRECISION` - a `NUMBER` in Oracle, so the division stays exact and reads back as text. Every
/// other dialect keeps its `CAST(.. AS <double>)`.
#[test]
fn an_oracle_ratio_converts_its_numerator_with_to_binary_double_and_others_keep_the_cast() {
    let sum_of = |column: &str| sutura_domain::plan::PlanTerm::Aggregate {
        aggregate: Aggregate::Sum,
        column: PlanColumn::new(
            TableName::parse("orders").expect("a test table is a table"),
            ColumnName::parse(column).expect("a test column is a column"),
        ),
    };
    let ratio = sutura_domain::plan::PlanMeasure::Ratio {
        numerator: sum_of("amount"),
        denominator: sum_of("quantity"),
        zero_denominator: sutura_domain::measure::ZeroDenominator::Null,
    };
    let rendered = |dialect| {
        render(
            &super::measure_expression(&ratio, None, dialect, &mut Vec::new()).into_inner(),
            dialect,
        )
        .expect("the ratio renders for this dialect")
    };
    let oracle = rendered(Dialect::Oracle);
    assert!(
        oracle.starts_with("TO_BINARY_DOUBLE(SUM(\"orders\".\"amount\")) / NULLIF("),
        "{oracle}"
    );
    assert!(!oracle.contains("DOUBLE PRECISION"), "{oracle}");
    for dialect in [Dialect::DuckDb, Dialect::Postgres] {
        let sql = rendered(dialect);
        assert!(
            sql.starts_with("CAST(SUM(\"orders\".\"amount\") AS DOUBLE"),
            "{dialect}: {sql}"
        );
        assert!(!sql.contains("TO_BINARY_DOUBLE"), "{dialect}: {sql}");
    }
}

/// An Oracle mean is `TO_BINARY_DOUBLE(AVG(..))`: `AVG` over a `NUMBER` is a `NUMBER`, which the adapter
/// reads back as exact text where the engine answers a double. Postgres keeps its `CAST(.. AS DOUBLE
/// PRECISION)`, for the same reason, and the other dialects keep the bare `AVG`.
#[test]
fn an_oracle_mean_is_read_as_a_binary_double_and_postgres_keeps_its_cast() {
    let mean = PlanMeasure::Simple {
        term: PlanTerm::Aggregate {
            aggregate: Aggregate::Avg,
            column: PlanColumn::new(
                TableName::parse("orders").expect("a test table is a table"),
                ColumnName::parse("amount").expect("a test column is a column"),
            ),
        },
    };
    let rendered = |dialect| {
        render(
            &super::measure_expression(&mean, None, dialect, &mut Vec::new()).into_inner(),
            dialect,
        )
        .expect("the mean renders for this dialect")
    };
    assert_eq!(rendered(Dialect::Oracle), "TO_BINARY_DOUBLE(AVG(\"orders\".\"amount\"))");
    let postgres = rendered(Dialect::Postgres);
    assert!(
        postgres.starts_with("CAST(AVG(\"orders\".\"amount\") AS DOUBLE"),
        "{postgres}"
    );
    for dialect in [Dialect::DuckDb, Dialect::BigQuery, Dialect::ClickHouse] {
        let sql = rendered(dialect);
        assert!(!sql.contains("TO_BINARY_DOUBLE") && !sql.contains("CAST"), "{dialect}: {sql}");
    }
}

/// A statement whose placeholder count differs from its bound-parameter count is refused, rather
/// than sent to a data system with too many or too few bind values.
///
/// The mismatch is the shape a future guard-embedding change could produce if the `emitted` list
/// and the AST diverged: two `?` placeholders in the AST but a one-element parameter list. The
/// `DuckDb` dialect writes `?` (`PlaceholderStyle::Question`), so the walk counts
/// `Expression::Placeholder` nodes.
#[test]
fn a_statement_whose_placeholder_count_differs_from_its_params_is_refused() {
    // An AST with two Placeholder nodes but a one-element params list.
    let placeholder = || Expr(Expression::Placeholder(Placeholder { index: None }));
    let ast = builder::select(vec![
        builder::col("t.c").eq(placeholder()),
        builder::col("t.d").eq(placeholder()),
    ])
    .from("t")
    .build();
    let dialect = Dialect::DuckDb;
    let params = vec![ParamValue::Text("one".into())]; // 1 param, 2 placeholders

    let result = assert_placeholder_count(&ast, dialect, &params);
    assert!(
        matches!(
            result,
            Err(GenerateError::PlaceholderCountMismatch {
                placeholders: 2,
                params: 1,
                ..
            })
        ),
        "a mismatched placeholder count must be refused, got {result:?}"
    );
}

/// A `Numbered` (Postgres) statement that reuses `$1` for one parameter - the guarded-ratio shape
/// a count-only check wrongly refuses - is accepted, because Postgres binds by index and a repeated
/// `$1` resolves to the same value with no extra bind. The golden
/// `assert_one_placeholder_per_parameter` accepts the same shape, and this check must not diverge.
#[test]
fn a_numbered_statement_that_reuses_one_parameter_for_a_guarded_ratio_is_accepted() {
    // Two Parameter nodes both naming $1 (index 1) - the shape a guarded ratio produces when the
    // guard is embedded once per term. One param value, one distinct index.
    let param = |index: u32| {
        Expr(Expression::Parameter(Box::new(Parameter {
            name: None,
            index: Some(index),
            style: ParameterStyle::Dollar,
            quoted: false,
            string_quoted: false,
            expression: None,
        })))
    };
    let ast = builder::select(vec![builder::col("t.c").eq(param(1)), builder::col("t.d").eq(param(1))])
        .from("t")
        .build();
    let dialect = Dialect::Postgres;
    let params = vec![ParamValue::Text("one".into())]; // 1 param, 2 nodes, 1 distinct index

    let result = assert_placeholder_count(&ast, dialect, &params);
    assert!(
        result.is_ok(),
        "a Numbered statement reusing $1 for one parameter must be accepted, got {result:?}"
    );
}

/// Pins the upstream half `generate` relies on: polyglot's own Oracle generator config renders a
/// limited `Select` as `FETCH FIRST n ROWS ONLY` (`ORA-03049` refuses `LIMIT`). Red on a release
/// whose generator ignores `limit_fetch_style` (`tobilg/polyglot#480`).
#[test]
fn polyglot_renders_an_oracle_limit_as_fetch_first() {
    let ast = builder::select(vec![builder::col("amount")]).from("orders").limit(7).build();
    let config = polyglot_sql::dialects::Dialect::get(polyglot_sql::DialectType::Oracle)
        .generator_config()
        .clone();
    let sql = polyglot_sql::Generator::with_config(config)
        .generate(&ast)
        .expect("a limited select renders for Oracle");
    assert!(sql.ends_with("FETCH FIRST 7 ROWS ONLY"), "Oracle takes FETCH FIRST:\n{sql}");
    assert!(!sql.contains("LIMIT"), "Oracle refuses LIMIT:\n{sql}");
}

#[test]
fn foundry_ontology_sql_keeps_values_bound_and_uses_spark_date_buckets() {
    let dialect = Dialect::parse("foundry").expect("Foundry is a rendering target");
    let plan = month_plan();
    let query = generate(&plan, dialect).expect("an ontology query renders");
    let sql = query.sql();
    assert!(sql.contains("DATE_TRUNC('month', `orders`.`order_date`)"), "{sql}");
    assert!(sql.contains("CAST("), "{sql}");
    assert!(sql.contains(" AS DATE)"), "{sql}");
    assert!(sql.contains("NULLS LAST"), "{sql}");
    assert_eq!(sql.matches('?').count(), query.params().len(), "{sql}");
    assert_eq!(query.params(), plan.params());
    assert!(!sql.contains("2026-06-01"), "{sql}");
    assert!(!sql.contains("2026-07-01"), "{sql}");
    assert!(polyglot_sql::parse(sql, super::dialect_type(dialect)).is_ok(), "{sql}");
}
