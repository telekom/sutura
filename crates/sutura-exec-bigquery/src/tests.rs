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

use core::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::identity::Presented;
use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, QualifiedTable, SourceName, TableName};
use sutura_domain::plan::{
    Executable, PlanBucket, PlanColumn, PlanFilter, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin, QueryPlan,
    StatementTables,
};
use sutura_domain::source::{AcknowledgementReason, ImpersonationCapability, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::{ParamValue, PreFlight, TablesPresent, Value, Warehouse};

use crate::transport::{
    Cell, DatasetAddress, DatasetId, Field, FieldType, HeldTables, JobRequest, JobRows, JobTransport, ProjectId,
};
use crate::{BigQueryError, BigQueryWarehouse};

// ------------------------------------------------------------------------------ the fake ----

/// Never returned: this transport answers from a constant.
#[derive(Debug, thiserror::Error)]
#[error("the fake transport cannot fail")]
struct FakeCannotFail;

/// One request the fake was handed, taken apart into the things a test asserts on.
///
/// A struct rather than a tuple, because a five-tuple of `String` is over the `type_complexity`
/// threshold this workspace tightened and is unreadable at the assertion anyway.
struct Asked {
    statement: String,
    params: Vec<String>,
    project: String,
    dataset: String,
    subject: Option<String>,
}

/// A transport that records what it was asked and answers with what a test handed it.
///
/// It records the STATEMENT and the PARAMETERS separately, which is what lets a test assert the
/// no-injection property at this boundary: an adapter that merged a value into the text would show up
/// as a statement carrying it and a parameter list one short.
struct Recording {
    answer: JobRows,
    seen: RefCell<Vec<Asked>>,
    validated: RefCell<usize>,
    /// What each dataset holds, keyed by the `project/dataset` pair a listing was asked for.
    ///
    /// A map rather than one set, because the pre-flight's whole claim is that it makes ONE call PER
    /// DATASET: a single set could not tell a bundle read with two calls from one read with one.
    holding: BTreeMap<String, BTreeSet<String>>,
    /// Which pairs were listed, in order, so a test can count the calls rather than trust them.
    listed: RefCell<Vec<String>>,
}

impl Recording {
    fn answering(answer: JobRows) -> Self {
        Self {
            answer,
            seen: RefCell::new(Vec::new()),
            validated: RefCell::new(0),
            holding: BTreeMap::new(),
            listed: RefCell::new(Vec::new()),
        }
    }

    /// The same fake, told which tables one dataset holds. Chainable, so two datasets are two calls.
    fn holding(mut self, pair: &str, tables: &[&str]) -> Self {
        self.holding
            .insert(String::from(pair), tables.iter().map(|table| String::from(*table)).collect());
        self
    }

    fn empty() -> Self {
        Self::answering(JobRows::of(Vec::new(), Vec::new(), 0))
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the recording fake exists to assert the exact bearer the adapter forwarded, which \
                  needs its text; production code never reads it"
    )]
    fn record(&self, request: &JobRequest<'_>) {
        self.seen.borrow_mut().push(Asked {
            statement: String::from(request.statement()),
            params: request
                .params()
                .iter()
                .map(|p| match *p {
                    ParamValue::Text(ref t) => t.clone(),
                    ParamValue::Date(d) => d.to_iso(),
                })
                .collect(),
            project: String::from(request.billing_project().as_str()),
            dataset: String::from(request.default_dataset().as_str()),
            // Exposed only here, in a test, where the whole point is to assert the exact bearer the
            // adapter forwarded. Production code never reads it as text.
            subject: request.subject_bearer().map(|secret| String::from(secret.expose_secret())),
        });
    }
}

impl JobTransport for Recording {
    type Error = FakeCannotFail;

    fn run(&self, request: &JobRequest<'_>) -> Result<JobRows, Self::Error> {
        self.record(request);
        Ok(self.answer.clone())
    }

