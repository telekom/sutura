//! What this adapter decides, exercised against a fake transport rather than a network.
//!
//! **The fake is the point rather than a shortcut.** Every refusal this adapter can produce has to be
//! provokable somewhere, and the ones that matter most - credential material it cannot carry, a leg
//! with no combiner, a column type nobody mapped, a non-finite double - are exactly the ones a live
//! endpoint would never hand back on demand. `Conventions` asks for fakes and not mocked HTTP; this is
//! why.
//!
//! What no test here can do is prove the WIRE. The implementor that speaks to the endpoint is
//! [`crate::adbc`], behind the default-off `adbc` feature, and its own suite proves what it builds
//! and reads over documents that are not the service's. Nothing in this repository has sent a
//! statement to a real project; `docs/adr/0017` records what a test could run against instead, and
//! `docs/adr/0018` records that it has not been.

use sutura_domain::calendar::TimeRange;
use sutura_domain::identity::Presented;
use sutura_domain::model::{ColumnName, Grain, MetricName, TableName};
use sutura_domain::plan::{Executable, PlanBindings, PlanBucket, PlanColumn, ResultLabel};
use sutura_domain::source::ImpersonationCapability;
use sutura_domain::warehouse::estimate::EstimatedBytes;
use sutura_domain::warehouse::{Accumulating, PreFlight, ResultBatches, Warehouse};

use std::sync::Arc;

use arrow_array::{ArrayRef, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema, SchemaRef};

use crate::transport::{JobDeadline, ListingTotal, NotShort, Shortfall};
use crate::{BigQueryError, BigQueryWarehouse};

/// The transports and fixtures these assertions are written against.
mod fakes;
mod preflight;
mod results;

use fakes::{
    Broken, ListingRefused, Paged, Recording, Refusing, TimedOut, a_subject_token, a_subject_token_naming_no_account, day,
    impersonating_posture, leg_of, one_column, open, other_posture, plan, plan_in_dataset, shared_posture, source, test_deadline,
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
    let deadline = test_deadline();
    drop(
        warehouse
            .execute(Executable::Query(&plan), &leg_of(&shared_posture()), deadline)
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
    // **F1: the port's own `Deadline` has to cross into `JobRequest` unmangled.** `execute` builds
    // `JobDeadline::Port(deadline)` and nothing else - a call site that silently handed the boot
    // path's `JobDeadline::Boot` instead (the exact regression `docs/adr/0029`'s BigQuery slice
    // exists to close) would read as `execute` still working, since `submit` opens a fresh window
    // from the configured bound either way, and only this assertion would catch it.
    assert_eq!(asked.deadline, JobDeadline::Port(deadline));
}

#[test]
fn a_table_naming_its_dataset_and_no_project_still_names_a_project_on_the_wire() {
    // **What `crate::resolve::resolve` closes, at the boundary that actually reaches the wire.**
    // Rendered as-is, `sales.fct_subscription_monthly` would leave BigQuery's own request-level
    // default to fill the missing project in - silently, and not necessarily where the model
    // actually lives. Resolving the plan before it renders means the STATEMENT says which
    // project, so a reviewer of the SQL text does not have to trust a request field beside it.
    let warehouse = open(Recording::empty(), shared_posture());
    let plan = plan_in_dataset();
    drop(
        warehouse
            .execute(Executable::Query(&plan), &leg_of(&shared_posture()), test_deadline())
            .expect("the fake answers"),
    );
    let seen = warehouse.transport.seen.borrow();
    let asked = seen.first().expect("the transport was asked once");
    assert!(
        asked
            .statement
            .contains("`acme-analytics`.`sales`.`fct_subscription_monthly`"),
        "the connection's own project was not filled in:\n{}",
        asked.statement
    );
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
            .execute(Executable::Query(&plan), &a_subject_token(token), test_deadline())
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
                .execute(Executable::Query(&plan), &a_subject_token(token), test_deadline())
                .expect("an impersonating source accepts a subject's own credential"),
        );
    }
    let seen = warehouse.transport.seen.borrow();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].subject.as_deref(), Some("exchanged-for-subject-a"));
    assert_eq!(seen[1].subject.as_deref(), Some("exchanged-for-subject-b"));
    assert_ne!(seen[0].subject, seen[1].subject);
}

