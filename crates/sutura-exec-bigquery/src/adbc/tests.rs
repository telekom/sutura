//! The ADBC transport's own guards, asserted WITHOUT a driver - which is the whole design of
//! [`super::AdbcBigQuery::connect`] and the reason this file can exist at all.
//!
//! **`adbc.rs` had no test module before this one**, and a reviewer measured what that cost: `&& false`
//! on either refusal in it left the suite green. Every cell here dies if its guard is neutralised,
//! and the last one is the negative control that stops the others passing for the wrong reason - a
//! path naming no `.so` makes the driver load fail, so a cell asserting a refusal has to show that
//! the refusal arrived INSTEAD of that failure.

use sutura_domain::calendar::Date;
use sutura_domain::identity::Secret;
use sutura_domain::warehouse::ParamValue;

use adbc_core::Statement;

use super::{AdbcBigQuery, AdbcError, Impersonation};
use crate::transport::{DatasetAddress, DatasetId, JobDeadline, JobIdentity, JobRequest, JobTransport as _, ProjectId};

/// A path that names no driver, so a load reached here always fails.
///
/// **That is the point rather than a nuisance:** every guard under test runs before the load, so a
/// cell that shows [`AdbcError::Uncovered`] over this path has shown the guard answered first.
const NO_DRIVER: &str = "/nonexistent/libadbc_driver_bigquery.so";

fn endpoint(impersonation: Impersonation) -> AdbcBigQuery {
    AdbcBigQuery::new(NO_DRIVER, impersonation)
}

fn impersonating() -> Impersonation {
    Impersonation::ThroughPool(
        super::subject::WorkloadPool::parse(
            "//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/a/providers/sso",
            "https://www.googleapis.com/auth/cloud-platform",
        )
        .expect("a provider resource and a scope are usable"),
    )
}

fn day(iso: &str) -> Date {
    Date::parse(iso).expect("a test date is a date")
}

fn project() -> ProjectId {
    ProjectId::parse("acme-analytics").expect("a test project is a project")
}

fn dataset() -> DatasetId {
    DatasetId::parse("warehouse").expect("a test dataset is a dataset")
}

#[test]
fn a_subject_at_a_shared_source_is_refused_before_the_driver_is_even_loaded() {
    // **THE XOR's refusal, at the transport rather than at the option builder.** A source opened
    // shared declares no pool, so there is nothing to federate a subject's assertion against - and
    // the only alternative to refusing is opening a connection the DEPLOYMENT authenticated, which
    // is the fallback the owner rejected. `Uncovered` over a path naming no `.so` is how the
    // ordering is observable without a driver: the identity is decided first.
    let assertion = Secret::new("a.caller.assertion");
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new(
        "SELECT 1",
        &[],
        &project,
        &dataset,
        JobIdentity::AsSubject(&assertion),
        JobDeadline::Boot,
    );
    let refused = endpoint(Impersonation::Disabled)
        .run(&request)
        .expect_err("a shared source cannot federate a subject");
    assert!(matches!(refused, AdbcError::Uncovered(_)), "{refused:?}");
    assert!(!refused.to_string().contains("a.caller.assertion"), "{refused}");
}

#[test]
fn a_subject_at_an_impersonating_source_gets_as_far_as_the_driver() {
    // The other arm: the assertion is authenticated through a workload-identity document, so the
    // only thing left to fail is the LOAD. Without this cell the refusal above passes over a
    // transport that refused every subject for any reason.
    let assertion = Secret::new("a.caller.assertion");
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new(
        "SELECT 1",
        &[],
        &project,
        &dataset,
        JobIdentity::AsSubject(&assertion),
        JobDeadline::Boot,
    );
    let failed = endpoint(impersonating())
        .run(&request)
        .expect_err("no driver lives at this path");
    assert!(
        matches!(failed, AdbcError::Load(_)),
        "an impersonating leg must reach the driver rather than be refused: {failed:?}"
    );
}