    fn validate(&self, request: &JobRequest<'_>) -> Result<(), Self::Error> {
        self.record(request);
        *self.validated.borrow_mut() += 1;
        Ok(())
    }

    /// Answers from what a test handed over, and records which pair was asked.
    ///
    /// A pair nobody declared holds nothing, which is the honest fake for a dataset that exists and
    /// is empty - a dataset that cannot be LISTED is [`Broken`]'s answer, because those two are the
    /// outcomes the port keeps apart.
    fn list_tables(&self, at: &DatasetAddress) -> Result<HeldTables, Self::Error> {
        let pair = format!("{}/{}", at.project().as_str(), at.dataset().as_str());
        self.listed.borrow_mut().push(pair.clone());
        Ok(self.holding.get(&pair).cloned().unwrap_or_default())
    }

    /// Recorded like the other two, which is what lets a test assert what a fixture load PUT ON THE
    /// WIRE without an endpoint - the statement text, and that it carries no parameters.
    #[cfg(feature = "fixtures")]
    fn apply(&self, request: &JobRequest<'_>) -> Result<(), Self::Error> {
        self.record(request);
        Ok(())
    }
}

/// A transport that fails, for the one arm that needs the endpoint to say no.
struct Broken;

/// The endpoint's own failure, as a transport would report one.
#[derive(Debug, thiserror::Error)]
#[error("the endpoint refused the job")]
struct EndpointSaidNo;

impl JobTransport for Broken {
    type Error = EndpointSaidNo;

    fn run(&self, _request: &JobRequest<'_>) -> Result<JobRows, Self::Error> {
        Err(EndpointSaidNo)
    }

    fn validate(&self, _request: &JobRequest<'_>) -> Result<(), Self::Error> {
        Err(EndpointSaidNo)
    }

    /// **The `could not verify` outcome, and it is an `Err` rather than an empty listing.** A
    /// transport that answered `Ok(empty)` here would report every table in the bundle as absent and
    /// refuse a correct deployment, which is the collapse the port forbids in so many words.
    fn list_tables(&self, _at: &DatasetAddress) -> Result<HeldTables, Self::Error> {
        Err(EndpointSaidNo)
    }

    #[cfg(feature = "fixtures")]
    fn apply(&self, _request: &JobRequest<'_>) -> Result<(), Self::Error> {
        Err(EndpointSaidNo)
    }
}

// ----------------------------------------------------------------------------- fixtures ----

fn source() -> SourceName {
    SourceName::parse("warehouse").expect("a test source is a source")
}

fn day(iso: &str) -> Date {
    Date::parse(iso).expect("a test date is a date")
}

/// The posture this adapter is opened with in every test here.
///
/// Shared, and it is the honest declaration rather than a convenience: one service account reaches
/// the dataset for everybody who asks, which is what the capability constant says out loud.
fn shared_posture() -> SourcePosture {
    SourcePosture::SharedServiceUser {
        declared: SharedIdentityDeclared::of(
            AcknowledgementReason::parse("one service account reaching the dataset for everybody who asks")
                .expect("a fixture reason is a reason"),
        ),
    }
}

/// A posture whose acknowledgement is a DIFFERENT sentence, for the disagreement case.
fn other_posture() -> SourcePosture {
    SourcePosture::SharedServiceUser {
        declared: SharedIdentityDeclared::of(
            AcknowledgementReason::parse("some other operator's reason entirely").expect("a fixture reason is a reason"),
        ),
    }
}

/// The posture the per-subject tests open this adapter with.
fn impersonating_posture() -> SourcePosture {
    SourcePosture::ImpersonationAtSource
}

/// A token presented as the asker's own, for the impersonating posture.
fn a_subject_token(raw: &str) -> Presented {
    Presented::SubjectToken {
        material: sutura_domain::identity::Secret::new(raw),
    }
}

fn leg_of(posture: &SourcePosture) -> Presented {
    match *posture {
        SourcePosture::SharedServiceUser { ref declared } => Presented::SharedServiceUser {
            declared: declared.clone(),
        },
        SourcePosture::ImpersonationAtSource => a_subject_token("an-exchanged-token-for-the-asker"),
    }
}

