use crate::calendar::{Date, TimeRange};
use crate::catalog::TIME_BUCKET_LABEL;
use crate::federation::Federation;
use crate::measure::{AggregatedColumn, Measure, Term, ZeroDenominator};
use crate::model::{Aggregate, ColumnName, Grain, MetricName, SourceName, TableName};
use crate::plan::leg::LegPlan;
use crate::plan::{
    AnswerKey, FederatedFailure, FederatedPlan, FederatedPlanError, PlanBucket, PlanColumn, PlanKey, StatementTables,
};
use crate::warehouse::{Real, RowSet, Value};

const FACT: &str = "fct_subscription_monthly";
const FACT_SOURCE: &str = "facts";
const REMOTE_SOURCE: &str = "geo";
/// No budget for the correctness tests: each passes an effectively unbounded ceiling so only the
/// budget test exercises the refusal.
const UNBOUNDED: u64 = u64::MAX;

fn metric(name: &str) -> MetricName {
    MetricName::parse(name).expect("a test metric is a metric")
}

fn column(name: &str) -> ColumnName {
    ColumnName::parse(name).expect("a test column is a column")
}

fn term(aggregate: Aggregate, name: &str) -> Term {
    Term::Aggregate(AggregatedColumn::new(aggregate, column(name)))
}

fn source(name: &str) -> SourceName {
    SourceName::parse(name).expect("a test source is a source")
}

fn table(name: &str) -> TableName {
    TableName::parse(name).expect("a test table is a table")
}

fn range() -> TimeRange {
    TimeRange::new(
        Date::parse("2026-01-01").expect("a test start"),
        Date::parse("2026-02-01").expect("a test end"),
    )
    .expect("a test range")
}

fn key(name: &str, table_name: &str) -> PlanKey {
    PlanKey::new(String::from(name), PlanColumn::new(table(table_name), column(name)))
}

fn bucket() -> PlanBucket {
    PlanBucket::new(
        String::from(TIME_BUCKET_LABEL),
        Grain::Month,
        PlanColumn::new(table(FACT), column("month")),
    )
}

fn fact_leg() -> LegPlan {
    LegPlan::Fact {
        source: source(FACT_SOURCE),
        metric: metric("revenue"),
        tables: StatementTables::only(table(FACT)),
        bucket: bucket(),
        keys: vec![key("product_family", FACT), key("customer_key", FACT)],
        terms: Vec::new(),
        filters: Vec::new(),
        params: Vec::new(),
        range: range(),
    }
}

fn lookup_leg() -> LegPlan {
    LegPlan::Lookup {
        source: source(REMOTE_SOURCE),
        table: table(FACT).into(),
        keys: vec![key("customer_key", FACT), key("region", FACT)],
        filters: Vec::new(),
        params: Vec::new(),
    }
}

fn plan_for(measure_name: &str, measure: &Measure, include_unmatched: bool) -> FederatedPlan {
    let name = metric(measure_name);
    let federation = Federation::of(measure);
    FederatedPlan::new(
        name,
        String::from(measure_name),
        bucket(),
        fact_leg(),
        lookup_leg(),
        String::from("customer_key"),
        String::from("customer_key"),
        include_unmatched,
        federation,
        vec![
            AnswerKey::fact(String::from("product_family")),
            AnswerKey::lookup(String::from("region")),
        ],
    )
    .expect("a test plan is a valid two-leg plan")
}

fn sum_plan(include_unmatched: bool) -> FederatedPlan {
    plan_for(
        "revenue",
        &Measure::Simple(term(Aggregate::Sum, "mrr_cents")),
        include_unmatched,
    )
}

fn avg_plan() -> FederatedPlan {
    plan_for(
        "mean_subscription_mrr",
        &Measure::Simple(term(Aggregate::Avg, "mrr_cents")),
        true,
    )
}