#[test]
fn a_question_carrying_values_is_no_longer_refused_and_gets_as_far_as_the_driver() {
    // **The defect this replaces was the whole surface, not a corner.** Every question carries a
    // mandatory range, `Dialect::BigQuery` renders positional `?`, so every rendered statement has
    // bound values - and this transport used to answer `Uncovered("bind statement parameters")` to
    // all of them. A served deployment booted clean and answered nothing.
    //
    // The values are assembled BEFORE the load, like the identity, so what this cell shows is both
    // that the refusal is gone and that assembling them did not become the new way to fail: the
    // failure is the LOAD, over a path naming no `.so`.
    let params = [ParamValue::Text(String::from("north")), ParamValue::Date(day("2026-06-01"))];
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new(
        "SELECT ? AS region WHERE d >= ?",
        &params,
        &project,
        &dataset,
        JobIdentity::Transport,
        JobDeadline::Boot,
    );
    let failed = endpoint(Impersonation::Disabled)
        .run(&request)
        .expect_err("no driver lives at this path");
    assert!(
        matches!(failed, AdbcError::Load(_)),
        "a question with values must reach the driver rather than be refused: {failed:?}"
    );
}

#[test]
fn a_request_this_transport_accepts_gets_as_far_as_the_driver_and_fails_there() {
    // **THE NEGATIVE CONTROL for the three cells above.** Without it each of them passes over a
    // transport that refused everything for any reason, because the path names no `.so` either way.
    // A shared leg with no parameters is a request every guard accepts, so what it must fail on is
    // the LOAD - a different variant, reached only after all three guards let it through.
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new("SELECT 1", &[], &project, &dataset, JobIdentity::Transport, JobDeadline::Boot);
    let failed = endpoint(Impersonation::Disabled)
        .run(&request)
        .expect_err("no driver lives at this path");
    assert!(matches!(failed, AdbcError::Load(_)), "{failed:?}");
}

#[test]
fn a_path_that_names_no_driver_is_a_load_failure_and_not_a_silent_pass() {
    // **`probe`'s whole job, and the reason it exists at boot.** Reading the environment variable
    // says a path was written down; this says the `.so` at it is this ABI and its runtime started.
    // A static-musl binary answers here too - it has no dynamic loader - which is what makes the
    // musl limit assertable by running the artefact rather than by a sentence.
    let failed = AdbcBigQuery::probe(NO_DRIVER).expect_err("no driver lives at this path");
    assert!(matches!(failed, AdbcError::Load(_)), "{failed:?}");
    // The path is an operator-written string on its way to a startup line; the refusal may carry it
    // (it is not a secret) but has to name the driver at all, or an operator reading a startup
    // failure cannot tell it apart from any other load problem.
    assert!(
        failed.to_string().contains("BigQuery ADBC driver"),
        "a load failure must say what could not be loaded: {failed}"
    );
}

#[test]
fn the_listing_this_transport_cannot_do_is_not_an_authorization_refusal() {
    // **BOTH directions, because the default answered this and no cell read it** - review measured
    // that flipping `listing_was_refused` left the whole suite green.
    //
    // `list_tables` needs no driver: it refuses before anything is opened, which is what makes this
    // the one port method assertable here at all. What the pair holds is the SPLIT: the listing
    // could not be done, and it was not REFUSED - so `serve::boot` warns and serves rather than
    // sending an operator to grant `bigquery.tables.list`, a permission that is not missing. Flip
    // the override and this cell reads a refusal over a listing nothing refused.
    let at = DatasetAddress::of(project(), project(), dataset());
    let endpoint = endpoint(Impersonation::Disabled);
    let refused = endpoint.list_tables(&at).expect_err("this transport cannot list a dataset");
    assert!(matches!(refused, AdbcError::Uncovered(_)), "{refused:?}");
    assert!(
        !endpoint.listing_was_refused(&refused),
        "a listing this transport never asked for was not refused by anybody"
    );
    // And the other direction on the same value, so a cell that only ever saw `false` cannot pass
    // over a predicate that answers `false` to everything: the deadline and result-size questions
    // are the two other predicates this error reaches, and neither may claim it either.
    assert!(!endpoint.result_did_not_fit(&refused), "{refused:?}");
    assert!(!endpoint.deadline_exceeded(&refused), "{refused:?}");
}

