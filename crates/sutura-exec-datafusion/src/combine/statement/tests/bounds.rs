//! What a statement may not make the deployment build: a call to a scalar function whose output the
//! caller grows in one call, or more operators than a shallow tree needs. Either is refused while
//! planning, before any source opens; each is asked over one source, where it would otherwise run
//! whole.

use sutura_domain::plan::StatementStopped;

use super::{Engines, UNBOUNDED, asked};

/// Each of `statements` refused before any source opens, where a call to `upper`, which the session
/// keeps, answers.
fn refused(statements: &[&str]) {
    let mut legs = Engines::new(None);
    let kept = asked("select upper(region) from dim_region", false, UNBOUNDED, &mut legs);
    assert!(matches!(kept, Some(Ok(_))), "a kept function answers: {kept:?}");
    for statement in statements {
        let mut legs = Engines::new(None);
        let answer = asked(statement, false, UNBOUNDED, &mut legs);
        assert!(
            matches!(answer, Some(Err(StatementStopped::NotPlanned))),
            "{statement}: {answer:?}"
        );
        assert!(legs.opened.is_empty(), "{statement} opened {:?}", legs.opened);
    }
}

#[test]
fn a_length_the_caller_spells_is_not_planned() {
    refused(&[
        "select repeat(region, 2) from dim_region",
        "select lpad(region, 9, 'x') from dim_region",
        "select rpad(region, 9, 'x') from dim_region",
    ]);
}

#[test]
fn a_replacement_that_multiplies_its_input_is_not_planned() {
    refused(&[
        "select replace(region, 'o', 'oo') from dim_region",
        "select regexp_replace(region, 'o', 'oo') from dim_region",
    ]);
}

#[test]
fn an_encoding_that_doubles_per_nesting_is_not_planned() {
    refused(&["select encode(region, 'hex') from dim_region"]);
}

#[test]
fn a_type_spelled_by_a_string_is_not_planned() {
    refused(&[
        "select arrow_cast(region, 'Utf8') from dim_region",
        "select arrow_try_cast(region, 'Utf8') from dim_region",
    ]);
}

/// A chain of `+` nests one level per operator without passing the parser's depth limit. One just
/// under the bound plans and answers on this thread's stack; one past it, and one long enough to
/// exhaust that stack without the bound, are refused before any source opens.
#[test]
fn an_expression_chained_past_the_bound_is_not_planned() {
    let chained = |terms: usize| format!("select region from dim_region where 0 < 1{}", "+1".repeat(terms));
    let mut legs = Engines::new(None);
    let under = asked(&chained(1_000), false, UNBOUNDED, &mut legs);
    assert!(matches!(under, Some(Ok(_))), "a chain under the bound answers: {under:?}");
    for terms in [1_100, 30_000] {
        let mut legs = Engines::new(None);
        let answer = asked(&chained(terms), false, UNBOUNDED, &mut legs);
        assert!(
            matches!(answer, Some(Err(StatementStopped::NotPlanned))),
            "{terms} terms: {answer:?}"
        );
        assert!(legs.opened.is_empty(), "{terms} terms opened {:?}", legs.opened);
    }
}
