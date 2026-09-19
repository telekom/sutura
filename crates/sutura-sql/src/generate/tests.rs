use sutura_domain::catalog::{Definitions, Description, Model, Relationship};
use sutura_domain::federation::Ranking;
use sutura_domain::model::{Aggregate, ColumnName, Grain, JoinType, ModelName, RelationshipName, SourceName, TableName};
use sutura_domain::plan::{
    FactTop, LegPlan, LegTerm, PlanBindings, PlanBucket, PlanColumn, PlanKey, PlanTerm, ResultLabel, StatementTables,
};
use sutura_domain::query::{Top, TopBy, TopDirection, TopN};

use polyglot_sql::builder;

use super::{
    DISTINCT_LABEL, DeclaredKey, ROWS_LABEL, bucket_expression, generate_key_probe, generate_leg, ordered_nulls_last, render,
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
            // as fixed by the layer. Both need the keyword to agree with the AST.
            Dialect::BigQuery | Dialect::DuckDb => assert!(sql.contains("NULLS LAST"), "{dialect}: {sql}"),
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
        ColumnName::parse("customer_key").expect("a test column is a column"),
        ModelName::parse("customers").expect("a test model is a model"),
        ColumnName::parse("customer_key").expect("a test column is a column"),
        JoinType::ManyToOne,
    );
    let definitions = Definitions::assemble(
        vec![
            Model::new(
                ModelName::parse("orders").expect("a test model is a model"),
                SourceName::parse("local").expect("a test source is a source"),
                TableName::parse("orders").expect("a test table is a table"),
                std::iter::once("customer_key")
                    .map(|c| ColumnName::parse(c).expect("a test column is a column"))
                    .collect(),
                Description::default(),
            ),
            Model::new(
                ModelName::parse("customers").expect("a test model is a model"),
                SourceName::parse("local").expect("a test source is a source"),
                TableName::parse("dim_customer").expect("a test table is a table"),
                std::iter::once("customer_key")
                    .map(|c| ColumnName::parse(c).expect("a test column is a column"))
                    .collect(),
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

/// A case-1 `top` on a fact leg - `github.com/telekom/sutura#777`. Its own fixture rather than
/// one added to `crates/sutura-app/tests/golden/legs.rs`, because that corpus's own header
/// declares "no leg golden may carry a `LIMIT`" and a `check-guidance`-counted literal
/// (`LIMIT 10001`) reads off it; a leg WITH a pushdown limit belongs beside the mechanism instead
/// of inside a corpus built on that declaration.
fn fact_with_pushed_down_top() -> LegPlan {
    let table = TableName::parse("orders").expect("a test table is a table");
    let column = |name: &str| PlanColumn::new(table.clone(), ColumnName::parse(name).expect("a test column is a column"));
    LegPlan::Fact {
        source: SourceName::parse("local").expect("a test source is a source"),
        metric: sutura_domain::model::MetricName::parse("revenue").expect("a test metric is a metric"),
        tables: StatementTables::only(table.clone()),
        bucket: bucket(Grain::Month),
        keys: vec![PlanKey::new(
            ResultLabel::dimension(&sutura_domain::model::DimensionName::parse("product_family").expect("a test dimension")),
            column("product_family"),
        )],
        terms: vec![LegTerm::new(
            PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: column("revenue_cents"),
            },
            ResultLabel::internal(sutura_domain::plan::InternalLabel::Leaf(0)),
        )],
        bindings: PlanBindings::none(),
        range: sutura_domain::calendar::TimeRange::new(
            sutura_domain::calendar::Date::parse("2026-06-01").expect("a test date is a date"),
            sutura_domain::calendar::Date::parse("2026-07-01").expect("a test date is a date"),
        )
        .expect("a test range is a range"),
        top: Some(FactTop::new(
            Top::new(
                TopN::parse(3).expect("three is a row count"),
                TopBy::Metric,
                TopDirection::Desc,
            ),
            Ranking::Term(0),
        )),
    }
}

#[test]
fn a_case_1_fact_leg_carries_the_pushed_down_order_and_limit() {
    for &dialect in ALL {
        let query = generate_leg(&fact_with_pushed_down_top(), dialect)
            .unwrap_or_else(|e| panic!("{dialect} would not render a case-1 leg: {e}"));
        let sql = query.sql();
        assert!(sql.contains("ORDER BY"), "{dialect}: {sql}");
        assert!(sql.contains("LIMIT 3"), "{dialect}: {sql}");
        // The ranking column, not the tie-break key, leads the ORDER BY - the same claim
        // `by_metric_ranks_the_measure_first_and_keeps_the_tiebreak_after_it` makes for the
        // mono path.
        let order_by = sql.split("ORDER BY").nth(1).expect("an ORDER BY clause is present");
        let revenue_at = order_by.find("revenue_cents").expect("the ranking leads");
        let family_at = order_by.find("product_family").expect("the tie-break follows");
        assert!(revenue_at < family_at, "{dialect}: {sql}");
        assert!(order_by.contains("DESC"), "{dialect}: {sql}");
        let parsed = polyglot_sql::parse(sql, super::dialect_type(dialect));
        assert!(
            parsed.is_ok(),
            "{dialect} did not parse its own case-1 leg: {:?}\n{sql}",
            parsed.err()
        );
    }
}

/// The negative control for the leg corpus's own row-limit assertion: a leg with NO `top` still
/// carries no `LIMIT`, so the pushdown above is additive rather than a change to every leg.
#[test]
fn a_fact_leg_with_no_top_still_carries_no_limit() {
    let mut leg = fact_with_pushed_down_top();
    let LegPlan::Fact { ref mut top, .. } = leg else {
        panic!("this fixture is a fact leg");
    };
    *top = None;
    let query = generate_leg(&leg, Dialect::DuckDb).expect("a leg with no top still renders");
    assert!(!query.sql().contains("LIMIT"), "{}", query.sql());
}
