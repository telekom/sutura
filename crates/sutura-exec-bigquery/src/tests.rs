//! What this adapter decides, exercised against a fake transport rather than a network.
//!
//! **The fake is the point rather than a shortcut.** Every refusal this adapter can produce has to be
//! provokable somewhere, and the ones that matter most - credential material it cannot carry, a leg
//! with no combiner, a column type nobody mapped, a non-finite double - are exactly the ones a live
//! endpoint would never hand back on demand. `Conventions` asks for fakes and not mocked HTTP; this is
//! why.
//!
//! What no test here can do is prove the WIRE. An implementor that speaks to the endpoint now exists,
//! [`crate::wire`], behind the default-off `wire` feature, and its own suite proves that it builds the
//! request it says it builds and reads the answer it says it reads, over documents that are not the
//! service's. Nothing in this repository has sent a statement to a real project; `docs/adr/0017`
//! records what a test could run against instead, and `docs/adr/0018` records that it has not been.

use std::collections::BTreeSet;

use sutura_domain::calendar::TimeRange;
use sutura_domain::identity::Presented;
use sutura_domain::model::{ColumnName, Grain, MetricName, QualifiedTable, TableName};
use sutura_domain::plan::{Executable, PlanBucket, PlanColumn};
use sutura_domain::source::ImpersonationCapability;
use sutura_domain::warehouse::preflight::TablesPresent;
use sutura_domain::warehouse::{PreFlight, Value, Warehouse};

use crate::transport::{Cell, Field, FieldType, JobRows, ListingTotal, NotShort, Shortfall};
use crate::{BigQueryError, BigQueryWarehouse};

/// The transports and fixtures these assertions are written against.
mod fakes;
mod results;

use fakes::{
    Broken, Case, ListingRefused, Paged, Recording, Refusing, a_subject_token, day, impersonating_posture, leg_of, one_cell,
    open, other_posture, plan, shared_posture, source,
};

// -------------------------------------------------------------------------------- tests ----

#[test]
fn this_adapter_declares_that_it_can_carry_a_subject() {
    // The declaration a boot check reads, pinned by value: `impersonation-at-source` may be opened
    // here, because a subject's own credential has somewhere to go - it rides as the job's bearer.
    assert_eq!(
        <BigQueryWarehouse<Recording> as Warehouse>::IMPERSONATION,
        ImpersonationCapability::PerSubjectCredential
    );
}

#[test]
fn the_statement_and_its_values_reach_the_transport_in_separate_fields() {
    // **The no-injection invariant at this boundary.** The generator keeps them apart and this is the
    // adapter that could put them back together; the request type has no merging constructor, and
    // this asserts the call site does not build one by hand.
    let warehouse = open(Recording::empty(), shared_posture());
    let plan = plan();
    drop(
        warehouse
            .execute(Executable::Query(&plan), &leg_of(&shared_posture()))
            .expect("the fake answers"),
    );
    let seen = warehouse.transport.seen.borrow();
    let asked = seen.first().expect("the transport was asked once");
    // The range bounds are bound, not written.
    assert_eq!(asked.params, vec![String::from("2026-06-01"), String::from("2026-07-01")]);
    assert!(!asked.statement.contains("2026"), "{}", asked.statement);
    assert_eq!(asked.statement.matches('?').count(), 2, "{}", asked.statement);
    // Rendered as GoogleSQL: backticks, and the bucket the way BigQuery spells it.
    assert!(asked.statement.contains("`fct_subscription_monthly`"), "{}", asked.statement);
    assert!(asked.statement.contains("MONTH)"), "{}", asked.statement);
    assert!(!asked.statement.contains('"'), "{}", asked.statement);
    // And the request carries where it is billed and where a bare table resolves.
    assert_eq!(asked.project, "acme-analytics");
    assert_eq!(asked.dataset, "warehouse");
}

#[test]
fn a_subjects_own_credential_is_sent_as_the_jobs_bearer_and_the_statement_runs_under_it() {
    // **The acceptance criterion, at the adapter boundary.** A source opened `impersonation-at-source`
    // accepts a `SubjectToken` - it does not refuse it - and forwards the token as THIS job's bearer.
    // It is that bearer, and not the adapter's own identity, that the endpoint evaluates the statement
    // against, which is what makes two subjects with different grants read different rows.
    let warehouse = open(Recording::empty(), impersonating_posture());
    let plan = plan();
    let token = "exchanged-for-subject-a";
    drop(
        warehouse
            .execute(Executable::Query(&plan), &a_subject_token(token))
            .expect("an impersonating source accepts a subject's own credential"),
    );
    let seen = warehouse.transport.seen.borrow();
    let asked = seen.first().expect("the transport was asked once");
    assert_eq!(asked.subject.as_deref(), Some(token));
}