fn failing_ratio_plan() -> FederatedPlan {
    plan_for(
        "mean_subscription_mrr",
        &Measure::Ratio {
            numerator: term(Aggregate::Sum, "mrr_cents"),
            denominator: term(Aggregate::Count, "mrr_cents"),
            zero_denominator: ZeroDenominator::Fail,
        },
        true,
    )
}

fn fact(rows: Vec<Vec<Value>>) -> RowSet {
    RowSet::new(
        vec![
            String::from("product_family"),
            String::from("customer_key"),
            String::from(TIME_BUCKET_LABEL),
            String::from("revenue"),
        ],
        rows,
    )
    .expect("a test fact result is well formed")
}

/// The same shape as [`fact`], with the single measure column under `label` rather than `revenue`.
///
/// A single-leaf plan projects its leaf under the metric's own name - `labels` says so - so a
/// minimum or a maximum needs its own label where the sum tests reuse `revenue`.
fn labelled_fact(label: &str, rows: Vec<Vec<Value>>) -> RowSet {
    RowSet::new(
        vec![
            String::from("product_family"),
            String::from("customer_key"),
            String::from(TIME_BUCKET_LABEL),
            String::from(label),
        ],
        rows,
    )
    .expect("a labelled fact result is well formed")
}

/// One fact row carrying `measure`, in the single group [`one_lookup`] maps `c1` into.
fn fact_row(measure: Value) -> Vec<Value> {
    vec![
        Value::Text("A".into()),
        Value::Text("c1".into()),
        Value::Text("2026-06".into()),
        measure,
    ]
}

/// The one lookup row the re-aggregation tests join against, so every fact row shares one group.
fn one_lookup() -> RowSet {
    lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]])
}

/// `2^53`: every integer below it is exactly representable as an `f64`.
const TWO_POW_53: i64 = 1 << 53;
/// `2^53 + 1`, the first integer an `f64` cannot hold: it rounds to [`TWO_POW_53`], so a comparison
/// taken on the widened values reads the two as equal.
const TWO_POW_53_PLUS_ONE: i64 = TWO_POW_53 + 1;

/// A plan whose measure is a re-aggregating minimum or maximum over one column.
fn extreme_plan(aggregate: Aggregate, measure_name: &str) -> FederatedPlan {
    plan_for(measure_name, &Measure::Simple(term(aggregate, "mrr_cents")), true)
}

fn lookup(rows: Vec<Vec<Value>>) -> RowSet {
    RowSet::new(vec![String::from("customer_key"), String::from("region")], rows).expect("a test lookup result is well formed")
}

#[test]
fn joins_two_legs_and_reaggregates_by_remote_key() {
    let plan = sum_plan(true);
    let fact = fact(vec![
        vec![
            Value::Text("A".into()),
            Value::Text("c1".into()),
            Value::Text("2026-06".into()),
            Value::Integer(100),
        ],
        vec![
            Value::Text("A".into()),
            Value::Text("c2".into()),
            Value::Text("2026-06".into()),
            Value::Integer(200),
        ],
        vec![
            Value::Text("B".into()),
            Value::Text("c1".into()),
            Value::Text("2026-06".into()),
            Value::Integer(50),
        ],
    ]);
    let lookup = lookup(vec![
        vec![Value::Text("c1".into()), Value::Text("north".into())],
        vec![Value::Text("c2".into()), Value::Text("north".into())],
    ]);

    let combined = plan.combine(&fact, &lookup, UNBOUNDED).expect("a two-leg question combines");
    assert_eq!(combined.columns(), &["product_family", "region", "period", "revenue"]);
    assert_eq!(
        combined.rows(),
        &[
            vec![
                Value::Text("A".into()),
                Value::Text("north".into()),
                Value::Text("2026-06".into()),
                Value::Integer(300)
            ],
            vec![
                Value::Text("B".into()),
                Value::Text("north".into()),
                Value::Text("2026-06".into()),
                Value::Integer(50)
            ],
        ]
    );
}

