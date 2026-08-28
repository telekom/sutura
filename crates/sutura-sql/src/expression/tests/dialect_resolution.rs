//! Which authored fragment a dialect resolves to, and what happens when none does.
//!
//! A submodule of [`super`] rather than four more tests in it, and the reason is mechanical:
//! `cargo xtask max-lines` fails at a thousand lines under `crates/` and cannot be exempted, and
//! that file plus these cases is over it. The seam is a real one - everything here is about ONE
//! decision, made once per dialect in [`super::super::compile`]: given what a catalog authored,
//! which string does this dialect get, and is "none of them" an answer. The refusals that read a
//! fragment's SHAPE stay next to each other in the parent.

use sutura_domain::expression::DialectTag;

use super::super::compile;
use super::super::refusal::{Construct, ExpressionError};
use super::{authored, columns, table};
use crate::dialect::{ALL, Dialect};

#[test]
fn a_dialect_word_that_is_not_one_fails_the_load_rather_than_being_never_chosen() {
    // The failure mode this closes is the quiet one: with a `portable` fragment beside a misspelled
    // `postgresql`, resolution would hand Postgres the portable text and nothing anywhere would say
    // that the variant the author wrote for it was never read.
    let err = compile(
        &authored(&[("portable", "SUM(mrr_eur)"), ("postgresql", "SUM(mrr_eur)::double precision")]),
        &table(),
        &columns(),
    )
    .expect_err("postgresql is not a dialect this build renders for");
    match err {
        ExpressionError::UnknownDialect {
            ref tag, ref choices, ..
        } => {
            // The typed fields, not the prose. `choices` is the LIST this build renders for, so a
            // caller offering "did you mean" has the words rather than a sentence to split.
            assert_eq!(tag.as_str(), "postgresql");
            assert_eq!(choices, &ALL.to_vec());
            assert!(choices.contains(&Dialect::Postgres), "{choices:?}");
            assert!(choices.contains(&Dialect::ClickHouse), "{choices:?}");
        }
        ref other => panic!("expected an unknown dialect, got {other}"),
    }
    // And the sentence still reads, because the join is in the format rather than at the
    // construction site. The quoting is deliberate here and nowhere else in this enum: a tag
    // differing from a real one by a trailing space is what this refusal is most often about.
    let message = err.to_string();
    assert!(message.starts_with("\"postgresql\" is not a data system"), "{message}");
    assert!(message.contains("duckdb, postgres, clickhouse"), "{message}");
    assert!(message.contains("or portable"), "{message}");
}

#[test]
fn a_per_dialect_variant_is_chosen_over_portable_and_recorded() {
    // The escape hatch inside the escape hatch, and the traceability half. `sumIf` exists in
    // ClickHouse and nowhere else, `COUNT_IF` renders verbatim into Postgres where it does not
    // exist - which is exactly the class of thing a per-dialect variant is for.
    let compiled = compile(
        &authored(&[
            ("portable", "SUM(CASE WHEN status = 'active' THEN mrr_eur END)"),
            ("clickhouse", "sumIf(mrr_eur, status = 'active')"),
        ]),
        &table(),
        &columns(),
    )
    .expect("both variants compile");

    let click = compiled.for_dialect(Dialect::ClickHouse).expect("clickhouse resolved");
    assert_eq!(click.authored_for().as_str(), "clickhouse");
    assert!(click.sql().starts_with("sumIf("), "{}", click.sql());
    for dialect in [Dialect::DuckDb, Dialect::Postgres] {
        let rendering = compiled.for_dialect(dialect).expect("resolved");
        assert_eq!(rendering.authored_for().as_str(), "portable", "{dialect}");
        assert!(rendering.sql().starts_with("SUM(CASE"), "{}", rendering.sql());
    }
}

#[test]
fn a_dialect_with_no_variant_and_no_portable_fragment_is_refused_not_guessed() {
    // Where this departs from the importer it copies. Wren's OSI reader falls back to the first
    // non-empty variant, which hands a Postgres query a ClickHouse expression because it happened
    // to be listed first: a number computed by a definition nobody chose, under a certified name.
    let err = compile(
        &authored(&[("clickhouse", "sumIf(mrr_eur, status = 'active')")]),
        &table(),
        &columns(),
    )
    .expect_err("nothing was authored for duckdb or postgres");
    match err {
        ExpressionError::NoFragment {
            dialect,
            ref authored_for,
            ..
        } => {
            assert!(!matches!(dialect, Dialect::ClickHouse), "{dialect} was authored for");
            // The words a catalog wrote, as words. Recovering this from the message used to mean
            // splitting on ", " between two other clauses, which is a contract nothing checks.
            assert_eq!(
                authored_for.iter().map(DialectTag::as_str).collect::<Vec<&str>>(),
                ["clickhouse"]
            );
        }
        ref other => panic!("expected a missing fragment, got {other}"),
    }
    assert!(err.to_string().contains("authored for clickhouse, and none"), "{err}");
}

#[test]
fn every_variant_is_checked_even_the_ones_a_dialect_would_never_read() {
    // A per-dialect variant is not a way around the checks. Compiling walks every dialect in `ALL`,
    // and the ClickHouse variant here is refused when ClickHouse's turn comes rather than being
    // carried unexamined because DuckDB and Postgres had a portable fragment.
    let err = compile(
        &authored(&[
            ("portable", "SUM(CASE WHEN status = 'active' THEN mrr_eur END)"),
            ("clickhouse", "sumIf(mrr_eur, status = 'active') FILTER (WHERE churned)"),
        ]),
        &table(),
        &columns(),
    )
    .expect_err("the clickhouse variant carries a FILTER");
    match err {
        ExpressionError::Refused { ref tag, construct } => {
            // Which VARIANT was refused is the useful half here, and it is a `DialectTag` rather
            // than a `String` for the reason every construction site already held one.
            assert_eq!(tag.as_str(), "clickhouse");
            assert!(!tag.is_portable());
            assert_eq!(construct, Construct::AggregateFilter);
        }
        ref other => panic!("expected a refusal, got {other}"),
    }
}
