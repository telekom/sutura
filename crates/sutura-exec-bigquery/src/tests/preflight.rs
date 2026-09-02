//! What the boot pre-flight decides, over the same fake transport the rest of this suite uses.
//!
//! **A submodule of `super` rather than a file of its own, and the reason is the file-length gate:**
//! `tests.rs` crossed 1000 lines. The cut is at a real seam - everything here is about
//! `Warehouse::preflight` and nothing here is about answering a question - and the fakes stay in the
//! parent, because a second `Recording` is a second thing to keep in step.

use std::collections::BTreeSet;

use sutura_domain::model::QualifiedTable;
use sutura_domain::warehouse::Warehouse as _;
use sutura_domain::warehouse::preflight::TablesPresent;

use super::{Broken, Recording, Refusing, open, shared_posture};
use crate::BigQueryError;

/// The tables a bundle would ask about, as the port takes them.
fn asked(paths: &[&str]) -> BTreeSet<QualifiedTable> {
    paths
        .iter()
        .map(|raw| QualifiedTable::parse(raw).expect("a test table path parses"))
        .collect()
}

/// The names a pre-flight answer reported absent, for an assertion that reads.
fn absent_names(answered: &TablesPresent) -> Vec<String> {
    answered
        .absent()
        .map(|tables| tables.named().iter().map(ToString::to_string).collect())
        .unwrap_or_default()
}

#[test]
fn a_table_the_dataset_does_not_hold_is_named_and_the_rest_are_not() {
    // THE asymmetry issue 120 is about, at the adapter: a `files` deployment already refuses this at
    // boot because the engine is given a file per model, and a dataset had no equivalent step - so
    // the same mistyped name cost a boot refusal on one kind of deployment and a failed answer for
    // whoever asked first on the other.
    let warehouse = open(
        Recording::empty().holding("acme-analytics/warehouse", &["dim_customer", "fct_subscription_monthly"]),
        shared_posture(),
    );
    let answered = warehouse
        .preflight(&asked(&["dim_customer", "fct_subscription_monthly", "dim_prodcut"]))
        .expect("the dataset answered, so this is not a failure");
    assert_eq!(
        absent_names(&answered),
        vec![String::from("dim_prodcut")],
        "the answer names the table that is not there and nothing else"
    );
}

#[test]
fn a_bundle_whose_tables_are_all_there_is_asked_about_and_answered_clean() {
    // The control that makes the test above mean something, and the second half of it is the one that
    // matters: an adapter that ASKED and found everything answers `All`, which is not the `NotAsked`
    // the port defaults to. A composition root reads the difference.
    let warehouse = open(
        Recording::empty().holding("acme-analytics/warehouse", &["dim_customer"]),
        shared_posture(),
    );
    let answered = warehouse.preflight(&asked(&["dim_customer"])).expect("the dataset answered");
    assert_eq!(answered, TablesPresent::All);
    assert!(answered.was_asked(), "this adapter really looked: {answered:?}");
}

#[test]
fn one_call_per_dataset_and_not_one_per_model() {
    // The cost argument that made this check affordable, asserted rather than claimed: five models
    // over two datasets is two metadata reads. A call per model is what kept the check from existing,
    // and `AGENTS.md` recorded it as the price of closing the gap.
    let warehouse = open(
        Recording::empty()
            .holding("acme-analytics/warehouse", &["dim_customer", "fct_subscription_monthly"])
            .holding("acme-analytics/reference", &["dim_plan"]),
        shared_posture(),
    );
    let answered = warehouse
        .preflight(&asked(&[
            "dim_customer",
            "fct_subscription_monthly",
            "reference.dim_plan",
            "reference.dim_region",
        ]))
        .expect("both datasets answered");
    assert_eq!(
        *warehouse.transport.listed.borrow(),
        vec![
            String::from("acme-analytics/reference"),
            String::from("acme-analytics/warehouse")
        ],
        "two datasets are two calls, whatever the model count"
    );
    assert_eq!(
        absent_names(&answered),
        vec![String::from("reference.dim_region")],
        "the answer names the absent table with the path the bundle wrote"
    );
}

#[test]
fn an_unqualified_model_is_looked_for_in_the_dataset_the_source_was_opened_against() {
    // The same decision the request body's `defaultDataset` carries, in the one other place this
    // adapter has to resolve a bare name. Getting it wrong would look for every unqualified model in
    // a dataset nobody named and report a correct bundle as entirely absent.
    let warehouse = open(
        Recording::empty().holding("acme-analytics/warehouse", &["dim_customer"]),
        shared_posture(),
    );
    assert_eq!(
        warehouse.preflight(&asked(&["dim_customer"])).expect("the dataset answered"),
        TablesPresent::All
    );
    assert_eq!(
        *warehouse.transport.listed.borrow(),
        vec![String::from("acme-analytics/warehouse")]
    );
}