/// An identity read's answer: one `session_user` column of `text`, as Arrow.
///
/// Here rather than in `fakes`, and it is the one fixture in this file that is: `fakes` is the
/// half a reverted implementation takes with it, so a fixture that lives there is one the causality
/// gate cannot see these assertions using. It is also three lines, and `one_column`'s column is
/// named `value` - which in an identity read would read as the value-mapping table's fixture
/// pointed at the wrong test.
fn identity_column(rows: Vec<Option<&str>>) -> ResultBatches {
    labelled("session_user", rows)
}

/// A one-column `text` result under any label, as Arrow.
fn labelled(label: &str, rows: Vec<Option<&str>>) -> ResultBatches {
    let schema: SchemaRef = Arc::new(Schema::new(vec![Field::new(label, DataType::Utf8, true)]));
    let count = rows.len();
    let array: ArrayRef = Arc::new(StringArray::from(rows));
    let mut accumulating = Accumulating::announcing(Arc::clone(&schema), count.max(1));
    accumulating
        .push(RecordBatch::try_new(schema, vec![array]).expect("a one-column fixture batch is rectangular"))
        .expect("a fixture batch carries its own schema");
    accumulating.finish()
}

/// The same, spelled from an identifier, for the two tests that assert on the value.
fn answered_as(who: &str) -> ResultBatches {
    identity_column(vec![Some(who)])
}

#[test]
fn a_session_users_debug_is_redacted_but_explicit_access_and_display_preserve_the_answer() {
    for answer in ["user@example.com", " USER@EXAMPLE.COM "] {
        let warehouse = open(Recording::answering(answered_as(answer)), shared_posture());
        let who = warehouse
            .session_user(&leg_of(&shared_posture()))
            .expect("the fake answers one identity");
        assert_eq!(who.as_str(), answer);
        assert_eq!(format!("{who}"), answer);
        assert_eq!(format!("{who:?}"), "SessionUser(<redacted>)");
        assert_eq!(format!("{who:#?}"), "SessionUser(<redacted>)");
    }
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
    assert_eq!(who.as_str(), "principal-a@example.com");
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
    assert_eq!(who.as_str(), "ci@example.com");
    let seen = warehouse.transport.seen.borrow();
    assert_eq!(seen.first().and_then(|asked| asked.subject.clone()), None);
    // **The boot-path control F1 asks for, on the one inherent method it is cheap to reach.** The
    // identity read is not part of the `Warehouse` port and has no caller's `Deadline` to carry, so
    // it hard-codes `JobDeadline::Boot` at its own call site (`identity_read.rs`) exactly as
    // `verify_anchor` does - and the seam that matters, `request.deadline()` crossing into
    // `JobRequest` unmangled, is the same one `dry_run`/`execute`'s own cells below prove for the
    // `Port` arm.
    assert_eq!(seen.first().map(|asked| asked.deadline), Some(JobDeadline::Boot));
}