#[test]
fn an_inner_join_drops_an_unmatched_fact_row() {
    let plan = sum_plan(false);
    let fact = fact(vec![vec![
        Value::Text("A".into()),
        Value::Text("c9".into()),
        Value::Text("2026-06".into()),
        Value::Integer(100),
    ]]);
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
    let combined = plan.combine(&fact, &lookup, UNBOUNDED).expect("combines");
    assert!(
        combined.rows().is_empty(),
        "an unmatched fact row is dropped by an inner join"
    );
}

#[test]
fn a_left_join_keeps_an_unmatched_fact_row_with_null_remote() {
    let plan = sum_plan(true);
    let fact = fact(vec![vec![
        Value::Text("A".into()),
        Value::Text("c9".into()),
        Value::Text("2026-06".into()),
        Value::Integer(100),
    ]]);
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
    let combined = plan.combine(&fact, &lookup, UNBOUNDED).expect("combines");
    assert_eq!(
        combined.rows(),
        &[vec![
            Value::Text("A".into()),
            Value::Null,
            Value::Text("2026-06".into()),
            Value::Integer(100),
        ]]
    );
}

fn avg_fact(rows: Vec<Vec<Value>>) -> RowSet {
    RowSet::new(
        vec![
            String::from("product_family"),
            String::from("customer_key"),
            String::from(TIME_BUCKET_LABEL),
            String::from("mean_subscription_mrr__0"),
            String::from("mean_subscription_mrr__1"),
        ],
        rows,
    )
    .expect("an average fact result is well formed")
}

#[test]
#[expect(clippy::float_cmp, reason = "100.0 is exactly representable, so the compare is exact")]
fn an_average_is_undivided_in_the_leg_and_divided_above() {
    let plan = avg_plan();
    let fact = avg_fact(vec![vec![
        Value::Text("A".into()),
        Value::Text("c1".into()),
        Value::Text("2026-06".into()),
        Value::Integer(300),
        Value::Integer(3),
    ]]);
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);

    let combined = plan.combine(&fact, &lookup, UNBOUNDED).expect("an average combines");
    assert_eq!(
        combined.columns(),
        &["product_family", "region", "period", "mean_subscription_mrr"]
    );
    match &combined.rows()[0][3] {
        Value::Real(r) => assert_eq!(r.get(), 100.0),
        other => panic!("an average answers a real number, got {other:?}"),
    }
}

#[test]
fn a_minimum_leaf_reaggregates_across_the_group() {
    let plan = plan_for("min_mrr", &Measure::Simple(term(Aggregate::Min, "mrr_cents")), true);
    let fact = RowSet::new(
        vec![
            String::from("product_family"),
            String::from("customer_key"),
            String::from(TIME_BUCKET_LABEL),
            String::from("min_mrr"),
        ],
        vec![
            vec![
                Value::Text("A".into()),
                Value::Text("c1".into()),
                Value::Text("2026-06".into()),
                Value::Integer(300),
            ],
            vec![
                Value::Text("A".into()),
                Value::Text("c1".into()),
                Value::Text("2026-06".into()),
                Value::Integer(100),
            ],
        ],
    )
    .expect("a min fact result is well formed");
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);

    let combined = plan.combine(&fact, &lookup, UNBOUNDED).expect("a minimum combines");
    assert_eq!(
        combined.rows(),
        &[vec![
            Value::Text("A".into()),
            Value::Text("north".into()),
            Value::Text("2026-06".into()),
            Value::Integer(100)
        ]]
    );
}

#[test]
fn a_failing_ratio_guard_errors_on_a_zero_denominator() {
    let plan = failing_ratio_plan();
    let fact = avg_fact(vec![vec![
        Value::Text("A".into()),
        Value::Text("c1".into()),
        Value::Text("2026-06".into()),
        Value::Integer(300),
        Value::Integer(0),
    ]]);
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
    assert!(matches!(
        plan.combine(&fact, &lookup, UNBOUNDED),
        Err(FederatedFailure::NonFinite { .. })
    ));
}

