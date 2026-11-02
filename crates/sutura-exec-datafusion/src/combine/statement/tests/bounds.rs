//! What a statement may not make the deployment build: a call to a scalar function whose output the
//! caller grows in one call, or more operators than a shallow tree needs. Either is refused while
//! planning, before any source opens; each is asked over one source, where it would otherwise run
//! whole.

use std::sync::Arc;

use datafusion::error::Result as Planned;
use datafusion::prelude::SQLOptions;
use sutura_domain::plan::StatementStopped;

use super::logged::recorded;
use super::{Engines, UNBOUNDED, asked, tables};
use crate::combine::statement::{Fetched, part_of, parts, session};
use crate::combine::working_set_of;

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

fn chained(terms: usize) -> String {
    format!("select region from dim_region where 0 < 1{}", "+1".repeat(terms))
}

/// The start of `text`: an engine's words can quote the whole chain.
fn clipped(text: &str) -> &str {
    text.get(..4096).unwrap_or(text)
}

/// Temporary, while the bound is measured: how far a chain of `terms` gets on this thread's stack
/// with the operator count skipped. Each stage is written to stderr before it starts, so an abort
/// names the stage it stopped in.
fn without_the_count(terms: usize) -> Planned<()> {
    let tables = tables();
    let context = session(&tables, &Arc::new(Fetched::default()), working_set_of(UNBOUNDED))?;
    let runtime = tokio::runtime::Builder::new_current_thread().build()?;
    eprintln!("{terms} terms without the count: planning");
    let plan = runtime.block_on(async {
        context
            .sql_with_options(&chained(terms), SQLOptions::new())
            .await?
            .into_optimized_plan()
    })?;
    eprintln!("{terms} terms without the count: displaying");
    eprintln!(
        "{terms} terms without the count: displayed {} bytes, cutting",
        plan.to_string().len()
    );
    let whole = part_of(&plan).is_some();
    for part in parts(&plan, &tables)? {
        eprintln!("{terms} terms without the count: unparsing a part");
        eprintln!(
            "{terms} terms without the count: unparsed {} bytes",
            part.statement(whole)?.as_str().len()
        );
    }
    eprintln!("{terms} terms without the count: dropping");
    drop(plan);
    eprintln!("{terms} terms without the count: dropped");
    Ok(())
}

/// A chain of `+` nests one level per operator without passing the parser's depth limit. One just
/// under the bound plans and answers on this thread's stack; one past it, and one long enough to
/// exhaust that stack without the bound, are refused before any source opens. While the bound is
/// measured, each chain under it is written to stderr with its answer and what it logged.
#[test]
fn an_expression_chained_past_the_bound_is_not_planned() {
    let under: Vec<_> = [10, 50, 100, 500, 1_000]
        .into_iter()
        .map(|terms| {
            let mut legs = Engines::new(None);
            let (answer, written) = recorded(|| asked(&chained(terms), false, UNBOUNDED, &mut legs));
            let written: Vec<_> = written
                .into_iter()
                .map(|(level, fields)| format!("{level} {}", clipped(&fields)))
                .collect();
            (terms, answer, legs.ran, written)
        })
        .collect();
    eprintln!("{under:#?}");
    let unbounded = without_the_count(30_000).map_err(|cause| clipped(&cause.to_string()).to_owned());
    assert!(
        under.iter().all(|(_, answer, ..)| matches!(answer, Some(Ok(_)))),
        "a chain under the bound answers: {under:#?}\n30000 terms without the count: {unbounded:?}"
    );
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