fn open<T>(transport: T, posture: SourcePosture) -> BigQueryWarehouse<T>
where
    T: JobTransport,
{
    BigQueryWarehouse::new(
        source(),
        posture,
        ProjectId::parse("acme-analytics").expect("a test project is a project"),
        DatasetId::parse("warehouse").expect("a test dataset is a dataset"),
        transport,
    )
}

/// A plan the way the compiler builds one: the two range bounds as predicates, and their values as
/// the parameters those predicates index.
///
/// **The bounds are not optional decoration.** A `TimeRange` has no unbounded form, so every real
/// plan carries them, and the generator refuses a plan with no predicate at all - which is what a
/// first version of this fixture ran into. Carrying them here is also what gives the no-injection
/// assertion something to look for.
fn plan() -> QueryPlan {
    let table = TableName::parse("fct_subscription_monthly").expect("a test table is a table");
    let column = |name: &str| PlanColumn::new(table.clone(), ColumnName::parse(name).expect("a test column is a column"));
    QueryPlan::new(
        source(),
        MetricName::parse("mrr").expect("a test metric is a metric"),
        StatementTables::only(table.clone()),
        PlanBucket::new(String::from("period"), Grain::Month, column("month")),
        Vec::new(),
        PlanMeasure::Simple {
            term: PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: column("mrr_cents"),
            },
        },
        String::from("mrr"),
        vec![
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::AtOrAfter {
                    column: column("month"),
                    param: 0,
                },
            ),
            PlanFilter::new(
                PredicateOrigin::Definition,
                PlanPredicate::Before {
                    column: column("month"),
                    param: 1,
                },
            ),
        ],
        vec![ParamValue::Date(day("2026-06-01")), ParamValue::Date(day("2026-07-01"))],
        TimeRange::new(day("2026-06-01"), day("2026-07-01")).expect("a test range is a range"),
    )
}

/// One row of the shared value-mapping table: what the endpoint declared, what it sent, and the
/// domain value both SQL adapters have to produce for it.
///
/// Named because the tuple is over the `type_complexity` threshold this workspace tightened, exactly
/// as `sutura-exec-duckdb`'s own `Case` is.
type Case = (FieldType, Cell, Value);

/// One column and one row of it, for the value-mapping table.
fn one_cell(kind: FieldType, cell: Cell) -> JobRows {
    JobRows::of(vec![Field::of(String::from("value"), kind)], vec![vec![cell]], 1)
}

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

#[test]
fn every_type_this_adapter_maps_answers_what_the_other_sql_adapter_answers() {
    // **The value mapping as a table, and the duplication with `sutura-exec-duckdb` is deliberate.**
    // The two `cell` functions live in crates that may not depend on each other, so the agreement is
    // asserted as the same expected column written out in both places. Three arms below are the ones
    // where disagreeing would produce a wrong number rather than an error, and each says so.
    let warehouse = open(Recording::empty(), shared_posture());
    let cases: Vec<Case> = vec![
        (FieldType::Int64, Cell::Text(String::from("250")), Value::Integer(250)),
        (
            FieldType::String,
            Cell::Text(String::from("north")),
            Value::Text(String::from("north")),
        ),
        // A boolean becomes an integer, because the domain has no boolean and the other adapter maps
        // `BOOLEAN` to `Integer(i64::from(v))`. The example catalog counts a churn flag, so the two
        // would otherwise disagree about a metric.
        (FieldType::Bool, Cell::Text(String::from("true")), Value::Integer(1)),
        (FieldType::Bool, Cell::Text(String::from("false")), Value::Integer(0)),
        // An exact decimal stays TEXT. Turning it into a double is how a total that was correct in the
        // data system stops being correct in an answer.
        (
            FieldType::Numeric,
            Cell::Text(String::from("12345.67")),
            Value::Text(String::from("12345.67")),
        ),
        // A date is re-rendered from a parse, so a malformed one is an error rather than text that
        // looks like a date downstream.
        (
            FieldType::Date,
            Cell::Text(String::from("2026-06-01")),
            Value::Text(String::from("2026-06-01")),
        ),
        (FieldType::Int64, Cell::Null, Value::Null),
    ];
    for (kind, cell, expected) in cases {
        let rows = BigQueryWarehouse::<Recording>::rows(&one_cell(kind.clone(), cell.clone()))
            .unwrap_or_else(|e| panic!("{kind:?} with {cell:?} should map: {e}"));
        assert_eq!(rows.rows().first().and_then(|r| r.first()), Some(&expected), "{kind:?}");
    }
    // A double maps too, and is asserted separately because `Real` has no `Eq`.
    let rows = BigQueryWarehouse::<Recording>::rows(&one_cell(FieldType::Float64, Cell::Text(String::from("1.5"))))
        .expect("a finite double maps");
    match rows.rows().first().and_then(|r| r.first()) {
        Some(&Value::Real(real)) => assert!((real.get() - 1.5).abs() < f64::EPSILON),
        other => panic!("expected a real, got {other:?}"),
    }
    drop(warehouse);
}