#[test]
fn an_identity_read_that_is_not_one_identity_is_refused_and_the_refusal_quotes_nothing() {
    // Three shapes that are not an identity, and one property that matters more than any of them:
    // **the refusal carries the SHAPE and never the value.** The venue that runs this read writes
    // to a public workflow log, and the one thing this answer can contain is an account
    // identifier - so a refusal quoting what came back would be the disclosure the read exists to
    // check for.
    let two_rows = identity_column(vec![Some("principal-a@example.com"), Some("principal-b@example.com")]);
    let two_columns = {
        let schema: SchemaRef = Arc::new(Schema::new(vec![
            Field::new("session_user", DataType::Utf8, true),
            Field::new("extra", DataType::Utf8, true),
        ]));
        let mut accumulating = Accumulating::announcing(Arc::clone(&schema), 1);
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(vec!["principal-a@example.com"])) as ArrayRef,
                Arc::new(StringArray::from(vec!["principal-b@example.com"])) as ArrayRef,
            ],
        )
        .expect("a two-column fixture batch is rectangular");
        accumulating.push(batch).expect("a fixture batch carries its own schema");
        accumulating.finish()
    };
    for answer in [two_rows, identity_column(vec![None]), two_columns] {
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

// `an_identity_read_whose_page_is_short_of_its_own_total_is_the_documented_refusal` WENT WITH THE
// PAGING IT READ. It asserted `BigQueryError::Incomplete` - a delivered count below the endpoint's
// own reported `totalRows` - which only the deleted HTTP wire transport ever reported. An ADBC read
// streams the whole result and `run` drains the reader, so a truncated stream is an `Err` rather
// than a short answer; `docs/adr/0039` records why completeness is the drain. The SHAPE check the
// identity read still needs is asserted one cell up, which is the half that would be a wrong
// identity rather than a missing one.

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
fn a_principal_to_switch_to_is_refused_rather_than_run_on_this_deployments_own_connection() {
    // **The mechanism reversal, as a cell.** For two rounds this adapter DELIVERED this shape: the
    // driver's `target_principal` option impersonated a declared account from the deployment's own
    // application default credentials, so the connection was the deployment's and the subject's
    // credential was nowhere in the chain. The owner rejected it - *"indeed no fallback! we must
    // work with impersonation!!"* - so `JobIdentity` carries one subject arm now and there is no
    // spelling for a principal switch at all.
    //
    // `agrees_with` passes this shape, because a principal and an assertion are one POSTURE to the
    // domain, so only the adapter can say it cannot be delivered. Both credential-taking methods
    // are asked, because `deliverable`'s successor is shared and the pre-flight is where a missing
    // check would be noticed least.
    let warehouse = open(Recording::empty(), impersonating_posture());
    let plan = plan();
    let presented = Presented::SubjectPrincipal {
        name: sutura_domain::identity::PrincipalName::parse("bq-a@sutura.example.com").expect("a test name is a name"),
    };
    let refused = warehouse
        .execute(Executable::Query(&plan), &presented, test_deadline())
        .expect_err("a principal switch is not a mechanism this adapter has");
    assert!(matches!(refused, BigQueryError::NoPrincipalSwitch { .. }), "{refused:?}");
    let pre_flight = warehouse
        .dry_run(Executable::Query(&plan), &presented, test_deadline())
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
            .execute(Executable::Query(&plan), &presented, test_deadline())
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
        .execute(Executable::Query(&plan), &leg_of(&other_posture()), test_deadline())
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
    //
    // The fake is told to answer with a fixed estimate, so this asserts the CARRY - that the number
    // the transport returns from `validate` is the number that reaches `PreFlight::Accepted`, and not
    // only that the endpoint was asked. A transport that discarded what it decoded and a `dry_run`
    // that hardcoded `None` regardless of the answer would both still pass a test that only checked
    // `validated`; this fixture value is what tells them apart.
    let warehouse = open(Recording::empty().estimating(2048), shared_posture());
    let plan = plan();
    let deadline = test_deadline();
    let answered = warehouse
        .dry_run(Executable::Query(&plan), &leg_of(&shared_posture()), deadline)
        .expect("the fake validates");
    assert_eq!(
        answered,
        PreFlight::Accepted {
            estimated_bytes: Some(EstimatedBytes::parse(2048))
        }
    );
    assert_eq!(*warehouse.transport.validated.borrow(), 1);
    // F1's other half: `dry_run` builds `JobDeadline::Port(deadline)` too, and a pre-flight that
    // silently reopened a fresh window (the boot path's shape) instead of carrying the port's own
    // `Deadline` would still pass every other assertion in this file.
    assert_eq!(
        warehouse.transport.seen.borrow().first().map(|asked| asked.deadline),
        Some(JobDeadline::Port(deadline))
    );
}

#[test]
fn prices_dry_run_is_checked_against_this_adapters_own_dry_run_path() {
    // BigQuery's `execute_packs!` binding (`telekom/sutura#710`,
    // `crates/sutura-exec-bigquery/tests/conformance.rs`) DOES now run
    // `sutura_conformance::execute::a_preflight_that_accepts_is_followed_by_an_answer`'s own
    // `estimated_bytes.is_some() != W::PRICES_DRY_RUN` comparison against this adapter - but over
    // a CANNED transport, never a live endpoint. This cell is the same comparison against the
    // fake standing in for an endpoint that priced the dry run, kept beside the pack rather than
    // retired by it: it is what makes `BigQueryWarehouse`'s own `true` checked even where the
    // pack binding did not exist, and the pack binding still does not reach a live endpoint.
    //
    // **What this does NOT cover**: whether the REAL endpoint always prices one. The fake is told
    // to here; a live endpoint that silently stopped would still agree with this cell, and the
    // pack's own canned fixture no more reaches a live endpoint than this one does.
    let warehouse = open(Recording::empty().estimating(2048), shared_posture());
    let answered = warehouse
        .dry_run(Executable::Query(&plan()), &leg_of(&shared_posture()), test_deadline())
        .expect("the fake validates");
    let PreFlight::Accepted { estimated_bytes } = answered else {
        panic!("a fake told to price a dry run answers Accepted, got {answered:?}");
    };
    assert!(
        BigQueryWarehouse::<Recording>::dry_run_estimate_agrees_with_its_declaration(estimated_bytes),
        "PRICES_DRY_RUN declares true; a priced dry run must carry an estimate to agree with it"
    );
}

#[test]
fn a_dry_run_the_endpoint_rejects_is_not_reported_as_accepted() {
    // The other half, so the assertion above is not passing on a transport that cannot say no.
    let warehouse = open(Broken, shared_posture());
    let plan = plan();
    let error = warehouse
        .dry_run(Executable::Query(&plan), &leg_of(&shared_posture()), test_deadline())
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
        bucket: PlanBucket::new(ResultLabel::bucket(), Grain::Month, column("month")),
        keys: Vec::new(),
        terms: Vec::new(),
        bindings: PlanBindings::none(),
        range: TimeRange::new(day("2026-06-01"), day("2026-07-01")).expect("a test range is a range"),
    }
}

