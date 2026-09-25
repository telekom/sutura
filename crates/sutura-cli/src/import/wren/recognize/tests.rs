use sutura_domain::model::Aggregate;

use super::{Measure, bare_column, equi_join, measure, term};

#[test]
fn a_bare_column_is_recognised_and_a_function_call_is_not() {
    assert_eq!(bare_column("status"), Some("status"));
    assert_eq!(bare_column(" status "), Some("status"));
    assert_eq!(bare_column("upper(status)"), None);
    assert_eq!(bare_column("a.b"), None);
}

#[test]
fn every_closed_aggregate_over_a_bare_column_is_recognised() {
    let cases = [
        ("SUM(amount_cents)", Aggregate::Sum, "amount_cents"),
        ("avg(amount_cents)", Aggregate::Avg, "amount_cents"),
        ("MIN(amount_cents)", Aggregate::Min, "amount_cents"),
        ("Max(amount_cents)", Aggregate::Max, "amount_cents"),
        ("COUNT(customer_id)", Aggregate::Count, "customer_id"),
        ("COUNT(DISTINCT customer_id)", Aggregate::CountDistinct, "customer_id"),
        ("count(distinct customer_id)", Aggregate::CountDistinct, "customer_id"),
    ];
    for (expr, expected_aggregate, expected_column) in cases {
        let recognised = term(expr).unwrap_or_else(|| panic!("{expr} did not recognise"));
        assert_eq!(recognised.aggregate, expected_aggregate, "{expr}");
        assert_eq!(recognised.column, expected_column, "{expr}");
    }
}

#[test]
fn a_term_over_an_expression_rather_than_a_column_is_refused() {
    assert!(term("SUM(amount_cents * 1.1)").is_none());
    assert!(term("SUM(amount_cents)  extra").is_none());
    assert!(term("NOT_AN_AGGREGATE(amount_cents)").is_none());
    // The clause the domain's own `Term::Aggregate(Count)` needs a column for: wren's `COUNT(*)`
    // names none.
    assert!(term("COUNT(*)").is_none());
}

#[test]
fn a_ratio_measure_splits_on_the_top_level_slash_only() {
    match measure("SUM(amount_cents) / COUNT(DISTINCT customer_id)") {
        Some(Measure::Ratio { numerator, denominator }) => {
            assert_eq!(numerator.aggregate, Aggregate::Sum);
            assert_eq!(denominator.aggregate, Aggregate::CountDistinct);
        }
        other => panic!("expected a ratio, got {}", other.is_some()),
    }
    // A slash inside a nested call must not split the measure - there is none in this vocabulary,
    // but the scan must still see it as depth rather than as the top-level separator.
    assert!(measure("SUM(a / b)").is_none());
}

#[test]
fn a_simple_measure_is_recognised() {
    assert!(matches!(measure("SUM(amount_cents)"), Some(Measure::Simple(_))));
}

#[test]
fn an_equi_join_is_recognised_and_a_wider_condition_is_refused() {
    let (left, right) = equi_join("orders.customer_id = customers.id").expect("a plain equality recognises");
    assert_eq!(left.model, "orders");
    assert_eq!(left.column, "customer_id");
    assert_eq!(right.model, "customers");
    assert_eq!(right.column, "id");

    // The escape hatch `sutura_domain::catalog::Relationship`'s own header names: a second `=`
    // anywhere refuses the whole condition rather than being parsed and rejected.
    assert!(equi_join("orders.customer_id = customers.id OR 1 = 1").is_none());
    assert!(equi_join("orders.customer_id").is_none());
}
