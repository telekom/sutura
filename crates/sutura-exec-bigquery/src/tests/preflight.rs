//! The boot pre-flight: what the adapter concludes from a dataset's own table listing.
//!
//! **Split out of `tests.rs` when the identity cells for the ADBC principal switch pushed that file
//! past the unexemptable 1000-line cap.** A concept and not a half: every assertion here is about
//! `BigQueryWarehouse::preflight` - which tables a bundle names, what a listing said about its own
//! size, and which of those outcomes is a refusal rather than an absence. The identity, rendering
//! and dry-run cells stay in the parent, next to the fake they are written against.

use std::collections::BTreeSet;

use sutura_domain::model::QualifiedTable;
use sutura_domain::warehouse::preflight::TablesPresent;

use super::{
    BigQueryError, Broken, Executable, ListingRefused, ListingTotal, NotShort, Recording, Refusing, Shortfall, TimedOut,
    Warehouse as _, leg_of, open, plan, shared_posture, test_deadline,
};

/// The tables a bundle would ask about, as the port takes them.
fn asked(paths: &[&str]) -> BTreeSet<QualifiedTable> {
    paths
        .iter()
        .map(|raw| QualifiedTable::parse(raw).expect("a test table path parses"))
        .collect()
}

/// A listing that reported more tables than it named, as the transport would have parsed one.
fn short(reported: u64, identified: u64) -> ListingTotal {
    ListingTotal::Short(Shortfall::parse(reported, identified).expect("the test means a shortfall"))
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
fn a_listing_short_of_its_own_total_does_not_report_the_table_it_never_named_as_absent() {
    // **`telekom/sutura#275`, at the seam that decides it.** A dataset answering with no readable
    // table id beside a total claiming three is a listing with a gap in it, and the bundle's table
    // may be sitting in that gap - so it is UNACCOUNTED FOR and not absent. Before this the gap was
    // rounded down to zero and the boot refused saying every table in the bundle is missing: the
    // right direction, the wrong reason, and an operator sent to fix a catalog that was never wrong.
    //
    // What the answer must NOT be is `AllBut`, and `absent()` is the accessor a root would have read
    // to build that sentence - so it is asserted rather than left to the variant's name.
    let warehouse = open(
        Recording::empty().holding_with_total("acme-analytics/warehouse", &[], short(3, 0)),
        shared_posture(),
    );
    let answered = warehouse
        .preflight(&asked(&["dim_customer"]))
        .expect("a listing with a gap in it is still a listing the dataset answered");
    let TablesPresent::Unaccounted { tables, shortfall } = &answered else {
        panic!("a listing short of its own total leaves the table unaccounted for, and it said {answered:?}");
    };
    assert_eq!(
        tables.to_string(),
        "dim_customer",
        "the outcome names the table nothing was said about"
    );
    assert_eq!(
        shortfall.get(),
        3,
        "and how many tables the listing left out of its own total"
    );
    assert!(
        answered.absent().is_none(),
        "nothing here says the dataset does not hold the table: {answered:?}"
    );
}

#[test]
fn a_listing_that_is_not_short_of_its_own_total_cannot_be_called_short() {
    // **The door review reproduced `telekom/sutura#275` through, on an UNMUTATED tree.** The
    // variant's fields were public and `HeldTables::of` is a `pub const fn`, so a `Short` whose
    // reported total sat BELOW its identified count was constructible; the pre-flight then computed
    // a shortfall of zero by saturating subtraction and fell back to reporting the bundle's tables
    // ABSENT - the exact defect being fixed, reached without touching a line of this crate. The
    // invariant belongs to the type now, so the fallback that needed it is gone with it.
    assert_eq!(
        Shortfall::parse(1, 5),
        Err(NotShort::Accounted {
            reported: 1,
            identified: 5
        }),
        "a total below the ids read is not a shortfall"
    );
    assert_eq!(
        Shortfall::parse(3, 3),
        Err(NotShort::Accounted {
            reported: 3,
            identified: 3
        }),
        "and neither is a total the ids read account for exactly"
    );
    let short = Shortfall::parse(4, 1).expect("four claimed beside one read is a shortfall");
    assert_eq!(short.unaccounted().get(), 3, "the gap is the count a decision reads");
    assert_eq!(
        (short.reported(), short.identified()),
        (4, 1),
        "and both totals survive the parse"
    );
}

#[test]
fn a_gap_of_one_does_not_claim_to_hide_three_tables() {
    // **A gap BOUNDS how many of the unnamed tables it can explain**, and the first version of this
    // decision printed the set and the shortfall as if they were one quantity - `shortfall: 1`
    // beside three tables, which is a sentence contradicting itself. Two of those three really are
    // missing; nothing here can say WHICH, because the listing named none of them. So the answer
    // carries both numbers and each root says *at most N of these M*.
    let warehouse = open(
        Recording::empty().holding_with_total("acme-analytics/warehouse", &["dim_plan"], short(4, 3)),
        shared_posture(),
    );
    let answered = warehouse
        .preflight(&asked(&["dim_customer", "fct_orders", "dim_region"]))
        .expect("the dataset answered");
    let TablesPresent::Unaccounted { tables, shortfall } = &answered else {
        panic!("a listing with a gap in it leaves the bundle's tables unaccounted for, and it said {answered:?}");
    };
    assert_eq!(tables.len(), 3, "three tables the listing never named: {tables}");
    assert_eq!(
        shortfall.get(),
        1,
        "and a gap of one, which is what bounds how many of them it explains"
    );
}

#[test]
fn a_listing_short_of_its_own_total_that_still_named_the_bundles_table_is_clean() {
    // **The half that keeps the decision narrow, and it is not symmetry.** A short listing cannot
    // un-name an entry it carried, so a table it DID name is a table the dataset really holds - and
    // a deployment whose bundle names only such tables is not stopped by a gap over tables it never
    // asked about. Refusing here would redden an ordinary boot for a total that moved while a
    // dataset was being written to.
    let warehouse = open(
        Recording::empty().holding_with_total("acme-analytics/warehouse", &["dim_customer"], short(5, 1)),
        shared_posture(),
    );
    let answered = warehouse.preflight(&asked(&["dim_customer"])).expect("the dataset answered");
    assert_eq!(
        answered,
        TablesPresent::All,
        "the listing named the table the bundle asks about, whatever its total said about the rest"
    );
}

#[test]
fn a_listing_that_accounted_for_itself_still_names_a_table_that_is_not_there() {
    // The control that stops the change above from being *nothing is ever absent again*: a listing
    // whose own total agrees with the ids it carried has no gap, so a table missing from it is
    // missing, and the refusal an operator acts on is unchanged.
    let warehouse = open(
        Recording::empty().holding_with_total(
            "acme-analytics/warehouse",
            &["dim_customer"],
            ListingTotal::Accounted { reported: 1 },
        ),
        shared_posture(),
    );
    let answered = warehouse
        .preflight(&asked(&["dim_customer", "dim_prodcut"]))
        .expect("the dataset answered");
    assert_eq!(
        absent_names(&answered),
        vec![String::from("dim_prodcut")],
        "a listing that accounts for itself still says what it does not hold"
    );
}

#[test]
fn a_table_a_whole_listing_does_not_hold_outranks_a_gap_in_another_dataset() {
    // **Two datasets, two findings, one sentence** - and the definite one is the one a root prints,
    // because it is the one an operator can act on. Both outcomes stop the boot, so nothing serves
    // that would not have; what the precedence buys is that the actionable sentence is not held
    // behind the one that says *look at this*.
    let warehouse = open(
        Recording::empty()
            .holding_with_total("acme-analytics/warehouse", &[], short(3, 0))
            .holding_with_total("acme-analytics/reference", &[], ListingTotal::Accounted { reported: 0 }),
        shared_posture(),
    );
    let answered = warehouse
        .preflight(&asked(&["dim_customer", "reference.dim_plan"]))
        .expect("both datasets answered");
    assert_eq!(
        absent_names(&answered),
        vec![String::from("reference.dim_plan")],
        "the empty dataset that accounted for itself is the finding; the gap waits for the next boot"
    );
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
            String::from("acme-analytics:acme-analytics/reference"),
            String::from("acme-analytics:acme-analytics/warehouse")
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
fn successful_listing_diagnostics_keep_their_precedence_and_their_own_tables() {
    for (unreadable, counted) in [("alpha", "omega"), ("omega", "alpha")] {
        for definite_absence in [false, true] {
            let transport = Recording::empty()
                .holding_with_total(
                    &format!("acme-analytics/{unreadable}"),
                    &[],
                    ListingTotal::Unreadable { identified: 0 },
                )
                .holding_with_total(&format!("acme-analytics/{counted}"), &[], short(3, 0))
                .holding_with_total("acme-analytics/middle", &[], ListingTotal::Accounted { reported: 0 });
            let unknown_table = format!("{unreadable}.dim_unknown");
            let counted_table = format!("{counted}.dim_counted");
            let mut requested = asked(&[&unknown_table, &counted_table]);
            if definite_absence {
                requested.insert(QualifiedTable::parse("middle.dim_absent").expect("a table path"));
            }
            let answer = open(transport, shared_posture())
                .preflight(&requested)
                .expect("all listings answered");
            if definite_absence {
                assert_eq!(absent_names(&answer), vec![String::from("middle.dim_absent")]);
            } else {
                assert!(answer.was_asked());
                assert!(answer.absent().is_none());
                let TablesPresent::UnreadableInventory(tables) = answer else {
                    panic!("an unreadable inventory precedes a counted gap: {answer:?}");
                };
                assert_eq!(
                    tables.to_string(),
                    unknown_table,
                    "the counted dataset's tables must not be relabeled"
                );
            }
        }
    }
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
        vec![String::from("acme-analytics:acme-analytics/warehouse")]
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
        vec![String::from("acme-analytics:acme-analytics/warehouse")],
        "the addressable tables are still looked for - one bad path does not skip the source"
    );
}

#[test]
fn a_cross_project_model_is_listed_in_its_own_project_and_billed_to_the_source() {
    // **The mechanism the quota-project fix did not have, and review is why it is here.** That fix
    // made the listing send the SOURCE's billing project in `x-goog-user-project` and the DATASET's
    // own project in the request path; nothing could observe it, because `billed_to()` is read in one
    // place no test can call and every fixture passed the same string in both roles.
    //
    // A source declared `billing_project: acme-analytics` reading `partner-data.shared.dim_region`
    // is the whole case: the caller holds `serviceusage.services.use` on its own project and not on
    // the partner's, so attributing the read to `partner-data` would `403` a listing whose QUERY
    // works - a permanent warning on exactly the deployment shape this field was added for. Reverting
    // `list`'s header to `at.project()` cannot be caught here (that line is inside the untestable
    // wire call), but a `DatasetAddress` built with the roles swapped now is.
    let warehouse = open(
        Recording::empty()
            .holding("acme-analytics/warehouse", &["dim_customer"])
            .holding("partner-data/shared", &["dim_region"]),
        shared_posture(),
    );
    let answered = warehouse
        .preflight(&asked(&["dim_customer", "partner-data.shared.dim_region"]))
        .expect("both datasets answered");
    assert_eq!(answered, TablesPresent::All, "both tables are where the bundle says");
    assert_eq!(
        *warehouse.transport.listed.borrow(),
        vec![
            String::from("acme-analytics:acme-analytics/warehouse"),
            String::from("acme-analytics:partner-data/shared")
        ],
        "the dataset's own project is the one looked in; the source's is the one billed, for BOTH"
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
fn a_refused_job_and_an_unreachable_job_are_not_the_same_source_outcome() {
    // **The query-time sibling of `a_refused_listing_and_an_unreachable_one_are_not_the_same_outcome`,
    // asked of `execute` instead of `preflight`.** Both transports below fail, both fail as
    // `BigQueryError::Endpoint`, and the adapter cannot tell them apart - which is exactly why it
    // asks the transport the way `preflight_was_refused` does. A `403` on the job is one IAM grant
    // and fails identically on every retry; an endpoint that did not answer is a condition a retry
    // may pass. `source_refused` is what carries that distinction to the domain, which names the
    // `true` half `RefusalReason::SourceRefused` and never a `503` (the mono-path pairing is pinned
    // in `sutura_app`'s `a_source_that_refuses_the_statement_is_refused_not_a_transport_failure`).
    let refused = open(Refusing, shared_posture());
    let error = refused
        .execute(Executable::Query(&plan()), &leg_of(&shared_posture()), test_deadline())
        .expect_err("a refused job is a failure of the call");
    assert!(
        refused.source_refused(&error),
        "an authorization refusal at query time has to reach the domain as a source refusal: {error:?}"
    );

    // The control, and it is what stops this being a predicate that says yes to everything: the
    // same variant, an error the adapter cannot tell from the one above, and a transport that does
    // not claim the refusal. Answering `false` keeps it a retryable transport failure above the port.
    let broken = open(Broken, shared_posture());
    let error = broken
        .execute(Executable::Query(&plan()), &leg_of(&shared_posture()), test_deadline())
        .expect_err("an unreachable endpoint is a failure of the call");
    assert!(
        !broken.source_refused(&error),
        "an outage must stay a retryable transport failure, not become a source refusal: {error:?}"
    );
    // The same shape one predicate over: `deadline_exceeded` asks the transport for the same reason,
    // and this is the control - a transport that never claimed the deadline running out stays `false`.
    assert!(
        !broken.deadline_exceeded(&error),
        "an outage must not answer `deadline_exceeded`: {error:?}"
    );
}

#[test]
fn the_ports_deadline_running_out_is_reported_through_the_transport() {
    // The half `a_refused_job_and_an_unreachable_job_are_not_the_same_source_outcome`'s control does
    // not reach: a transport that DOES claim the deadline running out has to have that answered back
    // as `true`.
    let timed_out = open(TimedOut, shared_posture());
    let error = timed_out
        .execute(Executable::Query(&plan()), &leg_of(&shared_posture()), test_deadline())
        .expect_err("the fake transport always refuses");
    assert!(
        timed_out.deadline_exceeded(&error),
        "the port's own deadline running out has to reach the domain as `deadline_exceeded`: {error:?}"
    );
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

#[test]
fn a_failed_listing_carries_the_transports_own_error_on_the_chain() {
    // **The regression guard for `#[source]` on the one variant `preflight` can fail with**, and it
    // is here rather than in the acceptance leg because that is the venue that costs a credential.
    // `BigQueryError::Endpoint` is the only error `preflight` produces - the single `map_err` on that
    // path - so a live assertion that the source is merely PRESENT could only ever fail if somebody
    // deleted the attribute, which is a hermetic property paid for over the network.
    //
    // **It asserts the source's CONTENT, which is what makes it more than its neighbour.**
    // `a_dry_run_the_endpoint_rejects_is_not_reported_as_accepted` already asserts `source().is_some()`
    // on the same variant off the `dry_run` path - so presence was covered and the listing path was
    // not, and neither asserted WHAT survived. What a root needs is the content:
    // `refuse_absent_tables` prints `flatten(cause)`, and *the data system did not answer* on its own
    // tells an operator nothing. The endpoint's own words are one link down.
    let refused = open(Refusing, shared_posture());
    let error = refused
        .preflight(&asked(&["dim_customer"]))
        .expect_err("a refused listing is a failure of the call");
    let source = core::error::Error::source(&error).expect("the transport's own error is on the chain");
    assert_eq!(
        source.to_string(),
        ListingRefused.to_string(),
        "the chain has to carry what the TRANSPORT said, not a second copy of the adapter's sentence"
    );
}
