//! What the combine COMPUTES: the join, the grouping, the re-aggregation and the divide tree.
//!
//! **This suite moved here from `sutura_domain::plan::federated`, and it moved because the combine
//! did.** `docs/adr/0039` step 3 replaced a pure domain function with a `DataFusion` plan, so every
//! assertion about what a two-leg question adds up to is an assertion about THIS engine's plan. The
//! cells are the domain suite's, ported: the same plan shapes, the same leg rows, the same expected
//! answers - which is what makes the move a move rather than a rewrite of the specification.
//!
//! Beside the implementation on purpose, for `crate::combine`'s own reason: `combine.rs` is a new
//! file, so a tree with the production files reverted loses the `mod combine;` that declares this,
//! and a test module that is never compiled reports a proof it did not make.
//!
//! The refusals are in [`refusals`], split for the `max-lines` cap along the seam the module header
//! already draws: what a combine answers, and what it declines to answer.

use sutura_domain::model::Aggregate;
use sutura_domain::warehouse::{Real, Value};

mod fixtures;
use fixtures::*;

/// The refusals, split out for this file's own `max-lines` reason.
mod refusals;

/// `2^53`: every integer below it is exactly representable as an `f64`.
const TWO_POW_53: i64 = 1 << 53;

/// `2^53 + 1`, the first integer an `f64` cannot hold: it rounds to [`TWO_POW_53`], so a comparison
/// taken on the widened values reads the two as equal.
const TWO_POW_53_PLUS_ONE: i64 = TWO_POW_53 + 1;

fn text(value: &str) -> Value {
    Value::Text(String::from(value))
}

fn real(value: f64) -> Value {
    Value::Real(Real::parse(value).expect("a test real is finite"))
}

#[test]
fn joins_two_legs_and_reaggregates_by_remote_key() {
    let plan = sum_plan(true);
    let fact = fact(vec![
        fact_row("A", text("c1"), Value::Integer(100)),
        fact_row("A", text("c2"), Value::Integer(200)),
        fact_row("B", text("c1"), Value::Integer(50)),
    ]);
    let lookup = lookup(vec![vec![text("c1"), text("north")], vec![text("c2"), text("north")]]);

    let combined = combined(&plan, &fact, &lookup, UNBOUNDED);
    assert_eq!(combined.columns(), &["product_family", "region", "period", "revenue"]);
    // Two fact rows under `A` join to one remote key, so their sums add: the re-aggregation is the
    // whole point, and a combine that grouped by the LINK instead would answer 100 and 200.
    assert_eq!(
        combined.rows(),
        &[
            vec![text("A"), text("north"), text("2026-06"), Value::Integer(300)],
            vec![text("B"), text("north"), text("2026-06"), Value::Integer(50)],
        ]
    );
}

#[test]
fn an_inner_join_drops_an_unmatched_fact_row() {
    let plan = sum_plan(false);
    let fact = fact(vec![fact_row("A", text("c9"), Value::Integer(100))]);
    let lookup = lookup(vec![vec![text("c1"), text("north")]]);
    let combined = combined(&plan, &fact, &lookup, UNBOUNDED);
    assert!(
        combined.rows().is_empty(),
        "an unmatched fact row is dropped by an inner join"
    );
}

#[test]
fn a_left_join_keeps_an_unmatched_fact_row_with_null_remote() {
    let plan = sum_plan(true);
    let fact = fact(vec![fact_row("A", text("c9"), Value::Integer(100))]);
    let lookup = lookup(vec![vec![text("c1"), text("north")]]);
    let combined = combined(&plan, &fact, &lookup, UNBOUNDED);
    assert_eq!(
        combined.rows(),
        &[vec![text("A"), Value::Null, text("2026-06"), Value::Integer(100)]]
    );
}

#[test]
fn the_join_kind_decides_a_null_fact_key_the_way_it_decides_an_unmatched_one() {
    // **The cross-product a pair of narrower cells missed between them.** A null link matches
    // nothing (`NULL = NULL` is not true), so a null-keyed fact row is UNMATCHED and the join kind
    // decides it: retained with null remote keys under LEFT, dropped under INNER. The lookup
    // carries a null-linked row of its own, so a combine that retained the fact row by MATCHING it
    // there would answer `nowhere` instead of null.
    let facts = fact(vec![
        fact_row("matched", text("c1"), Value::Integer(100)),
        fact_row("unmatched", text("c9"), Value::Integer(200)),
        fact_row("null_key", Value::Null, Value::Integer(400)),
    ]);
    let lookup = lookup(vec![vec![text("c1"), text("north")], vec![Value::Null, text("nowhere")]]);

    let left = combined(&sum_plan(true), &facts, &lookup, UNBOUNDED);
    assert_eq!(
        left.rows(),
        &[
            vec![text("matched"), text("north"), text("2026-06"), Value::Integer(100)],
            vec![text("null_key"), Value::Null, text("2026-06"), Value::Integer(400)],
            vec![text("unmatched"), Value::Null, text("2026-06"), Value::Integer(200)],
        ],
        "a left join retains a null-keyed fact row with its measure, unmatched"
    );

    let inner = combined(&sum_plan(false), &facts, &lookup, UNBOUNDED);
    assert_eq!(
        inner.rows(),
        &[vec![text("matched"), text("north"), text("2026-06"), Value::Integer(100)]],
        "an inner join drops both the unmatched and the null-keyed fact row"
    );
}

