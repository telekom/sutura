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
use sutura_domain::warehouse::{ParamValue, UnannouncedBatch};

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use adbc_core::Statement;
use sutura_domain::warehouse::deadline::{Budget, Deadline};

use super::{AdbcBigQuery, AdbcError, DriverLocation, Impersonation};
use crate::transport::{DatasetAddress, DatasetId, JobDeadline, JobIdentity, JobRequest, JobTransport, ProjectId};

/// A path that names no driver, so a load reached here always fails.
///
/// **That is the point rather than a nuisance:** every guard under test runs before the load, so a
/// cell that shows [`AdbcError::Uncovered`] over this path has shown the guard answered first.
const NO_DRIVER: &str = "/nonexistent/libadbc_driver_bigquery.so";

/// [`NO_DRIVER`] as the parsed location a constructor takes.
fn nowhere() -> DriverLocation {
    DriverLocation::parse(NO_DRIVER).expect("an absolute path parses whether or not a file is there")
}

fn endpoint(impersonation: Impersonation) -> AdbcBigQuery {
    AdbcBigQuery::new(nowhere(), impersonation, a_gibibyte())
}

/// The money bound every cell here hands over, a gibibyte - the value `docs/adr/0017` uses and the
/// one every fixture in this workspace writes.
fn a_gibibyte() -> super::BytesBilledCeiling {
    super::BytesBilledCeiling::parse(1024 * 1024 * 1024).expect("a gibibyte is a usable ceiling")
}