#[test]
fn a_decomposed_average_with_zero_over_zero_is_null() {
    let plan = avg_plan();
    let fact = avg_fact(vec![vec![
        Value::Text("A".into()),
        Value::Text("c1".into()),
        Value::Text("2026-06".into()),
        Value::Integer(0),
        Value::Integer(0),
    ]]);
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
    let combined = plan.combine(&fact, &lookup, UNBOUNDED).expect("a null guard answers");
    assert!(matches!(combined.rows()[0][3], Value::Null));
}

#[test]
fn a_missing_leaf_label_is_an_error() {
    let plan = sum_plan(true);
    let fact = RowSet::new(
        vec![
            String::from("product_family"),
            String::from("customer_key"),
            String::from(TIME_BUCKET_LABEL),
        ],
        vec![vec![
            Value::Text("A".into()),
            Value::Text("c1".into()),
            Value::Text("2026-06".into()),
        ]],
    )
    .expect("a fact result missing the measure column");
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
    assert!(matches!(
        plan.combine(&fact, &lookup, UNBOUNDED),
        Err(FederatedFailure::MissingColumn { .. })
    ));
}

#[test]
fn a_non_numeric_leaf_is_refused_not_counted_as_zero() {
    // The DuckDB adapter returns a DECIMAL money column as Text to keep it exact; a sum that
    // meets it must refuse rather than certify a zero.
    let plan = sum_plan(true);
    let fact = fact(vec![vec![
        Value::Text("A".into()),
        Value::Text("c1".into()),
        Value::Text("2026-06".into()),
        Value::Text("1234.56".into()),
    ]]);
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
    assert!(matches!(
        plan.combine(&fact, &lookup, UNBOUNDED),
        Err(FederatedFailure::NonNumericLeaf {
            aggregate: Aggregate::Sum,
            ..
        })
    ));
}

#[test]
fn a_sum_over_a_column_mixing_integers_and_reals_is_refused() {
    // `RowSet::new` checks a row's width and nothing about its cells, so one leaf column holding an
    // `Integer` beside a `Real` is representable. Neither subtotal may be dropped and neither may be
    // widened onto the other, so the column is refused: this group answered `1.5` for a column
    // totalling `101.5`, under the metric's own certified name.
    let plan = sum_plan(true);
    let fact = fact(vec![
        fact_row(Value::Integer(100)),
        fact_row(Value::Real(Real::parse(1.5).expect("a finite real"))),
    ]);

    // Asserted on the refusal's own sentence rather than on the variant naming it: `MixedNumericLeaf`
    // is declared in the file `just causality` reverts, and naming it here costs the base tree its
    // build - which takes the base verdict for every other test in this file with it.
    let refusal = plan
        .combine(&fact, &one_lookup(), UNBOUNDED)
        .expect_err("a column mixing integers and reals has no exact total");
    assert_eq!(
        refusal.to_string(),
        "a `Sum` re-aggregation met a leaf column mixing integer and real cells"
    );
}

#[test]
fn a_minimum_over_a_column_mixing_integers_and_reals_is_refused() {
    // The refusal is a property of the column, not of the sum: a minimum over the same column
    // compares an `i64` against an `f64`, which above `2^53` reads two distinguishable cells as
    // equal. One type per column is what makes the comparison exact rather than checked.
    let plan = extreme_plan(Aggregate::Min, "min_mrr");
    let fact = labelled_fact(
        "min_mrr",
        vec![
            fact_row(Value::Integer(100)),
            fact_row(Value::Real(Real::parse(1.5).expect("a finite real"))),
        ],
    );

    let refusal = plan
        .combine(&fact, &one_lookup(), UNBOUNDED)
        .expect_err("a column mixing integers and reals has no exact minimum");
    assert_eq!(
        refusal.to_string(),
        "a `Min` re-aggregation met a leaf column mixing integer and real cells"
    );
}