#[test]
fn a_null_key_and_an_unmatched_key_reaggregate_into_one_unmatched_group() {
    // Both are unmatched, both project null remote keys, so they are ONE group under LEFT - and the
    // measure is their sum. A combine that projected them separately would answer twice under one
    // certified name.
    let facts = fact(vec![
        fact_row("A", Value::Null, Value::Integer(7)),
        fact_row("A", text("c9"), Value::Integer(11)),
    ]);
    let lookup = lookup(vec![vec![text("c1"), text("north")]]);
    let combined = combined(&sum_plan(true), &facts, &lookup, UNBOUNDED);
    assert_eq!(
        combined.rows(),
        &[vec![text("A"), Value::Null, text("2026-06"), Value::Integer(18)]]
    );
}

#[test]
fn an_average_is_undivided_in_the_leg_and_divided_above() {
    // The decomposed shape: the leg carries a sum and a count, the combine adds each across the
    // group and divides once. A mean of means is not the mean, which is what this pins.
    let plan = avg_plan();
    let fact = two_leaf_fact(vec![
        vec![text("A"), text("c1"), text("2026-06"), Value::Integer(300), Value::Integer(2)],
        vec![text("A"), text("c1"), text("2026-06"), Value::Integer(100), Value::Integer(2)],
    ]);
    let combined = combined(&plan, &fact, &one_lookup(), UNBOUNDED);
    assert_eq!(
        combined.rows(),
        &[vec![text("A"), text("north"), text("2026-06"), real(100.0),]],
        "400 over 4, divided once above the legs"
    );
}

#[test]
fn a_decomposed_average_with_zero_over_zero_is_null() {
    // `Descent::of(Avg)` declares `ZeroDenominator::Null`, because an average over no rows is null
    // rather than an error. The guard is applied to the FINAL denominator, above every leg.
    let plan = avg_plan();
    let fact = two_leaf_fact(vec![vec![
        text("A"),
        text("c1"),
        text("2026-06"),
        Value::Integer(0),
        Value::Integer(0),
    ]]);
    let combined = combined(&plan, &fact, &one_lookup(), UNBOUNDED);
    assert_eq!(
        combined.rows(),
        &[vec![text("A"), text("north"), text("2026-06"), Value::Null]]
    );
}

#[test]
fn a_minimum_leaf_reaggregates_across_the_group() {
    let plan = extreme_plan(Aggregate::Min, "smallest_subscription_mrr");
    let fact = fact(vec![
        fact_row("A", text("c1"), Value::Integer(50)),
        fact_row("A", text("c1"), Value::Integer(20)),
        fact_row("A", text("c1"), Value::Integer(90)),
    ]);
    let combined = combined(&plan, &fact, &one_lookup(), UNBOUNDED);
    assert_eq!(
        combined.rows(),
        &[vec![text("A"), text("north"), text("2026-06"), Value::Integer(20)]]
    );
}

#[test]
fn a_maximum_over_wide_integers_compares_exactly_rather_than_as_floats() {
    // **`2^53` and `2^53 + 1` are one `f64`.** A comparison taken on widened values reads them as
    // equal and answers whichever arrived first, so the greater of the two is the assertion that
    // the comparison is exact. `DataFusion`'s `max` over an `Int64` column compares `i64`s.
    let plan = extreme_plan(Aggregate::Max, "largest_subscription_mrr");
    let fact = fact(vec![
        fact_row("A", text("c1"), Value::Integer(TWO_POW_53_PLUS_ONE)),
        fact_row("A", text("c1"), Value::Integer(TWO_POW_53)),
    ]);
    let combined = combined(&plan, &fact, &one_lookup(), UNBOUNDED);
    assert_eq!(
        combined.rows(),
        &[vec![
            text("A"),
            text("north"),
            text("2026-06"),
            Value::Integer(TWO_POW_53_PLUS_ONE)
        ]]
    );
}