#[test]
fn a_non_finite_double_is_refused_rather_than_answered() {
    // **The arm that keeps a stored non-finite value from answering under a certified number.** A
    // `FLOAT64` column holding a non-finite value is refused here - in GoogleSQL the unguarded `/`
    // raises on a zero divisor, so this arm is not the ratio case that `zero_denominator: fails`
    // carries on DuckDB; it is a STORED `Infinity`. The fixture spells the values the way the endpoint
    // does - `Infinity`/`-Infinity`/`NaN` - rather than the standard library's `inf`, so the test
    // keeps measuring the wire's shape.
    for hostile in ["Infinity", "-Infinity", "NaN"] {
        let error = BigQueryWarehouse::<Recording>::rows(&one_cell(FieldType::Float64, Cell::Text(String::from(hostile))))
            .expect_err("a non-finite double is refused");
        assert!(matches!(error, BigQueryError::NotFinite { .. }), "{hostile}: {error:?}");
    }
}

#[test]
fn a_type_this_adapter_does_not_map_names_itself_rather_than_answering_null() {
    // The reason `FieldType::Unmapped` carries the endpoint's own spelling: a null here would be a
    // wrong number, and a message that said "an unsupported type" would not say which column to fix.
    let error = BigQueryWarehouse::<Recording>::rows(&one_cell(
        FieldType::Unmapped(String::from("GEOGRAPHY")),
        Cell::Text(String::from("POINT(0 0)")),
    ))
    .expect_err("an unmapped type is refused");
    match error {
        BigQueryError::UnmappedType { ref column, ref named } => {
            assert_eq!(column, "value");
            assert_eq!(named, "GEOGRAPHY");
        }
        other => panic!("expected an unmapped-type refusal, got {other:?}"),
    }
}

#[test]
fn a_declared_number_that_did_not_come_back_as_one_is_refused() {
    // The endpoint sends every value as text, so "declared INT64" and "parses as an integer" are two
    // different facts and this is where they are reconciled.
    // Two variants rather than one, because the CAUSE differs and each keeps its own on the chain.
    let integer = BigQueryWarehouse::<Recording>::rows(&one_cell(FieldType::Int64, Cell::Text(String::from("not a number"))))
        .expect_err("a non-numeric value in an INT64 column is refused");
    assert!(matches!(integer, BigQueryError::NotAnInteger { .. }), "{integer:?}");
    let double = BigQueryWarehouse::<Recording>::rows(&one_cell(FieldType::Float64, Cell::Text(String::from("not a number"))))
        .expect_err("a non-numeric value in a FLOAT64 column is refused");
    assert!(matches!(double, BigQueryError::NotADouble { .. }), "{double:?}");
    // The standard-library cause survives, because `#[source]` is not wired for you.
    assert!(core::error::Error::source(&integer).is_some());
    assert!(core::error::Error::source(&double).is_some());
    // And a boolean that is neither spelling, which carries no cause because there is no parse
    // behind it.
    let boolean = BigQueryWarehouse::<Recording>::rows(&one_cell(FieldType::Bool, Cell::Text(String::from("yes"))))
        .expect_err("a BOOL column holding `yes` is refused");
    assert!(matches!(boolean, BigQueryError::NotABool { .. }), "{boolean:?}");
    // And a malformed date, which is the same argument on a different type.
    let error = BigQueryWarehouse::<Recording>::rows(&one_cell(FieldType::Date, Cell::Text(String::from("2026-13-45"))))
        .expect_err("a malformed date is refused");
    assert!(matches!(error, BigQueryError::NotADate { .. }), "{error:?}");
}