#[test]
fn a_maximum_over_wide_integers_answers_the_larger_cell() {
    // Two integers one apart, which a data system tells apart and an `f64` does not. Taken as `f64`
    // the comparison reads them as equal and keeps whichever arrived first - here the smaller - so
    // the maximum of the column was not the larger of its cells.
    let plan = extreme_plan(Aggregate::Max, "max_mrr");
    let fact = labelled_fact(
        "max_mrr",
        vec![
            fact_row(Value::Integer(TWO_POW_53)),
            fact_row(Value::Integer(TWO_POW_53_PLUS_ONE)),
        ],
    );

    let combined = plan.combine(&fact, &one_lookup(), UNBOUNDED).expect("a maximum combines");
    assert_eq!(
        combined.rows(),
        &[vec![
            Value::Text("A".into()),
            Value::Text("north".into()),
            Value::Text("2026-06".into()),
            Value::Integer(TWO_POW_53_PLUS_ONE),
        ]]
    );
}

#[test]
fn a_lone_non_numeric_cell_is_refused_by_a_minimum() {
    // The cell the sum above refuses, in a group of one. A minimum accepted its first cell as its
    // own best without ever reading it as a number, so a `DECIMAL` money column - which the
    // `DuckDB` adapter returns as `Text` to keep it exact - came back as the metric's value.
    let plan = extreme_plan(Aggregate::Min, "min_mrr");
    let fact = labelled_fact("min_mrr", vec![fact_row(Value::Text("1234.56".into()))]);

    assert!(matches!(
        plan.combine(&fact, &one_lookup(), UNBOUNDED),
        Err(FederatedFailure::NonNumericLeaf {
            aggregate: Aggregate::Min,
            ..
        })
    ));
}

#[test]
fn a_non_numeric_cell_does_not_win_a_minimum_over_a_number() {
    // The two-cell shape of the same defect: a candidate that is a number could not be compared
    // against a best that is not, and an incomparable pair kept the incumbent - so the text won a
    // comparison it was never in.
    //
    // Both row orders, because only one of them is the defect: with the text second, the old
    // comparison reached the `other` arm and refused anyway, so an order-dependent assertion would
    // have been red against the base tree for the wrong reason.
    let plan = extreme_plan(Aggregate::Min, "min_mrr");
    for cells in [
        vec![fact_row(Value::Text("1234.56".into())), fact_row(Value::Integer(1))],
        vec![fact_row(Value::Integer(1)), fact_row(Value::Text("1234.56".into()))],
    ] {
        let fact = labelled_fact("min_mrr", cells);
        assert!(matches!(
            plan.combine(&fact, &one_lookup(), UNBOUNDED),
            Err(FederatedFailure::NonNumericLeaf {
                aggregate: Aggregate::Min,
                ..
            })
        ));
    }
}

#[test]
fn a_float_link_key_is_refused() {
    let plan = sum_plan(true);
    let fact = fact(vec![vec![
        Value::Text("A".into()),
        Value::Real(Real::parse(1001.0).expect("a finite real")),
        Value::Text("2026-06".into()),
        Value::Integer(100),
    ]]);
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
    assert!(matches!(
        plan.combine(&fact, &lookup, UNBOUNDED),
        Err(FederatedFailure::FloatLinkKey { .. })
    ));
}

#[test]
fn an_integer_link_and_a_text_link_do_not_false_match() {
    // Integer(1001) and Text("1001") are different cells; comparing them as rendered text would
    // join them, which is the false match the typed link key refuses. Under an inner join the
    // non-matching fact row is dropped.
    let plan = sum_plan(false);
    let fact = fact(vec![vec![
        Value::Text("A".into()),
        Value::Integer(1001),
        Value::Text("2026-06".into()),
        Value::Integer(100),
    ]]);
    // The lookup holds the same digits as text.
    let lookup = lookup(vec![vec![Value::Text("1001".into()), Value::Text("north".into())]]);
    let combined = plan.combine(&fact, &lookup, UNBOUNDED).expect("combines");
    assert!(
        combined.rows().is_empty(),
        "an integer link must not join to a text link with the same digits"
    );
}

