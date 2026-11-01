//! Which sources `run_sql` runs a statement over when nothing plans it across them.

use sutura_domain::raw::RawStatement;

use crate::run_sql;
use crate::tests::{asked_by_a_person, shared, source, test_deadline};
use crate::tests_support::{CountingBroker, RawCapableWarehouse};
use crate::warehouses::Warehouses;

/// Two sources and no plan across them: the statement is not handed to either one, and nothing is
/// minted for it - a failure of the wiring, never one source picked.
#[test]
fn run_sql_over_two_open_sources_runs_the_statement_on_neither() {
    let other = sutura_domain::model::SourceName::parse("other").expect("a test source is a source");
    let both = Warehouses::of(RawCapableWarehouse::answering_rows(source(), shared(), 1))
        .and(RawCapableWarehouse::answering_rows(other, shared(), 1))
        .expect("two distinct sources open together");
    let broker = CountingBroker::default();
    let statement = RawStatement::parse("select 1").expect("a test statement is a statement");
    let failure = run_sql(&asked_by_a_person(), &statement, &broker, &both, test_deadline())
        .expect_err("a statement no plan spans is not answered from one of two sources");
    assert!(
        failure.to_string().contains("more than one source"),
        "the failure names why: {failure}"
    );
    assert_eq!(broker.asked(), 0, "nothing was minted for a statement that ran nowhere");
}