#[test]
fn two_subjects_each_run_their_statement_under_the_bearer_minted_for_them() {
    // **The acceptance criterion, mechanised at the seam the end-to-end path depends on.** Two askers,
    // each with a credential minted for them, drive two jobs; each job's bearer is the asker's own and
    // never the other's. At a dataset with row-level security that is exactly what makes the two
    // subjects read different rows. The adapter holds the identity of neither asker; the `Presented`
    // value carries it, and the statement runs under it.
    let warehouse = open(Recording::empty(), impersonating_posture());
    let plan = plan();
    for token in ["exchanged-for-subject-a", "exchanged-for-subject-b"] {
        drop(
            warehouse
                .execute(Executable::Query(&plan), &a_subject_token(token))
                .expect("an impersonating source accepts a subject's own credential"),
        );
    }
    let seen = warehouse.transport.seen.borrow();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].subject.as_deref(), Some("exchanged-for-subject-a"));
    assert_eq!(seen[1].subject.as_deref(), Some("exchanged-for-subject-b"));
    assert_ne!(seen[0].subject, seen[1].subject);
}

/// The answer an identity read is supposed to get: one `session_user` column, one row, one cell.
///
/// Here rather than in `fakes`, and it is the one fixture in this file that is: `fakes` is the
/// half a reverted implementation takes with it, so a fixture that lives there is one the causality
/// gate cannot see these assertions using. It is also three lines, and `one_cell`'s column is named
/// `value` - which in an identity read would read as the value-mapping table's fixture pointed at
/// the wrong test.
fn one_identity(cell: Cell) -> JobRows {
    JobRows::of(
        vec![Field::of(String::from("session_user"), FieldType::String)],
        vec![vec![cell]],
        1,
    )
}

/// The same, spelled from an identifier, for the two tests that assert on the value.
fn answered_as(who: &str) -> JobRows {
    one_identity(Cell::Text(String::from(who)))
}

#[test]
fn the_identity_read_goes_out_under_the_subjects_own_bearer_and_carries_no_values() {
    // **The half of the exchanged-identity venue a fake CAN answer**, and it is the half that
    // decides whether that venue means anything: an identity read submitted under the credential
    // the TRANSPORT already holds would answer *the transport* every time and pass, whatever the
    // exchange did. So what is asserted is the bearer, not the answer.
    //
    // The statement is asserted too, because the answer is only an identity if the question was:
    // one fixed statement this crate renders, with no parameters, so there is no position a value
    // from a question could occupy - the *no arbitrary SQL entry point* property `load_fixture`
    // holds by taking a table name and a path.
    let warehouse = open(
        Recording::answering(answered_as("principal-a@example.com")),
        impersonating_posture(),
    );
    let token = "exchanged-for-principal-a";
    let who = warehouse
        .session_user(&a_subject_token(token))
        .expect("the fake answers one identity");
    assert_eq!(who, "principal-a@example.com");
    let seen = warehouse.transport.seen.borrow();
    let asked = seen.first().expect("the transport was asked once");
    assert_eq!(asked.subject.as_deref(), Some(token));
    assert!(asked.statement.contains("SESSION_USER()"), "{}", asked.statement);
    assert!(asked.params.is_empty(), "{:?}", asked.params);
    assert_eq!(asked.statement.matches('?').count(), 0, "{}", asked.statement);
}

#[test]
fn the_identity_read_under_a_shared_source_sends_no_bearer_of_its_own() {
    // The other posture, asserted rather than assumed because the exchanged-identity venue runs a
    // CONTROL leg under the deployment's own credential - and that leg is a control only if it
    // really goes out as the deployment. `subject_bearer` answers `None` for a shared leg, so the
    // transport's own identity is what the endpoint resolves, which is what makes *the deployment
    // is neither principal* a thing that venue can find out rather than assume.
    let warehouse = open(Recording::answering(answered_as("ci@example.com")), shared_posture());
    let who = warehouse
        .session_user(&leg_of(&shared_posture()))
        .expect("the fake answers one identity");
    assert_eq!(who, "ci@example.com");
    let seen = warehouse.transport.seen.borrow();
    assert_eq!(seen.first().and_then(|asked| asked.subject.clone()), None);
}