/// One bind call, as much of it as an assertion needs: the batch's shape and its column types.
///
/// Named because `Vec<(usize, usize, Vec<DataType>)>` is over this workspace's `type_complexity`
/// threshold - the same reason `crate::Mapped` exists.
type BoundBatch = (usize, usize, Vec<arrow_schema::DataType>);

/// A statement that records what it was asked, standing in for the driver's own.
///
/// **The whole reason [`super::prepared`] is generic**, and the reason it is a fake rather than a
/// hosted leg: `adbc_core::Statement` is a trait, so the one thing a driver would otherwise be
/// needed for - *was the batch this transport built actually bound* - is assertable in process.
/// Review measured what its absence cost: replacing the `bind` call with `drop(bound)` left the
/// clippy leg clean and 143 tests green.
struct Recording {
    /// The statement text `set_sql_query` was handed, in order.
    queries: Vec<String>,
    /// One entry per `bind`, each the batch's (column count, row count) and column types.
    bound: Vec<BoundBatch>,
}

impl Recording {
    fn new() -> Self {
        Self {
            queries: Vec::new(),
            bound: Vec::new(),
        }
    }
}

/// The failure every unreached method answers with, so a fake that is asked something this
/// transport does not ask fails loudly rather than returning a plausible value.
fn not_asked(what: &str) -> adbc_core::error::Error {
    adbc_core::error::Error::with_message_and_status(
        format!("the recording statement was asked to {what}, which `prepared` does not do"),
        adbc_core::error::Status::NotImplemented,
    )
}

impl adbc_core::Optionable for Recording {
    type Option = adbc_core::options::OptionStatement;

    fn set_option(&mut self, _key: Self::Option, _value: adbc_core::options::OptionValue) -> adbc_core::error::Result<()> {
        Err(not_asked("set an option"))
    }

    fn get_option_string(&self, _key: Self::Option) -> adbc_core::error::Result<String> {
        Err(not_asked("read a string option"))
    }

    fn get_option_bytes(&self, _key: Self::Option) -> adbc_core::error::Result<Vec<u8>> {
        Err(not_asked("read a bytes option"))
    }

    fn get_option_int(&self, _key: Self::Option) -> adbc_core::error::Result<i64> {
        Err(not_asked("read an integer option"))
    }

    fn get_option_double(&self, _key: Self::Option) -> adbc_core::error::Result<f64> {
        Err(not_asked("read a double option"))
    }
}

impl Statement for Recording {
    fn bind(&mut self, batch: arrow_array::RecordBatch) -> adbc_core::error::Result<()> {
        self.bound.push((
            batch.num_columns(),
            batch.num_rows(),
            batch
                .schema()
                .fields()
                .iter()
                .map(|field| field.data_type().clone())
                .collect(),
        ));
        Ok(())
    }

    fn bind_stream(&mut self, _reader: Box<dyn arrow_array::RecordBatchReader + Send>) -> adbc_core::error::Result<()> {
        Err(not_asked("bind a stream"))
    }

    fn execute(&mut self) -> adbc_core::error::Result<Box<dyn arrow_array::RecordBatchReader + Send + 'static>> {
        Err(not_asked("execute"))
    }

    fn execute_update(&mut self) -> adbc_core::error::Result<Option<i64>> {
        Err(not_asked("execute an update"))
    }

    fn execute_schema(&mut self) -> adbc_core::error::Result<arrow_schema::Schema> {
        Err(not_asked("read a result schema"))
    }

    fn execute_partitions(&mut self) -> adbc_core::error::Result<adbc_core::PartitionedResult> {
        Err(not_asked("execute partitions"))
    }

    fn get_parameter_schema(&self) -> adbc_core::error::Result<arrow_schema::Schema> {
        Err(not_asked("read a parameter schema"))
    }

    fn prepare(&mut self) -> adbc_core::error::Result<()> {
        Err(not_asked("prepare"))
    }

    fn set_sql_query(&mut self, query: impl AsRef<str>) -> adbc_core::error::Result<()> {
        self.queries.push(String::from(query.as_ref()));
        Ok(())
    }

    fn set_substrait_plan(&mut self, _plan: impl AsRef<[u8]>) -> adbc_core::error::Result<()> {
        Err(not_asked("set a Substrait plan"))
    }

    fn cancel(&mut self) -> adbc_core::error::Result<()> {
        Err(not_asked("cancel"))
    }
}

