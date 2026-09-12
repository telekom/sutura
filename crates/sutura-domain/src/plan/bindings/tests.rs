//! What the parsed binding set accepts, and the three shapes it refuses.
//!
//! **Every refusal here was constructible before this type existed**, and one of them was live in
//! this repository: `plan/anchor_tests.rs` built plans whose predicate bound parameter 0 beside an
//! EMPTY parameter list, and every assertion over them passed. The module header says what each
//! adapter does with such a plan.

use crate::calendar::Date;
use crate::model::{ColumnName, TableName};
use crate::plan::{PlanColumn, PlanFilter, PlanPredicate, PredicateOrigin};
use crate::warehouse::ParamValue;

use super::{IncoherentBindings, PlanBindings};

fn column(name: &str) -> PlanColumn {
    PlanColumn::new(
        TableName::parse("orders").expect("a test table is a table"),
        ColumnName::parse(name).expect("a test column is a column"),
    )
}

fn day(iso: &str) -> ParamValue {
    ParamValue::Date(Date::parse(iso).expect("a test date is a date"))
}

/// `column >= param` at the given index, definitional.
fn at_or_after(param: usize) -> PlanFilter {
    PlanFilter::new(
        PredicateOrigin::Definition,
        PlanPredicate::AtOrAfter {
            column: column("order_date"),
            param,
        },
    )
}

/// `column < param` at the given index, definitional.
fn before(param: usize) -> PlanFilter {
    PlanFilter::new(
        PredicateOrigin::Definition,
        PlanPredicate::Before {
            column: column("order_date"),
            param,
        },
    )
}

/// A predicate that binds nothing.
fn is_true() -> PlanFilter {
    PlanFilter::new(
        PredicateOrigin::Definition,
        PlanPredicate::IsTrue {
            column: column("is_active"),
        },
    )
}

#[test]
fn a_coherent_set_parses_and_keeps_both_orders() {
    let bindings = PlanBindings::parse(vec![at_or_after(0), before(1)], vec![day("2026-06-01"), day("2026-07-01")])
        .expect("two bounds binding parameters 0 and 1 in that order is coherent");

    // Parameter order is the acceptance criterion the positional dialects rest on: the Nth
    // placeholder takes the Nth value, so the list must come back as it went in rather than sorted,
    // deduplicated or reordered by the parse.
    assert_eq!(
        bindings.params(),
        [day("2026-06-01"), day("2026-07-01")],
        "a parsed set hands its parameters back in placeholder order"
    );
    // And the filters, because the renderer emits them in this order and that is what makes the
    // placeholder positions agree with the list above.
    assert_eq!(bindings.filters().len(), 2);
    assert_eq!(
        bindings.filters().first().map(PlanFilter::predicate),
        Some(at_or_after(0).predicate())
    );
    assert_eq!(
        bindings.filters().get(1).map(PlanFilter::predicate),
        Some(before(1).predicate())
    );

    // `none` repeats no check, so this is the one assertion that keeps it equal to the parse rather
    // than merely infallible. Folded in here rather than a test of its own: no mutation of `parse`
    // reddens it alone, so on its own it would look like coverage it is not.
    assert_eq!(
        PlanBindings::parse(Vec::new(), Vec::new()).expect("no predicates and no parameters is coherent"),
        PlanBindings::none(),
        "the no-parameter spelling and the parse agree on the empty set"
    );
}

#[test]
fn a_predicate_binding_a_parameter_the_set_does_not_hold_is_refused() {
    // The shape `sutura_sql` renders as a placeholder the statement carries no value for, and
    // `QueryPlan::definitional_params` silently dropped.
    assert_eq!(
        PlanBindings::parse(vec![at_or_after(5)], vec![day("2026-06-01")]).unwrap_err(),
        IncoherentBindings::OutOfRange { index: 5, params: 1 },
        "an index the parameter list cannot hold is reported as the index and the count"
    );
}

#[test]
fn a_predicate_binding_the_parameter_just_past_the_list_is_out_of_range() {
    // The boundary the test above does not pin: `index == params.len()` is one past the last valid
    // index (`0..params.len()`), and the range check runs before the order check, so it is refused
    // as `OutOfRange` rather than `OutOfPlaceholderOrder` - the order check never sees an index that
    // is not already known to be in range. Refusal is complete either way this boundary is read;
    // only the diagnostic is pinned here.
    assert_eq!(
        PlanBindings::parse(vec![at_or_after(1)], vec![day("2026-06-01")]).unwrap_err(),
        IncoherentBindings::OutOfRange { index: 1, params: 1 },
        "index == params.len() is out of range at the boundary, not out of order"
    );
}

#[test]
fn a_predicate_binding_out_of_placeholder_order_is_refused() {
    // Both indices EXIST here, which is what makes this the sharper half: every index is in range,
    // a numbered dialect renders `$2` then `$1` and reads the right values, and a positional one
    // renders `?` then `?` and binds the range backwards - an empty result under a certified metric
    // name. So the ordering is refused rather than left to the dialect.
    assert_eq!(
        PlanBindings::parse(vec![at_or_after(1), before(0)], vec![day("2026-07-01"), day("2026-06-01")]).unwrap_err(),
        IncoherentBindings::OutOfPlaceholderOrder { position: 0, index: 1 },
        "the first binding predicate takes the first placeholder"
    );
}

#[test]
fn a_parameter_no_predicate_binds_is_refused() {
    assert_eq!(
        PlanBindings::parse(vec![at_or_after(0)], vec![day("2026-06-01"), day("2026-07-01")]).unwrap_err(),
        IncoherentBindings::NeverRead { read: 1, params: 2 },
        "a value nothing reads is a placeholder count that disagrees with the bound list"
    );
}

#[test]
fn a_predicate_that_binds_nothing_consumes_no_placeholder() {
    // `IS TRUE` and `IS NOT NULL` render no placeholder, so they must not advance the count - and a
    // parse that counted filters rather than BINDING filters would refuse this coherent set. The
    // unbound predicate sits between the two bounds deliberately: at the end it would pass either
    // way.
    let bindings = PlanBindings::parse(
        vec![at_or_after(0), is_true(), before(1)],
        vec![day("2026-06-01"), day("2026-07-01")],
    )
    .expect("an unbound predicate between two bound ones consumes no placeholder");
    assert_eq!(bindings.filters().len(), 3, "all three predicates are kept");
    assert_eq!(bindings.params().len(), 2, "only two of them bind");
}