#[test]
fn an_identity_read_that_is_not_one_identity_is_refused_and_the_refusal_quotes_nothing() {
    // Three shapes that are not an identity, and one property that matters more than any of them:
    // **the refusal carries the SHAPE and never the value.** The venue that runs this read writes
    // to a public workflow log, and the one thing this answer can contain is an account
    // identifier - so a refusal quoting what came back would be the disclosure the read exists to
    // check for.
    let two_rows = JobRows::of(
        vec![Field::of(String::from("session_user"), FieldType::String)],
        vec![
            vec![Cell::Text(String::from("principal-a@example.com"))],
            vec![Cell::Text(String::from("principal-b@example.com"))],
        ],
        2,
    );
    let two_columns = JobRows::of(
        vec![
            Field::of(String::from("session_user"), FieldType::String),
            Field::of(String::from("extra"), FieldType::String),
        ],
        vec![vec![
            Cell::Text(String::from("principal-a@example.com")),
            Cell::Text(String::from("principal-b@example.com")),
        ]],
        1,
    );
    for answer in [two_rows, one_identity(Cell::Null), two_columns] {
        let warehouse = open(Recording::answering(answer), impersonating_posture());
        let refused = warehouse
            .session_user(&a_subject_token("exchanged-for-principal-a"))
            .expect_err("an answer that is not one identity is a refusal");
        assert!(matches!(refused, BigQueryError::NoIdentityInTheAnswer { .. }), "{refused:?}");
        let said = refused.to_string();
        for identifier in ["principal-a@example.com", "principal-b@example.com"] {
            assert!(
                !said.contains(identifier),
                "a refusal a public log will carry may not quote what came back: {said}"
            );
        }
    }
}

#[test]
fn an_identity_read_whose_page_is_short_of_its_own_total_is_the_documented_refusal() {
    // One comparison, in one place. `Incomplete` is this crate's documented reading of *the
    // endpoint delivered fewer rows than it reported*, and an identity read writing a second
    // comparison of its own beside it is the two-deadlines defect `sts::clears_floor` records -
    // two answers to one question, free to disagree.
    let short = JobRows::of(
        vec![Field::of(String::from("session_user"), FieldType::String)],
        vec![vec![Cell::Text(String::from("principal-a@example.com"))]],
        2,
    );
    let warehouse = open(Recording::answering(short), impersonating_posture());
    let refused = warehouse
        .session_user(&a_subject_token("exchanged-for-principal-a"))
        .expect_err("a page short of its own total is refused");
    assert!(
        matches!(refused, BigQueryError::Incomplete { delivered: 1, total: 2 }),
        "{refused:?}"
    );
}

#[test]
fn an_identity_read_whose_credential_disagrees_with_the_posture_reaches_no_endpoint() {
    // `deliverable` is shared by every credential-taking method, and this is the one added last -
    // so it is the one where forgetting it would be least visible. A shared source handed a
    // subject's token is refused here exactly as `execute` refuses it, and nothing is sent.
    let warehouse = open(Recording::empty(), shared_posture());
    let refused = warehouse
        .session_user(&a_subject_token("exchanged-for-principal-a"))
        .expect_err("a subject's token at a shared source disagrees with the posture");
    assert!(
        matches!(refused, BigQueryError::PresentedDisagreesWithPosture { .. }),
        "{refused:?}"
    );
    assert!(
        warehouse.transport.seen.borrow().is_empty(),
        "a leg this adapter cannot deliver may not reach the endpoint under any identity"
    );
}

#[test]
fn a_principal_to_switch_to_is_refused_rather_than_run_under_this_deployments_own_identity() {
    // **The shape this adapter declares it can carry a subject and still cannot deliver.** A
    // `SubjectPrincipal` is the SAME POSTURE as a subject token to the domain, so `agrees_with` passes
    // it at an `impersonation-at-source` source and only this adapter can say `BigQuery` has no
    // proxy-user mechanism to resolve it. Accepting it would submit the job under the credential the
    // transport already holds - `subject_bearer` has no material to send - while provenance, read off
    // this source's posture, reported the answer as impersonated: every row as the process, recorded
    // as the asker. Both credential-taking methods are asked, because `deliverable` is shared and the
    // pre-flight is where a missing check would be noticed least.
    let warehouse = open(Recording::empty(), impersonating_posture());
    let plan = plan();
    let presented = Presented::SubjectPrincipal {
        name: sutura_domain::identity::PrincipalName::parse("analyst_role").expect("a test name is a name"),
    };
    let refused = warehouse
        .execute(Executable::Query(&plan), &presented)
        .expect_err("a principal switch is not a bearer this adapter can send");
    assert!(matches!(refused, BigQueryError::NoPrincipalSwitch { .. }), "{refused:?}");
    let pre_flight = warehouse
        .dry_run(Executable::Query(&plan), &presented)
        .expect_err("the pre-flight refuses the same shape");
    assert!(
        matches!(pre_flight, BigQueryError::NoPrincipalSwitch { .. }),
        "{pre_flight:?}"
    );
    assert!(
        warehouse.transport.seen.borrow().is_empty(),
        "a leg this adapter cannot deliver may not reach the endpoint under any identity"
    );
}