#[test]
fn a_row_at_the_wrong_width_is_refused_and_names_which_row() {
    // The endpoint disagreeing with its own schema. `RowSet::new` would catch it too; this catches it
    // first so the message can say which row, which is the difference between a usable failure and a
    // count.
    let rows = JobRows::of(
        vec![
            Field::of(String::from("a"), FieldType::Int64),
            Field::of(String::from("b"), FieldType::Int64),
        ],
        vec![
            vec![Cell::Text(String::from("1")), Cell::Text(String::from("2"))],
            vec![Cell::Text(String::from("3"))],
        ],
        2,
    );
    let error = BigQueryWarehouse::<Recording>::rows(&rows).expect_err("a ragged result is refused");
    match error {
        BigQueryError::RowWidth { row, cells, columns } => {
            assert_eq!((row, cells, columns), (1, 1, 2));
        }
        other => panic!("expected a row-width refusal, got {other:?}"),
    }
}

#[test]
fn a_result_shorter_than_what_the_endpoint_reported_is_refused() {
    // `jobs.query` answers ONE page; completeness is the endpoint's `totalRows`, never the rows alone.
    // A first page, or an incomplete job's empty `rows`, would otherwise read to `answer()` as *under
    // the cap, not truncated* - a wrong number under a certified name, through the exact row the
    // row-cap invariant exists to hold. This is the seam refusing it.
    let answered = JobRows::of(
        vec![
            Field::of(String::from("a"), FieldType::Int64),
            Field::of(String::from("b"), FieldType::Int64),
        ],
        vec![
            vec![Cell::Text(String::from("1")), Cell::Text(String::from("2"))],
            vec![Cell::Text(String::from("3")), Cell::Text(String::from("4"))],
        ],
        3,
    );
    let error = BigQueryWarehouse::<Recording>::rows(&answered).expect_err("a partial result is refused");
    match error {
        BigQueryError::Incomplete { delivered, total } => assert_eq!((delivered, total), (2, 3)),
        other => panic!("expected an incomplete-result refusal, got {other:?}"),
    }
}

#[test]
fn a_federated_leg_is_refused_because_there_is_nothing_above_it_to_combine_legs() {
    // A leg executed with nothing above it returns rows at a finer grouping than the question asked
    // for, which is a wrong number under a certified name. Both SQL adapters answer this the same way.
    let warehouse = open(Recording::empty(), shared_posture());
    let leg = crate::tests::a_leg();
    let error = warehouse
        .execute(Executable::Leg(&leg), &leg_of(&shared_posture()))
        .expect_err("a leg has no combiner above it");
    match error {
        BigQueryError::LegWithoutCombiner { ref table } => assert_eq!(table, "fct_subscription_monthly"),
        other => panic!("expected a leg refusal, got {other:?}"),
    }
    assert!(warehouse.transport.seen.borrow().is_empty());
}

/// A transport whose failure IS the endpoint declining to return the result at once.
///
/// Its own type rather than a flag on [`Broken`], because what is under test is that the adapter asks
/// the transport rather than guessing: two transports whose errors are indistinguishable to
/// `BigQueryError::Endpoint` and which answer the predicate differently is the only shape that can
/// show the delegation happening.
struct Paged;

