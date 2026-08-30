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

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::identity::Presented;
use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, SourceName, TableName};
use sutura_domain::plan::{
    Executable, PlanBucket, PlanColumn, PlanFilter, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin, QueryPlan,
    StatementTables,
};
use sutura_domain::source::{AcknowledgementReason, ImpersonationCapability, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::{ParamValue, PreFlight, Value, Warehouse};

use crate::transport::{Cell, DatasetId, Field, FieldType, JobRequest, JobRows, JobTransport, ProjectId};
use crate::{BigQueryError, BigQueryWarehouse};

// ------------------------------------------------------------------------------ the fake ----

/// Never returned: this transport answers from a constant.
#[derive(Debug, thiserror::Error)]
#[error("the fake transport cannot fail")]
struct FakeCannotFail;

/// One request the fake was handed, taken apart into the four things a test asserts on.
///
/// A struct rather than a tuple, because a four-tuple of `String` is over the `type_complexity`
/// threshold this workspace tightened and is unreadable at the assertion anyway.
struct Asked {
    statement: String,
    params: Vec<String>,
    project: String,
    dataset: String,
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
}

impl Recording {
    fn answering(answer: JobRows) -> Self {
        Self {
            answer,
            seen: RefCell::new(Vec::new()),
            validated: RefCell::new(0),
        }
    }

    fn empty() -> Self {
        Self::answering(JobRows::of(Vec::new(), Vec::new(), 0))
    }

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

fn leg_of(posture: &SourcePosture) -> Presented {
    match *posture {
        SourcePosture::SharedServiceUser { ref declared } => Presented::SharedServiceUser {
            declared: declared.clone(),
        },
        SourcePosture::ImpersonationAtSource => panic!("the fixture postures are shared"),
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
fn this_adapter_declares_that_it_has_nowhere_for_a_subject_to_arrive() {
    // The declaration a boot check reads, pinned by value. Declaring the other way round "to leave
    // room" would report a leg as impersonated that ran shared.
    assert_eq!(
        <BigQueryWarehouse<Recording> as Warehouse>::IMPERSONATION,
        ImpersonationCapability::NoPlaceForASubject
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
fn credential_material_this_adapter_cannot_use_is_refused_before_anything_is_asked() {
    // Refused BEFORE the transport is reached, which is the half that matters: a statement prepared
    // as the wrong identity resolves against tables the asker may not be able to see.
    let warehouse = open(Recording::empty(), shared_posture());
    let plan = plan();
    for presented in [
        Presented::SubjectToken {
            material: sutura_domain::identity::Secret::new("an-exchanged-token"),
        },
        Presented::SubjectPrincipal {
            name: sutura_domain::identity::PrincipalName::parse("analyst_role").expect("a test name is a name"),
        },
    ] {
        let error = warehouse
            .execute(Executable::Query(&plan), &presented)
            .expect_err("a subject credential has nowhere to go here");
        assert!(matches!(error, BigQueryError::NoPlaceForASubject { .. }), "{error:?}");
    }
    assert!(
        warehouse.transport.seen.borrow().is_empty(),
        "nothing may reach the endpoint after a credential refusal"
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

/// One fact leg, for the arm that refuses one.
fn a_leg() -> sutura_domain::plan::LegPlan {
    let table = TableName::parse("fct_subscription_monthly").expect("a test table is a table");
    let column = |name: &str| PlanColumn::new(table.clone(), ColumnName::parse(name).expect("a test column is a column"));
    sutura_domain::plan::LegPlan::Fact {
        source: source(),
        metric: MetricName::parse("mrr").expect("a test metric is a metric"),
        table: table.clone().into(),
        joins: Vec::new(),
        bucket: PlanBucket::new(String::from("period"), Grain::Month, column("month")),
        keys: Vec::new(),
        terms: Vec::new(),
        filters: Vec::new(),
        params: Vec::new(),
        range: TimeRange::new(day("2026-06-01"), day("2026-07-01")).expect("a test range is a range"),
    }
}