#[test]
fn the_batch_this_transport_built_is_the_batch_the_statement_is_bound_with() {
    // **THE CELL F3 ASKS FOR.** Every question carries values, so a transport that assembled a
    // batch and never bound it would send a statement full of unbound `?` - a driver error at best
    // and, if the driver defaulted them, a wrong answer. Reaching the real `bind` needs a driver;
    // reaching THIS one needs a trait, which is what `prepared` is generic over.
    let params = [ParamValue::Text(String::from("north")), ParamValue::Date(day("2026-06-01"))];
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new(
        "SELECT ? AS region WHERE d >= ?",
        &params,
        &project,
        &dataset,
        JobIdentity::Transport,
        JobDeadline::Boot,
    );
    let bound = super::bind::parameter_batch(request.params()).expect("a range is bindable");
    let mut statement = Recording::new();
    super::prepared(&mut statement, &request, bound).expect("the recording statement accepts both calls");
    assert_eq!(statement.queries, vec![String::from("SELECT ? AS region WHERE d >= ?")]);
    assert_eq!(
        statement.bound,
        vec![(2, 1, vec![arrow_schema::DataType::Utf8, arrow_schema::DataType::Date32])],
        "the statement was bound with something other than this request's own values"
    );
}

#[test]
fn a_call_carrying_no_values_binds_nothing_at_all() {
    // The other direction, and it is not the same assertion inverted: the driver takes a DIFFERENT
    // code path when nothing is bound, so a transport that bound an empty batch on the boot path
    // would send an empty parameter list rather than none. `bind` must not be reached at all.
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new(
        "SELECT SESSION_USER()",
        &[],
        &project,
        &dataset,
        JobIdentity::Transport,
        JobDeadline::Boot,
    );
    let bound = super::bind::parameter_batch(request.params()).expect("no values is not a failure");
    let mut statement = Recording::new();
    super::prepared(&mut statement, &request, bound).expect("the recording statement accepts the query");
    assert_eq!(statement.queries.len(), 1);
    assert!(
        statement.bound.is_empty(),
        "a call with no values bound something: {:?}",
        statement.bound
    );
}

#[test]
fn a_listing_this_transport_cannot_do_is_a_warning_and_not_a_startup_refusal() {
    // **The OUTCOME layer, which the predicate's own cell cannot reach, and the reason it is here
    // rather than over a fake: this is the SHIPPING transport.** `listing_was_refused` answering
    // `false` is a value; what a deployment feels is what the adapter makes of it at the seam
    // `sutura-cli`'s `serve::boot` reads - `Warehouse::preflight_was_refused` - and that is the
    // direction that would change if this transport's constant were ever right to be `true`.
    //
    // The consequence is the limit, stated here as well as at `list_tables`: on a `bigquery` source
    // a mistyped `table:` is not caught at boot. Flip the predicate and this cell reads a refusal,
    // and a correct deployment stops booting while an operator is sent to grant a permission that
    // is not missing. No driver is needed - `list_tables` refuses before anything is opened.
    let warehouse = crate::BigQueryWarehouse::over_adbc(
        sutura_domain::model::SourceName::parse("warehouse").expect("a source name is a name"),
        sutura_domain::source::SourcePosture::SharedServiceUser {
            declared: sutura_domain::source::SharedIdentityDeclared::of(
                sutura_domain::source::AcknowledgementReason::parse(
                    "this boot check reads the deployment's own identity and asks about no subject",
                )
                .expect("a reason is a reason"),
            ),
        },
        project(),
        dataset(),
        NO_DRIVER,
        Impersonation::Disabled,
    );
    let asked =
        std::iter::once(sutura_domain::model::QualifiedTable::parse("dim_customer").expect("a test table path parses")).collect();
    let refused = sutura_domain::warehouse::Warehouse::preflight(&warehouse, &asked)
        .expect_err("a transport that cannot list a dataset answers an error");
    assert!(
        !sutura_domain::warehouse::Warehouse::preflight_was_refused(&warehouse, &refused),
        "a listing nothing refused must not become a startup refusal naming a missing grant: {refused:?}"
    );
}