/// One page of a larger result, as a transport would report it. Same shape as [`EndpointSaidNo`], and
/// that is the point: the adapter cannot tell them apart and does not try.
#[derive(Debug, thiserror::Error)]
#[error("the endpoint answered with one page of a larger result")]
struct OnePageOfMore;

impl JobTransport for Paged {
    type Error = OnePageOfMore;

    fn run(&self, _request: &JobRequest<'_>) -> Result<JobRows, Self::Error> {
        Err(OnePageOfMore)
    }

    fn validate(&self, _request: &JobRequest<'_>) -> Result<(), Self::Error> {
        Err(OnePageOfMore)
    }

    fn result_did_not_fit(&self, _error: &Self::Error) -> bool {
        true
    }

    // This fake is about a failure, so the listing fails the same way the others do.
    fn list_tables(&self, _at: &DatasetAddress) -> Result<HeldTables, Self::Error> {
        Err(OnePageOfMore)
    }

    // This fake is about a failure, so the fixtures method fails the same way the others do.
    #[cfg(feature = "fixtures")]
    fn apply(&self, _request: &JobRequest<'_>) -> Result<(), Self::Error> {
        Err(OnePageOfMore)
    }
}

#[test]
fn a_result_the_endpoint_would_not_return_at_once_is_a_size_bound_and_not_an_outage() {
    // THE defect the second bound exists for, at the adapter. `jobs.query` answers one page - as many
    // rows as fit the maximum permitted reply size - so a result UNDER the row cap can still be over
    // that, and both shapes that say so used to leave here as `BigQueryError` and reach a caller as
    // `503`: the status a dead endpoint produces, inviting a retry that returns the same page.
    //
    // Two shapes, and each is asked of the thing that knows. The page token is a fact about the wire
    // document, so the transport is asked - which is why the two failing transports below are
    // indistinguishable to `BigQueryError::Endpoint` and answer differently.
    let paged = open(Paged, shared_posture());
    let error = paged
        .execute(Executable::Query(&plan()), &leg_of(&shared_posture()))
        .expect_err("a paged result is not a result");
    assert!(
        paged.result_did_not_fit(&error),
        "a page of a larger result is a size bound: {error:?}"
    );

    // The control, and the reason this is not a test that says yes to everything: the same variant,
    // an error the adapter cannot tell from the one above, and a transport that does not claim the
    // bound. It must stay a failure.
    let broken = open(Broken, shared_posture());
    let refused = broken
        .execute(Executable::Query(&plan()), &leg_of(&shared_posture()))
        .expect_err("the endpoint said no");
    assert!(
        !broken.result_did_not_fit(&refused),
        "an endpoint that refused is not a result too large: {refused:?}"
    );
}

#[test]
fn a_page_shorter_than_the_reported_total_is_a_size_bound_and_a_longer_one_is_not() {
    // The second shape, and this one the ADAPTER decides: a delivered count BELOW the reported total
    // is the same bound reached without a page token, so it is a governance refusal rather than a
    // `503`. `a_result_shorter_than_what_the_endpoint_reported_is_refused` above asserts that it is
    // refused at all; this asserts what a caller is then told it was.
    let warehouse = open(Recording::empty(), shared_posture());
    assert!(
        warehouse.result_did_not_fit(&BigQueryError::Incomplete { delivered: 2, total: 3 }),
        "a partial page is a result too large"
    );

    // And the OTHER side of the same variant is deliberately NOT this bound. More rows delivered than
    // the endpoint says exist is the endpoint contradicting itself - a defect, which a retry may well
    // not repeat - so calling it a governance refusal would tell a caller not to retry the one shape
    // here where retrying could work.
    assert!(
        !warehouse.result_did_not_fit(&BigQueryError::Incomplete { delivered: 3, total: 2 }),
        "an endpoint contradicting itself is not a result too large"
    );
}