#[test]
fn a_leaf_total_past_an_i64_comes_back_exact_rather_than_refused() {
    // **The refusal this REPLACES, and it is a better answer than the one it replaces.** The
    // hand-written combine added leaf cells with `checked_add` and refused an overflow as
    // `FederatedAnswerRefusal::Overflow`, which lost the number. `crate::combine::leaf_sum` casts an
    // exact leaf to a 256-bit decimal, so the total is computed exactly and
    // `ResultBatches::to_rows` renders one that does not fit an `i64` as its exact TEXT.
    //
    // `DataFusion`'s own accumulator adds with WRAPPING arithmetic, so without that cast this
    // answers `-2` rather than either refusing or answering exactly - which is why the cell asserts
    // the digits and not merely that it is not an error.
    let plan = sum_plan(true);
    let fact = fact(vec![
        fact_row("A", text("c1"), Value::Integer(i64::MAX)),
        fact_row("A", text("c1"), Value::Integer(i64::MAX)),
    ]);
    let combined = combined(&plan, &fact, &one_lookup(), UNBOUNDED);
    let exact = (i128::from(i64::MAX) * 2).to_string();
    assert_eq!(
        combined.rows(),
        &[vec![text("A"), text("north"), text("2026-06"), text(&exact)]],
        "the total is exact text rather than a wrapped integer or a refusal"
    );
}

#[test]
fn a_leaf_column_of_nulls_answers_null_and_never_names_a_refusal() {
    // A group that contributed nothing is a null and not a zero: a zero is a number somebody could
    // read as a total, and nothing was totalled.
    let plan = sum_plan(true);
    let fact = fact(vec![
        fact_row("A", text("c1"), Value::Null),
        fact_row("A", text("c1"), Value::Null),
    ]);
    let combined = combined(&plan, &fact, &one_lookup(), UNBOUNDED);
    assert_eq!(
        combined.rows(),
        &[vec![text("A"), text("north"), text("2026-06"), Value::Null]]
    );
}

#[test]
fn a_real_leaf_totals_as_a_float() {
    // The other half of `leaf_sum`: a float leaf is summed as a float, which is the same addition
    // the mono path's own `SUM` performs. Casting one to a decimal would be a different number.
    let plan = sum_plan(true);
    let fact = fact(vec![
        fact_row("A", text("c1"), real(1.5)),
        fact_row("A", text("c1"), real(2.25)),
    ]);
    let combined = combined(&plan, &fact, &one_lookup(), UNBOUNDED);
    assert_eq!(
        combined.rows(),
        &[vec![text("A"), text("north"), text("2026-06"), real(3.75)]]
    );
}

#[test]
fn the_answer_orders_ascending_with_nulls_last_like_the_mono_path() {
    // `ASC NULLS LAST`, which is the whole of the ordered-result contract and not this combine's
    // choice: `sutura_sql::generate::ordered_nulls_last` emits it for every rendered answer, so a
    // combine that placed a null first would make one certified metric come back in one order from
    // one data system and another from two - with no golden able to see it.
    let plan = sum_plan(true);
    let fact = fact(vec![
        fact_row("B", text("c1"), Value::Integer(1)),
        fact_row("A", text("c1"), Value::Integer(2)),
        fact_row("C", text("c9"), Value::Integer(3)),
    ]);
    let lookup = lookup(vec![vec![text("c1"), text("north")]]);
    let combined = combined(&plan, &fact, &lookup, UNBOUNDED);
    let families: Vec<&Value> = combined.rows().iter().filter_map(|row| row.first()).collect();
    assert_eq!(
        families,
        vec![&text("A"), &text("B"), &text("C")],
        "ascending by the first key"
    );
    let regions: Vec<&Value> = combined.rows().iter().filter_map(|row| row.get(1)).collect();
    assert_eq!(
        regions,
        vec![&text("north"), &text("north"), &Value::Null],
        "the unmatched row's null remote key sorts after every value"
    );
}

#[test]
fn a_numeric_key_orders_by_value_and_not_by_its_rendered_text() {
    // `10` after `9`, which ordering by rendered text gets wrong (`"10" < "9"`). The key column is
    // `Int64` here, so `DataFusion` sorts it numerically - the property the hand-written
    // comparator had to establish by hand over a per-cell union.
    let plan = sum_plan(true);
    let fact = fact(vec![
        fact_row("A", text("c1"), Value::Integer(1)),
        fact_row("A", text("c1"), Value::Integer(1)),
    ]);
    let numeric = batches_of(
        vec![
            String::from("product_family"),
            link(),
            String::from(sutura_domain::catalog::TIME_BUCKET_LABEL),
            leaf(0),
        ],
        vec![
            vec![Value::Integer(10), text("c1"), text("2026-06"), Value::Integer(1)],
            vec![Value::Integer(9), text("c1"), text("2026-06"), Value::Integer(1)],
        ],
    );
    drop(fact);
    let combined = combined(&plan, &numeric, &one_lookup(), UNBOUNDED);
    let families: Vec<&Value> = combined.rows().iter().filter_map(|row| row.first()).collect();
    assert_eq!(families, vec![&Value::Integer(9), &Value::Integer(10)]);
}