#[test]
fn a_null_link_never_joins() {
    let plan = sum_plan(false);
    let fact = fact(vec![vec![
        Value::Text("A".into()),
        Value::Null,
        Value::Text("2026-06".into()),
        Value::Integer(100),
    ]]);
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
    let combined = plan.combine(&fact, &lookup, UNBOUNDED).expect("combines");
    assert!(
        combined.rows().is_empty(),
        "a null link value never joins, not even to itself"
    );
}

#[test]
fn a_duplicate_leaf_label_is_refused() {
    // Two columns under one name would be traced to one of them arbitrarily, so the boundary
    // refuses the result rather than answer a wrong number.
    let plan = sum_plan(true);
    let fact = RowSet::new(
        vec![
            String::from("product_family"),
            String::from("customer_key"),
            String::from(TIME_BUCKET_LABEL),
            String::from("revenue"),
            String::from("revenue"),
        ],
        vec![vec![
            Value::Text("A".into()),
            Value::Text("c1".into()),
            Value::Text("2026-06".into()),
            Value::Integer(100),
            Value::Integer(200),
        ]],
    )
    .expect("a fact result with a duplicated label");
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
    assert!(matches!(
        plan.combine(&fact, &lookup, UNBOUNDED),
        Err(FederatedFailure::DuplicateLabels { .. })
    ));
}

#[test]
fn an_ambiguous_lookup_link_is_refused() {
    let plan = sum_plan(true);
    let fact = fact(vec![vec![
        Value::Text("A".into()),
        Value::Text("c1".into()),
        Value::Text("2026-06".into()),
        Value::Integer(100),
    ]]);
    // Two lookup rows for one link would double the measure.
    let lookup = RowSet::new(
        vec![String::from("customer_key"), String::from("region")],
        vec![
            vec![Value::Text("c1".into()), Value::Text("north".into())],
            vec![Value::Text("c1".into()), Value::Text("south".into())],
        ],
    )
    .expect("a lookup result with two rows for one link");
    assert!(matches!(
        plan.combine(&fact, &lookup, UNBOUNDED),
        Err(FederatedFailure::AmbiguousLink { .. })
    ));
}

#[test]
fn an_answer_that_crosses_the_byte_budget_is_refused_not_truncated() {
    // #72's acceptance criterion and the ADR-0009 conversion-boundary bound: the answer is
    // refused as the budget is crossed, never returned part-way. A one-byte budget cannot hold
    // any answer; it provokes the refusal no matter how the accounting is split.
    let plan = sum_plan(true);
    let fact = fact(vec![vec![
        Value::Text("A".into()),
        Value::Text("c1".into()),
        Value::Text("2026-06".into()),
        Value::Integer(100),
    ]]);
    let lookup = lookup(vec![vec![Value::Text("c1".into()), Value::Text("north".into())]]);
    assert!(matches!(
        plan.combine(&fact, &lookup, 1),
        Err(FederatedFailure::ResourcesExhausted { ceiling_bytes: 1 })
    ));
    // And the boundary holds the other way: the same question under a ceiling it fits answers.
    plan.combine(&fact, &lookup, UNBOUNDED)
        .expect("an unbounded budget fits the answer");
}

#[test]
fn a_plan_with_two_legs_on_one_source_does_not_construct() {
    // The constructor is the newtype convention: a plan that is not a fact leg beside a lookup
    // leg on a different data system is not a value of this type. A lookup passed in the fact
    // slot is the first guard to fire, before any source agreement is even compared.
    let plan = FederatedPlan::new(
        metric("revenue"),
        String::from("revenue"),
        bucket(),
        lookup_leg(),
        lookup_leg(),
        String::from("customer_key"),
        String::from("customer_key"),
        true,
        Federation::of(&Measure::Simple(term(Aggregate::Sum, "mrr_cents"))),
        vec![AnswerKey::fact(String::from("product_family"))],
    );
    assert!(matches!(plan, Err(FederatedPlanError::NotFact { .. })));
}
