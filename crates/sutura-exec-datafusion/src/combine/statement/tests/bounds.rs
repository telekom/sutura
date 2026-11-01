//! What a statement may not make the deployment build: the scalar functions whose output a caller
//! grows in one call are not in the session, so a call to one is refused while planning and no
//! source is opened. Each family is asked over one source, where it would otherwise run whole.

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
