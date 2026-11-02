//! What a statement may not make the deployment build: a call to a scalar function whose output the
//! caller grows in one call, or more operators than a shallow tree needs. Either is refused while
//! planning, before any source opens; each is asked over one source, where it would otherwise run
//! whole.

use std::collections::BTreeSet;

use sutura_domain::model::SourceName;
use sutura_domain::plan::{StatementAnswer, StatementStopped};

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
    refused(&["select concat(region, encode('ab', 'hex')) from dim_region"]);
}

#[test]
fn a_type_spelled_by_a_string_is_not_planned() {
    refused(&[
        "select arrow_cast(region, 'Utf8') from dim_region",
        "select arrow_try_cast(region, 'Utf8') from dim_region",
    ]);
}

/// `refused`'s kept statement filtered by a constant chain of `terms` additions, which folds to true
/// while planning, so neither the part's text nor a source carries it. `upper` keeps a projection
/// over the scan: with a bare column the optimizer leaves only the scan, which the federation rule
/// does not cut into a part.
fn chained(terms: usize) -> String {
    format!("select upper(region) from dim_region where 0 < 1{}", "+1".repeat(terms))
}

/// Room for a chain just past the bound to plan with `recursive_protection` off, as on the base
/// tree: about 60 KiB a level. Not measured, so a chain that exhausts it aborts the cell instead.
const DEEP: usize = 64 << 20;

/// The sources each `open` named.
type Opened = Vec<BTreeSet<SourceName>>;

/// `chained(terms)`'s answer and the sources opened for it, asked on a thread of [`DEEP`] bytes.
fn asked_deep(terms: usize) -> (StatementAnswer<SourceName>, Opened) {
    std::thread::Builder::new()
        .stack_size(DEEP)
        .spawn(move || {
            let mut legs = Engines::new(None);
            (asked(&chained(terms), false, UNBOUNDED, &mut legs), legs.opened)
        })
        .expect("a test thread spawns")
        .join()
        .expect("asking does not panic")
}

/// A chain of `+` nests one level per operator without passing the parser's depth limit. One just
/// past the bound is refused before any source opens; it is asked on a deep stack, so without the
/// count it plans and the cell fails on its answer rather than aborting. Then, on this thread's own
/// stack, a chain near the longest the byte bound admits, which exhausted that stack while planning
/// when it was measured without the count, is refused, and one just under the bound plans and
/// answers.
#[test]
fn an_expression_chained_past_the_bound_is_not_planned() {
    let (answer, opened) = asked_deep(1_100);
    assert!(
        matches!(answer, Some(Err(StatementStopped::NotPlanned))),
        "1100 terms: {answer:?}"
    );
    assert!(opened.is_empty(), "1100 terms opened {opened:?}");
    let mut legs = Engines::new(None);
    let answer = asked(&chained(30_000), false, UNBOUNDED, &mut legs);
    assert!(
        matches!(answer, Some(Err(StatementStopped::NotPlanned))),
        "30000 terms: {answer:?}"
    );
    assert!(legs.opened.is_empty(), "30000 terms opened {:?}", legs.opened);
    let mut legs = Engines::new(None);
    let answer = asked(&chained(1_000), false, UNBOUNDED, &mut legs);
    assert!(matches!(answer, Some(Ok(_))), "a chain under the bound answers: {answer:?}");
}