#[test]
fn a_schema_this_adapter_cannot_map_is_refused_whatever_the_data_happened_to_be() {
    // **The hole review found, and it was in the comment as well as in the code.** `cell` answers a
    // null BEFORE it reads the column's type, which is right for a null and wrong for the schema: a
    // result with NO rows never reaches `cell` at all, and a result whose unmapped column happens to
    // be entirely null reaches it and is answered. So a `TIMESTAMP` column came back as a successful
    // empty `RowSet`, and whether this adapter maps a type depended on what the data happened to be.
    //
    // Both shapes, because they were reachable for two different reasons.
    let empty = JobRows::of(
        vec![Field::of(String::from("at"), FieldType::Unmapped(String::from("TIMESTAMP")))],
        Vec::new(),
        0,
    );
    match BigQueryWarehouse::<Recording>::rows(&empty).expect_err("a zero-row unmapped schema is refused") {
        BigQueryError::UnmappedType { ref column, ref named } => {
            assert_eq!(column, "at");
            assert_eq!(named, "TIMESTAMP");
        }
        other => panic!("a zero-row unmapped schema was mapped to {other:?}"),
    }

    let all_null = JobRows::of(
        vec![Field::of(String::from("at"), FieldType::Unmapped(String::from("BYTES")))],
        vec![vec![Cell::Null], vec![Cell::Null]],
        2,
    );
    match BigQueryWarehouse::<Recording>::rows(&all_null).expect_err("an all-null unmapped column is refused") {
        BigQueryError::UnmappedType { ref named, .. } => assert_eq!(named, "BYTES"),
        other => panic!("an all-null unmapped column was mapped to {other:?}"),
    }

    // A malformed type name is the same case rather than a third one: an empty `type` decodes to
    // `Unmapped("")`, so it is named as what it is rather than read as a column that answers.
    let malformed = JobRows::of(vec![Field::of(String::from("at"), FieldType::parse(""))], Vec::new(), 0);
    assert!(
        matches!(
            BigQueryWarehouse::<Recording>::rows(&malformed),
            Err(BigQueryError::UnmappedType { .. })
        ),
        "an empty type name was accepted"
    );
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
fn a_table_path_this_adapter_cannot_address_is_refused_rather_than_skipped() {
    // **A path the DOMAIN accepts and this adapter cannot write into a request path.** The domain's
    // `ProjectName` is deliberately a UNION - it stands for a `BigQuery` project id and for a
    // standard catalog name, so it admits uppercase - while `ProjectId::parse` accepts `[a-z0-9-]`,
    // because that is what can go in a URL path segment. A model on such a path is one no question
    // could ever answer, so saying so at boot beats pretending the table might be there, and it must
    // not be quietly dropped from the set that gets compared.
    //
    // **Which of the two positions this reaches, stated because the other is not provokable today:**
    // the project one. The domain's `DatasetName` accepts `[A-Za-z_][A-Za-z0-9_]*`, which is inside
    // `DatasetId::parse`'s own set, so no dataset name a bundle can carry is refused there. The
    // dataset arm stays because the parse is one this adapter has to make either way, and a second
    // name type's rules are not something to depend on staying narrower.
    let warehouse = open(Recording::empty(), shared_posture());
    let hostile =
        QualifiedTable::parse("Acme-Analytics.warehouse.dim_customer").expect("a domain project name may be mixed case");
    let error = warehouse
        .preflight(&core::iter::once(hostile).collect())
        .expect_err("a project id this adapter cannot address is a refusal");
    match error {
        BigQueryError::UnusableTablePath { ref table, what, .. } => {
            assert_eq!(table, "Acme-Analytics.warehouse.dim_customer");
            assert_eq!(what, "project");
        }
        other => panic!("expected an unusable path, got {other:?}"),
    }
    assert!(
        warehouse.transport.listed.borrow().is_empty(),
        "the path is refused before a dataset is listed"
    );
}