#[cfg(feature = "adbc")]
#[test]
fn a_dry_run_the_transport_declined_is_not_asked_rather_than_a_failed_question() {
    // **THE CELL FOR "a configured ADBC source can answer a question at all".** `sutura_app::answer`
    // calls `Warehouse::dry_run` before `execute` and turns an `Err` that is neither a spent
    // deadline nor a source refusal into `ServiceError::Warehouse` - so while this adapter mapped a
    // declined dry run to a failure, an ADBC-backed source answered NOTHING. Round 4 of
    // `telekom/sutura#929`'s review called that *adoption scaffolding, not an adopted transport*.
    //
    // `NotAsked` and never `Accepted`, which is the half a weaker fix would have got wrong: the
    // port's own documentation says a defaulted pre-flight reads as *this subject may run this
    // plan*. Asserted by VALUE, so an `Accepted { estimated_bytes: None }` fails here.
    //
    // The real `AdbcBigQuery` over a path naming no `.so`, deliberately: `validate` refuses before
    // the driver is loaded, so this exercises the shipped transport rather than a fake that would
    // have to restate the decision under test.
    let warehouse = open(
        crate::adbc::AdbcBigQuery::new(
            crate::adbc::DriverLocation::parse("/nonexistent/libadbc_driver_bigquery.so")
                .expect("an absolute path parses whether or not a file is there"),
            crate::adbc::Impersonation::Disabled,
            crate::adbc::BytesBilledCeiling::parse(1024 * 1024 * 1024).expect("a gibibyte is a usable ceiling"),
        ),
        shared_posture(),
    );
    let plan = plan();
    let answered = warehouse
        .dry_run(Executable::Query(&plan), &leg_of(&shared_posture()), test_deadline())
        .expect("a dry run the transport declined is not a question that failed");
    assert_eq!(answered, PreFlight::NotAsked);
}

#[test]
fn a_subject_token_with_no_declared_target_is_refused_rather_than_run_as_the_pool_principal() {
    // **The half-configured deployment, refused at the adapter seam.** A subject's own credential
    // with no account declared beside it has no `service_account_impersonation_url` to become, so
    // the only two things this adapter could do are refuse or let the question run as whatever
    // principal the declared pool resolves the subject to - and the second is a deployment whose
    // `impersonate` map says one thing while every caller executes as another. `telekom/sutura#929`
    // review: a security-critical setting must not be accepted and then ignored; silently widened
    // is the same defect from the other side.
    //
    // Not reachable from the broker this crate ships - that one mints the account off the map it
    // parsed - so this is the refusal for `Presented` being a public port, and the transport is
    // never asked at all.
    let warehouse = open(Recording::empty(), impersonating_posture());
    let plan = plan();
    let refused = warehouse
        .execute(
            Executable::Query(&plan),
            &a_subject_token_naming_no_account("an-assertion-with-no-account"),
            test_deadline(),
        )
        .expect_err("a subject's credential with no declared account is not a leg this adapter runs");
    assert!(
        matches!(refused, BigQueryError::NoImpersonationTarget { ref at } if at == "warehouse"),
        "{refused:?}"
    );
    assert!(
        warehouse.transport.seen.borrow().is_empty(),
        "the job reached the transport, so the question ran as somebody"
    );
    // The refusal reaches a log and may not carry the caller's assertion.
    assert!(!refused.to_string().contains("an-assertion-with-no-account"), "{refused}");
    // And the DECLARED direction is served by the same adapter, so this is not passing against a
    // path that refuses every subject.
    drop(
        warehouse
            .execute(
                Executable::Query(&plan),
                &a_subject_token("an-assertion-with-an-account"),
                test_deadline(),
            )
            .expect("a subject's credential with a declared account is a leg this adapter runs"),
    );
}