#[test]
fn a_subjects_own_credential_on_a_shared_source_is_refused_by_the_posture_check() {
    // A source opened shared has nowhere for a subject's credential - it is the wrong SHAPE for the
    // posture, not a value this adapter cannot carry. Refused before the transport is reached, which
    // is the half that matters: a statement prepared as the wrong identity resolves against tables
    // the asker may not be able to see.
    let warehouse = open(Recording::empty(), shared_posture());
    let plan = plan();
    for presented in [
        a_subject_token("an-exchanged-token"),
        Presented::SubjectPrincipal {
            name: sutura_domain::identity::PrincipalName::parse("analyst_role").expect("a test name is a name"),
        },
    ] {
        let error = warehouse
            .execute(Executable::Query(&plan), &presented)
            .expect_err("a subject credential does not fit the shared posture");
        assert!(
            matches!(error, BigQueryError::PresentedDisagreesWithPosture { .. }),
            "{error:?}"
        );
    }
    assert!(
        warehouse.transport.seen.borrow().is_empty(),
        "nothing may reach the endpoint after a posture refusal"
    );
}

#[test]
fn a_shared_leg_carrying_another_acknowledgement_is_refused_rather_than_run() {
    // The SECOND half of `deliverable`, and a different question from the first: the shape is one
    // this adapter can carry, and the witness on it is not this source's. Without this the leg
    // executes and is then reported under the adapter's own declaration.
    let warehouse = open(Recording::empty(), shared_posture());
    let plan = plan();
    let error = warehouse
        .execute(Executable::Query(&plan), &leg_of(&other_posture()))
        .expect_err("another operator's acknowledgement is not this source's");
    assert!(
        matches!(error, BigQueryError::PresentedDisagreesWithPosture { .. }),
        "{error:?}"
    );
    assert!(warehouse.transport.seen.borrow().is_empty());
}

#[test]
fn a_dry_run_really_asks_the_endpoint_before_it_says_accepted() {
    // `PreFlight::Accepted` is a claim that the data system looked, and the port's own `NotAsked`
    // default exists so an adapter cannot make that claim without asking. This one can: a dry run at
    // this endpoint validates the query without using slots and without being charged.
    let warehouse = open(Recording::empty(), shared_posture());
    let plan = plan();
    let answered = warehouse
        .dry_run(Executable::Query(&plan), &leg_of(&shared_posture()))
        .expect("the fake validates");
    assert_eq!(answered, PreFlight::Accepted);
    assert_eq!(*warehouse.transport.validated.borrow(), 1);
}

#[test]
fn a_dry_run_the_endpoint_rejects_is_not_reported_as_accepted() {
    // The other half, so the assertion above is not passing on a transport that cannot say no.
    let warehouse = open(Broken, shared_posture());
    let plan = plan();
    let error = warehouse
        .dry_run(Executable::Query(&plan), &leg_of(&shared_posture()))
        .expect_err("a rejected dry run is not an acceptance");
    assert!(matches!(error, BigQueryError::Endpoint { .. }), "{error:?}");
    // The cause survives, so a caller that knows the transport can still read it.
    assert!(core::error::Error::source(&error).is_some());
}

/// One fact leg, for the arm that refuses one.
fn a_leg() -> sutura_domain::plan::LegPlan {
    let table = TableName::parse("fct_subscription_monthly").expect("a test table is a table");
    let column = |name: &str| PlanColumn::new(table.clone(), ColumnName::parse(name).expect("a test column is a column"));
    sutura_domain::plan::LegPlan::Fact {
        source: source(),
        metric: MetricName::parse("mrr").expect("a test metric is a metric"),
        tables: sutura_domain::plan::StatementTables::only(table.clone()),
        bucket: PlanBucket::new(String::from("period"), Grain::Month, column("month")),
        keys: Vec::new(),
        terms: Vec::new(),
        filters: Vec::new(),
        params: Vec::new(),
        range: TimeRange::new(day("2026-06-01"), day("2026-07-01")).expect("a test range is a range"),
    }
}

// ------------------------------------------------------------------- the boot pre-flight ----

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