#[test]
fn a_dataset_that_cannot_be_listed_is_a_different_outcome_from_a_missing_table() {
    // **The separation the port states in so many words**, and the two mistakes it keeps apart are
    // not symmetric: an operator told *this table is absent* when the credential simply cannot list
    // the dataset fixes the catalog, which was never wrong. So a transport that could not ask is an
    // `Err` carrying its own cause, and never an answer naming tables.
    let warehouse = open(Broken, shared_posture());
    let error = warehouse
        .preflight(&asked(&["dim_customer"]))
        .expect_err("a dataset that cannot be listed is not an answer about its tables");
    assert!(
        matches!(error, BigQueryError::Endpoint { .. }),
        "the transport's own failure has to survive as the cause: {error:?}"
    );
}

#[test]
fn the_comparison_does_not_fold_case() {
    // `GoogleSQL` folds the case of an alias and a result column and does NOT fold a table name, so a
    // model naming `Dim_Customer` where the dataset holds `dim_customer` is a model whose questions
    // really would fail. Reporting it present because a folded comparison matched would put the
    // failure back on the first caller, which is the whole defect this check removes.
    let warehouse = open(
        Recording::empty().holding("acme-analytics/warehouse", &["dim_customer"]),
        shared_posture(),
    );
    let answered = warehouse.preflight(&asked(&["Dim_Customer"])).expect("the dataset answered");
    assert_eq!(absent_names(&answered), vec![String::from("Dim_Customer")]);
}

#[test]
fn a_table_path_this_adapter_cannot_address_is_reported_absent_and_stops_nothing_else() {
    // **A path the DOMAIN accepts and this adapter cannot write into a request path.** The domain's
    // `ProjectName` is deliberately a UNION - it stands for a `BigQuery` project id and for a
    // standard catalog name, so it admits uppercase - while `ProjectId::parse` accepts `[a-z0-9-]`,
    // because that is what can go in a URL path segment. A model on such a path is one no question
    // could ever answer.
    //
    // **This test asserted an `Err` and a name until review, and the shape it asserted was the
    // defect.** `preflight` propagated the error out of the GROUPING loop, before any dataset was
    // listed, and the composition root turns any `Err` into a `WARN` and serves - so one mixed-case
    // project id in a forty-model bundle turned the whole check off for that source. An
    // unaddressable path is a definite NEGATIVE rather than an unknown, so it is an absence: it
    // reaches the operator as a refusal naming the model, and the other tables on the source are
    // still checked. The second assertion below is the one that would have caught the original.
    let warehouse = open(
        Recording::empty().holding("acme-analytics/warehouse", &["dim_customer"]),
        shared_posture(),
    );
    let answered = warehouse
        .preflight(&asked(&[
            "Acme-Analytics.warehouse.dim_customer",
            "dim_customer",
            "fct_orders",
        ]))
        .expect("an unaddressable path is an answer about that table, not a failure of the call");
    // Ordered as `QualifiedTable`'s derived `Ord` orders them - the qualifier first, so a bare name
    // sorts before a qualified one. That is the ordering that type documents wanting: grouped by
    // where a table lives rather than by its own name.
    assert_eq!(
        absent_names(&answered),
        vec![
            String::from("fct_orders"),
            String::from("Acme-Analytics.warehouse.dim_customer")
        ],
        "the unaddressable path AND the genuinely missing table are both named"
    );
    assert_eq!(
        *warehouse.transport.listed.borrow(),
        vec![String::from("acme-analytics/warehouse")],
        "the addressable tables are still looked for - one bad path does not skip the source"
    );
}

#[test]
fn asking_about_no_tables_answers_not_asked_rather_than_all() {
    // `All` means *asked, nothing missing*. Nothing was asked, so it is not `All` - and `NotAsked`
    // is what a composition root reads as *nothing here verified anything*.
    let warehouse = open(Recording::empty(), shared_posture());
    assert_eq!(
        warehouse.preflight(&BTreeSet::new()).expect("an empty set is not a failure"),
        TablesPresent::NotAsked
    );
    assert!(warehouse.transport.listed.borrow().is_empty(), "and nothing was listed");
}

#[test]
fn a_refused_listing_and_an_unreachable_one_are_not_the_same_outcome() {
    // **The predicate a review asked for**, at the adapter. Both transports below fail, both fail
    // as `BigQueryError::Endpoint`, and the adapter cannot tell them apart - which is exactly why it
    // asks the transport, the way `result_did_not_fit` does. A `403` on `tables.list` is one IAM
    // grant and fails identically on every boot; an endpoint that did not answer passes.
    let refused = open(Refusing, shared_posture());
    let error = refused
        .preflight(&asked(&["dim_customer"]))
        .expect_err("a refused listing is a failure of the call");
    assert!(
        refused.preflight_was_refused(&error),
        "an authorization failure has to reach the root as a refusal: {error:?}"
    );

    // The control, and it is what stops this being a predicate that says yes to everything: the
    // same variant, an error the adapter cannot tell from the one above, and a transport that does
    // not claim the refusal.
    let broken = open(Broken, shared_posture());
    let error = broken
        .preflight(&asked(&["dim_customer"]))
        .expect_err("an unreachable endpoint is a failure of the call");
    assert!(
        !broken.preflight_was_refused(&error),
        "an outage must stay a warning, not become a boot refusal: {error:?}"
    );
}