fn impersonating() -> Impersonation {
    Impersonation::ThroughPool(
        super::subject::WorkloadPool::parse(
            "//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/a/providers/sso",
        )
        .expect("a provider resource is usable"),
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
    let account = crate::adbc::a_declared_account();
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new(
        "SELECT 1",
        &[],
        &project,
        &dataset,
        JobIdentity::AsSubject {
            assertion: &assertion,
            target: &account,
        },
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
    let account = crate::adbc::a_declared_account();
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new(
        "SELECT 1",
        &[],
        &project,
        &dataset,
        JobIdentity::AsSubject {
            assertion: &assertion,
            target: &account,
        },
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
    // **`probe`'s whole job, and the reason it exists at boot.** Resolving a location says a driver
    // was decided on; this says the driver at it is this ABI and its runtime started. A mounted path
    // is the only route a source build has, and this is that route's failure.
    let failed = AdbcBigQuery::probe(&nowhere()).expect_err("no driver lives at this path");
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
    // **NOT both directions - a constant `false` has no second direction to pin**, which is review's
    // correction to what this cell used to open with. What it holds is the value being READ at all:
    // the trait default answered this question and no cell looked, so flipping the override left the
    // whole suite green. Flip it now and this line dies.
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

#[test]
fn only_a_declined_dry_run_reads_as_one_and_the_listing_it_cannot_do_does_not() {
    // **The refusal `declined_to_dry_run` turns into an answer, and its control.** Until that
    // predicate existed, `validate`'s `Err` made every question against a configured ADBC source a
    // service error - `sutura_app::answer` calls `Warehouse::dry_run` before `execute` and turns
    // anything that is neither a spent deadline nor a source refusal into `ServiceError::Warehouse`.
    // The adapter's own half is `crate::tests`'s
    // `a_dry_run_the_transport_declined_is_not_asked_rather_than_a_failed_question`.
    //
    // Neither call needs a driver: both refuse before the `.so` is named, which is what makes them
    // assertable here at all.
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new("SELECT 1", &[], &project, &dataset, JobIdentity::Transport, JobDeadline::Boot);
    let endpoint = endpoint(Impersonation::Disabled);
    let declined = endpoint.validate(&request).expect_err("this transport prices nothing");
    assert!(matches!(declined, AdbcError::NoDryRun), "{declined:?}");
    assert!(endpoint.declined_to_dry_run(&declined));
    // **THE CONTROL, and the reason the predicate matches a VARIANT and not `Uncovered`'s text.**
    // `list_tables` answers `Uncovered` too, so a predicate keyed on a string would read a listing
    // this transport cannot do as a dry run it declined - which would turn a boot WARNING into a
    // pre-flight nobody made, and report `PreFlight::NotAsked` for a call that was not a dry run.
    let at = DatasetAddress::of(project.clone(), project.clone(), dataset.clone());
    let listing = endpoint.list_tables(&at).expect_err("this transport cannot list a dataset");
    assert!(!endpoint.declined_to_dry_run(&listing), "{listing:?}");
    // And the other direction on the declined value, so a cell over a predicate answering `true` to
    // everything cannot pass: neither of the two other predicates this error reaches may claim it.
    assert!(!endpoint.deadline_exceeded(&declined), "{declined:?}");
    assert!(!endpoint.result_did_not_fit(&declined), "{declined:?}");
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
///
/// `Clone`, so this fake can stand in for a real driver's statement in [`super::run_to_deadline`],
/// which needs its own `S: Clone` bound to hold a cancelling handle beside the one `execute` runs
/// on - a plain derive is honest here because nothing below reads a clone's copy of the recording
/// back; the cell that does need shared state (`Hanging`, further down) says so.
#[derive(Clone)]
struct Recording {
    /// The statement text `set_sql_query` was handed, in order.
    queries: Vec<String>,
    /// One entry per `bind`, each the batch's (column count, row count) and column types.
    bound: Vec<BoundBatch>,
    /// Every integer option set on this statement, as `(key, value)` in order.
    ///
    /// **Recorded rather than refused, unlike every other `Optionable` method here**, because the
    /// money bound is sent this way and a fake that errored on it would make
    /// `every_statement_this_transport_submits_carries_the_configured_bytes_billed_ceiling`
    /// unwritable. A non-integer option is still `not_asked`: this transport sets exactly one
    /// statement option and it is an integer.
    integer_options: Vec<(String, i64)>,
}

impl Recording {
    fn new() -> Self {
        Self {
            queries: Vec::new(),
            bound: Vec::new(),
            integer_options: Vec::new(),
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

    fn set_option(&mut self, key: Self::Option, value: adbc_core::options::OptionValue) -> adbc_core::error::Result<()> {
        match value {
            adbc_core::options::OptionValue::Int(value) => {
                self.integer_options.push((key.as_ref().to_owned(), value));
                Ok(())
            }
            _ => Err(not_asked("set a non-integer option")),
        }
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
    super::prepared(&mut statement, &request, bound, a_gibibyte(), Instant::now())
        .expect("the recording statement accepts both calls");
    assert_eq!(statement.queries, vec![String::from("SELECT ? AS region WHERE d >= ?")]);
    assert_eq!(
        statement.bound,
        vec![(2, 1, vec![arrow_schema::DataType::Utf8, arrow_schema::DataType::Date32])],
        "the statement was bound with something other than this request's own values"
    );
}

#[test]
fn every_statement_this_transport_submits_carries_the_configured_bytes_billed_ceiling() {
    // **THE CELL THE P1 ASKS FOR.** `sources.<alias>.max_bytes_billed` was required at boot,
    // parsed by nothing and sent nowhere, so the only key in the settings tree that spends money
    // bounded nothing: a declared `0` and a declared `u64::MAX` both booted green over a source
    // with no bound on bytes scanned at all.
    //
    // Asserted here rather than over a driver for `prepared`'s own reason - reaching the real
    // `set_option` needs one - and asserted by the option's exact KEY and VALUE, because a
    // misspelled key is not a refusal: the driver answers `NotImplemented` for an unknown statement
    // option, which would fail the job loudly, but a key that reaches a DIFFERENT option of the
    // driver's own would set something else and still bill the scan.
    //
    // Neutralise the `set_option` call in `prepared` - `if false { .. }`, `drop`, or a string value
    // instead of an integer - and this cell is the only thing that reddens.
    let project = project();
    let dataset = dataset();
    let request = JobRequest::new(
        "SELECT total FROM t",
        &[],
        &project,
        &dataset,
        JobIdentity::Transport,
        JobDeadline::Boot,
    );
    let mut statement = Recording::new();
    super::prepared(&mut statement, &request, None, a_gibibyte(), Instant::now())
        .expect("the recording statement accepts the ceiling");
    assert_eq!(
        statement.integer_options,
        vec![(String::from("bigquery.query.max_bytes_billed"), 1024 * 1024 * 1024_i64)],
        "the statement carried something other than this source's own `maximumBytesBilled`"
    );
    // And a DIFFERENT ceiling travels as itself, so the cell above cannot pass on a hard-coded
    // constant that happens to match the fixture.
    let mut other = Recording::new();
    let tighter = super::BytesBilledCeiling::parse(4096).expect("four kibibytes is a usable ceiling");
    super::prepared(&mut other, &request, None, tighter, Instant::now()).expect("the recording statement accepts the ceiling");
    assert_eq!(
        other.integer_options,
        vec![(String::from("bigquery.query.max_bytes_billed"), 4096_i64)]
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
    super::prepared(&mut statement, &request, bound, a_gibibyte(), Instant::now())
        .expect("the recording statement accepts the query");
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
        nowhere(),
        Impersonation::Disabled,
        a_gibibyte(),
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

#[test]
fn the_row_ceiling_is_a_size_bound_and_not_an_outage() {
    // **What replaced the page bound, and it is the same argument one layer down.** `jobs.query`
    // used to answer one page, so a result under the row cap could still be over the reply size, and
    // both shapes that said so reached a caller as `503` - the status a dead endpoint produces,
    // inviting a retry that returns the same page. An ADBC stream has no pages; what it has is
    // `MOST_RESULT_ROWS`, and a stream refused for crossing it is equally a result that did not fit:
    // the caller cannot get it whatever it retries.
    //
    // Asked of `AdbcBigQuery`'s own predicate, because that is where the port's default `false` was
    // wrong. The transport holds this, not the adapter above it.
    let over = AdbcError::Unannounced(UnannouncedBatch::OverBound {
        most: super::MOST_RESULT_ROWS,
    });
    assert!(
        JobTransport::result_did_not_fit(&endpoint(Impersonation::Disabled), &over),
        "a stream over the ceiling is a result that did not fit"
    );

    // THE CONTROL, and without it this test says yes to everything: a batch that disagreed with its
    // announced schema is the same variant and is NOT a size bound - no narrower request fixes a
    // driver contradicting its own schema, and a retry may not repeat it.
    let mislabelled = AdbcError::Unannounced(UnannouncedBatch::Mislabelled {
        at: 0,
        announced: String::from("orders Int64"),
        delivered: String::from("refunds Int64"),
    });
    assert!(
        !JobTransport::result_did_not_fit(&endpoint(Impersonation::Disabled), &mislabelled),
        "a mislabelled batch is not a result that did not fit"
    );
}

/// Both of this transport's own result ceilings, pinned in both directions.
///
/// **The row half was cited by `MOST_RESULT_ROWS`' own doc and did not exist.** That doc named
/// `tests::the_transports_own_ceiling_is_two_orders_of_magnitude_above_the_answer_cap` as what held
/// the value; nothing in this tree defines it, so the number was held by a sentence. The doc quoted
/// the measurement that made it necessary - raising the constant to `usize::MAX` left the whole suite
/// green, because `delivered + n > usize::MAX` is never true - and that measurement applies unchanged
/// to the byte half, which is why both are here.
///
/// Asserted as a RELATION to `MAX_ROWS` and to `docs/adr/0009`'s provisional working set rather than
/// as two literals: a cell repeating the constant passes whatever the constant becomes, which is the
/// same nothing the missing cell was providing.
#[test]
fn both_of_the_transports_own_result_ceilings_are_pinned_to_what_they_were_derived_from() {
    // Two orders of magnitude above the ANSWER cap, because a leg legitimately returns more rows
    // than the one answer re-aggregated above it keeps.
    assert_eq!(
        super::MOST_RESULT_ROWS,
        usize::try_from(sutura_domain::plan::MAX_ROWS).expect("ten thousand fits a usize") * 100
    );
    // A quarter of 0009's provisional 1 GiB working set, so two legs plus the combine above them
    // cannot each spend the whole of a query's provisional byte budget.
    assert_eq!(super::MOST_RESULT_BYTES * 4, 1024 * 1024 * 1024);
    // **Both equalities exclude `usize::MAX` by construction**, which is what makes them the fix for
    // the measurement above rather than a restatement of it: neither derived value can be the
    // saturating ceiling a suite cannot cross, so raising either constant to it fails here. Written
    // as equalities and not as a `< usize::MAX` pair, which `clippy::assertions_on_constants`
    // refuses - and rightly: an assertion the compiler folds away holds nothing.
}

/// A port deadline opened at `opened` with `budget` to spend.
fn deadline(opened: Instant, budget: Duration) -> Deadline {
    Deadline::opened_at(opened, Budget::parse(budget).expect("a non-zero budget parses"))
}

#[test]
fn a_request_time_statement_carries_what_is_left_of_its_deadline_as_the_job_timeout() {
    // **THE CELL `telekom/sutura#929`'s deadline finding asks for.** `JobDeadline::Port` reached
    // the request and nothing read it, so a question whose caller had long given up still ran to
    // completion on the service. Asserted by the option's exact KEY and VALUE for the reason the
    // bytes-billed cell above gives: a key reaching a different driver option would bound nothing.
    let project = project();
    let dataset = dataset();
    let opened = Instant::now();
    let now = opened + Duration::from_millis(1_500);
    let port = JobDeadline::Port(deadline(opened, Duration::from_secs(30)));
    let request = JobRequest::new("SELECT 1", &[], &project, &dataset, JobIdentity::Transport, port);
    let mut statement = Recording::new();
    super::prepared(&mut statement, &request, None, a_gibibyte(), now).expect("the recording statement accepts both bounds");
    assert_eq!(
        statement.integer_options,
        vec![
            (String::from("bigquery.query.max_bytes_billed"), 1024 * 1024 * 1024_i64),
            (String::from("bigquery.query.job_timeout"), 28_500_i64),
        ],
        "the job was not bounded by what is left of the caller's own deadline"
    );
    // A sub-millisecond remainder rounds UP: floored it would be `0`, which the driver reads as
    // no timeout at all - the one value this option must never carry.
    let mut nearly = Recording::new();
    let almost = opened + Duration::from_micros(29_999_990);
    super::prepared(&mut nearly, &request, None, a_gibibyte(), almost).expect("a remainder is still a remainder");
    assert_eq!(
        nearly.integer_options.last(),
        Some(&(String::from("bigquery.query.job_timeout"), 1_i64))
    );
    // THE CONTROL: the boot path has no caller and no port deadline, so it carries no job timeout -
    // a cell that saw the option on every statement could pass over a hard-coded constant.
    let boot = JobRequest::new("SELECT 1", &[], &project, &dataset, JobIdentity::Transport, JobDeadline::Boot);
    let mut booting = Recording::new();
    super::prepared(&mut booting, &boot, None, a_gibibyte(), now).expect("the boot path accepts the ceiling");
    assert_eq!(booting.integer_options.len(), 1, "{:?}", booting.integer_options);
}

#[test]
fn a_spent_deadline_sends_nothing_and_reads_as_the_deadline() {
    // What is left of a spent deadline is `0`, and a `jobTimeoutMs` of `0` is unbounded - so the
    // refusal has to land before any statement is prepared, and it has to read as the deadline
    // rather than a service error.
    let project = project();
    let dataset = dataset();
    let opened = Instant::now();
    let port = JobDeadline::Port(deadline(opened, Duration::from_secs(1)));
    let request = JobRequest::new("SELECT 1", &[], &project, &dataset, JobIdentity::Transport, port);
    let mut statement = Recording::new();
    let spent = super::prepared(&mut statement, &request, None, a_gibibyte(), opened + Duration::from_secs(2))
        .expect_err("a spent deadline is refused");
    assert!(matches!(spent, AdbcError::DeadlineSpent), "{spent:?}");
    assert!(statement.queries.is_empty(), "a statement was prepared past its deadline");
    let endpoint = endpoint(Impersonation::Disabled);
    assert!(endpoint.deadline_exceeded(&spent));
    assert!(!endpoint.deadline_exceeded(&AdbcError::NoDryRun));
}

/// A statement whose `execute` never returns on its own, standing in for a driver call this
/// process cannot finish waiting on.
///
/// **The whole reason [`super::run_to_deadline`] is generic over `S: Clone`.** `execute` and
/// `cancel` share ONE flag through the `Arc` every clone holds, which is what a real
/// `ManagedStatement`'s shared lock is standing in for: the only way `execute` below ever returns
/// is a `cancel` call reaching the SAME shared state, not a timeout of its own. A polled
/// `AtomicBool` rather than a condition variable - this workspace disallows `std::sync::Mutex`,
/// and a fake's own wait does not need to be efficient, only observable.
#[derive(Clone)]
struct Hanging {
    /// `true` once `cancel` has been asked for. `execute` polls this until it flips, which is
    /// also [`Self::cancelled`] - one flag serves both jobs, and a second field would only be
    /// able to disagree with it.
    cancelled: Arc<AtomicBool>,
}

impl Hanging {
    fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl adbc_core::Optionable for Hanging {
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

impl Statement for Hanging {
    fn bind(&mut self, _batch: arrow_array::RecordBatch) -> adbc_core::error::Result<()> {
        Err(not_asked("bind"))
    }

    fn bind_stream(&mut self, _reader: Box<dyn arrow_array::RecordBatchReader + Send>) -> adbc_core::error::Result<()> {
        Err(not_asked("bind a stream"))
    }

    fn execute(&mut self) -> adbc_core::error::Result<Box<dyn arrow_array::RecordBatchReader + Send + 'static>> {
        while !self.cancelled.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(2));
        }
        Err(not_asked("execute past being told to stop"))
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

    fn set_sql_query(&mut self, _query: impl AsRef<str>) -> adbc_core::error::Result<()> {
        Err(not_asked("set the SQL query"))
    }

    fn set_substrait_plan(&mut self, _plan: impl AsRef<[u8]>) -> adbc_core::error::Result<()> {
        Err(not_asked("set a Substrait plan"))
    }

    fn cancel(&mut self) -> adbc_core::error::Result<()> {
        self.cancelled.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn a_hung_execute_is_cancelled_and_the_call_returns_by_the_deadline() {
    // **THE CELL `telekom/sutura#929`'s first finding asks for.** Nothing here ever finishes
    // `execute` on its own - the ONLY way `run_to_deadline` below returns at all is the deadline
    // firing and asking this fake to `cancel`, which is what unblocks it. Carrying the deadline as
    // a value, unenforced, would hang this test rather than fail it.
    let hanging = Hanging::new();
    let cancelled = Arc::clone(&hanging.cancelled);
    let opened = Instant::now();
    let port = JobDeadline::Port(deadline(opened, Duration::from_millis(20)));
    let started = Instant::now();
    let result = super::run_to_deadline(hanging, (), port, opened);
    assert!(matches!(result, Err(AdbcError::DeadlineElapsed)), "{result:?}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the call outlived its 20ms deadline by {:?}",
        started.elapsed()
    );
    // The cancel a fired deadline asks for is deliberately ASYNCHRONOUS - see
    // `run_to_deadline`'s own doc for why it must not be waited on here - so give it a moment to
    // land before reading the flag it sets.
    let latest = Instant::now() + Duration::from_secs(10);
    while !cancelled.load(Ordering::SeqCst) && Instant::now() < latest {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        cancelled.load(Ordering::SeqCst),
        "the deadline elapsing must ask the driver to cancel"
    );
}

#[test]
fn a_fast_failure_within_the_deadline_is_not_read_as_the_deadline() {
    // **THE CONTROL.** A plentiful deadline beside a statement that fails IMMEDIATELY - `Recording`
    // refuses `execute` outright - has to surface as that failure and not as
    // `AdbcError::DeadlineElapsed`, or every ordinary error through this path would misreport as
    // the deadline the moment a caller happened to pass one.
    let statement = Recording::new();
    let opened = Instant::now();
    let port = JobDeadline::Port(deadline(opened, Duration::from_secs(30)));
    let failed = super::run_to_deadline(statement, (), port, opened).expect_err("the recording statement refuses `execute`");
    assert!(matches!(failed, AdbcError::Adbc(_)), "{failed:?}");
}
