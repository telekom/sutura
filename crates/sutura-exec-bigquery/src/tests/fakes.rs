//! The transports and fixtures every assertion in `super` is written against.
//!
//! **Split out of `tests.rs` when that file reached the length gate, and the cut is the same seam
//! `document.rs` is:** a harness on one side, assertions on the other. Nothing here asserts
//! anything - which is also what makes it the half that has to stay `pub(super)` rather than
//! private.
//!
//! **Why the ASSERTIONS did not move instead, since that was the first attempt and it was wrong:**
//! `cargo xtask test-causality` never reverts a file that added tests, and it does revert one that
//! did not. Moving the pre-flight's tests into a module of their own left `tests.rs` an
//! implementation-only file, so the gate reverted it, which deleted the `mod` declaration and
//! ORPHANED the new test file - the base tree then compiled with none of the new tests in it and
//! the gate reported *green against base behaviour*, which is a false green it is right to refuse.
//! Keeping the assertions here and moving the harness means `tests.rs` still carries `#[test]`s,
//! still stays at HEAD, and still cannot compile against a base with no `preflight` - the honest
//! INCONCLUSIVE this change is entitled to.

use core::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::identity::Presented;
use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, SourceName, TableName};
use sutura_domain::plan::{
    PlanBucket, PlanColumn, PlanFilter, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin, QueryPlan, ResultLabel,
    StatementTables,
};
use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::{ParamValue, Value};

use crate::BigQueryWarehouse;
use crate::transport::{
    Cell, DatasetAddress, DatasetId, Field, FieldType, HeldTables, JobRequest, JobRows, JobTransport, ListingTotal, ProjectId,
};

// ------------------------------------------------------------------------------ the fake ----

/// Never returned: this transport answers from a constant.
#[derive(Debug, thiserror::Error)]
#[error("the fake transport cannot fail")]
pub(super) struct FakeCannotFail;

/// One request the fake was handed, taken apart into the things a test asserts on.
///
/// A struct rather than a tuple, because a five-tuple of `String` is over the `type_complexity`
/// threshold this workspace tightened and is unreadable at the assertion anyway.
pub(super) struct Asked {
    pub(super) statement: String,
    pub(super) params: Vec<String>,
    pub(super) project: String,
    pub(super) dataset: String,
    pub(super) subject: Option<String>,
}

/// A transport that records what it was asked and answers with what a test handed it.
///
/// It records the STATEMENT and the PARAMETERS separately, which is what lets a test assert the
/// no-injection property at this boundary: an adapter that merged a value into the text would show up
/// as a statement carrying it and a parameter list one short.
pub(super) struct Recording {
    answer: JobRows,
    pub(super) seen: RefCell<Vec<Asked>>,
    pub(super) validated: RefCell<usize>,
    /// What each dataset holds, keyed by the `project/dataset` pair a listing was asked for.
    ///
    /// A map rather than one set, because the pre-flight's whole claim is that it makes ONE call PER
    /// DATASET: a single set could not tell a bundle read with two calls from one read with one.
    holding: BTreeMap<String, HeldTables>,
    /// Which addresses were listed, in order, so a test can count the calls rather than trust them.
    ///
    /// **`billed_to:project/dataset` and not the `project/dataset` [`Self::holding`] is keyed on,
    /// which is a review finding rather than a formatting choice.** The quota-project fix made
    /// `list` send `billed_to()` in the header and `project()` in the path, and no test could see
    /// it: this vector recorded two of the three fields, so a cross-project listing looked exactly
    /// like a same-project one. Recording all three makes the ROLE each project resolves to an
    /// assertion - see `a_cross_project_model_is_listed_in_its_own_project_and_billed_to_the_source`.
    pub(super) listed: RefCell<Vec<String>>,
}

impl Recording {
    pub(super) fn answering(answer: JobRows) -> Self {
        Self {
            answer,
            seen: RefCell::new(Vec::new()),
            validated: RefCell::new(0),
            holding: BTreeMap::new(),
            listed: RefCell::new(Vec::new()),
        }
    }

    /// The same fake, told which tables one dataset holds. Chainable, so two datasets are two calls.
    ///
    /// Keyed by `project/dataset`, because that is WHERE a table lives - the project a read is billed
    /// to cannot change which tables a dataset holds, and a key that carried it would say it could.
    /// Reports NO total, which is the honest answer for a fake that is not about that axis - the
    /// same direction `listing_was_refused` defaults in, and the one every caller behaved as if it
    /// had before the field was decoded at all. [`Self::holding_with_total`] is for a test that IS
    /// about it.
    pub(super) fn holding(self, pair: &str, tables: &[&str]) -> Self {
        self.holding_with_total(pair, tables, ListingTotal::Unreported)
    }

    /// The same, told what the listing said about its own size.
    ///
    /// The verdict is stated rather than computed here on purpose: `wire::tables::reported_total` is
    /// the one place that derives one from a document, and a fake deriving its own would be a second
    /// copy of the rule under test.
    pub(super) fn holding_with_total(mut self, pair: &str, tables: &[&str], total: ListingTotal) -> Self {
        let named: BTreeSet<String> = tables.iter().map(|table| String::from(*table)).collect();
        self.holding.insert(String::from(pair), HeldTables::of(named, total));
        self
    }

    pub(super) fn empty() -> Self {
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
        // All THREE fields recorded, and only the last two used to look the answer up: which project
        // pays is not a fact about which tables a dataset holds, and it is the one this fake was
        // blind to. See `Self::listed`.
        self.listed.borrow_mut().push(format!("{}:{pair}", at.billed_to().as_str()));
        // An undeclared pair is an empty dataset that reported nothing - the ambiguity the real
        // decoder now reports rather than hides. `HeldTables` has no `Default` for that reason: what
        // an absence means here is the question, so it is stated at the site.
        Ok(self
            .holding
            .get(&pair)
            .cloned()
            .unwrap_or_else(|| HeldTables::of(BTreeSet::new(), ListingTotal::Unreported)))
    }

    /// Recorded like the other two, which is what lets a test assert what a fixture load PUT ON THE
    /// WIRE without an endpoint - the statement text, and that it carries no parameters.
    #[cfg(feature = "fixtures")]
    fn apply(&self, request: &JobRequest<'_>) -> Result<(), Self::Error> {
        self.record(request);
        Ok(())
    }
}

/// A transport whose listing failure is the endpoint REFUSING, rather than failing to answer.
///
/// **Its own type rather than a flag on [`Broken`]**, for [`Paged`]'s reason one predicate over: two
/// transports whose errors are indistinguishable to `BigQueryError::Endpoint` and which answer the
/// predicate differently is the only shape that can show the delegation happening.
pub(super) struct Refusing;

/// The endpoint declining to list a dataset. Same shape as [`EndpointSaidNo`], which is the point.
#[derive(Debug, thiserror::Error)]
#[error("the endpoint refused to list the dataset")]
pub(super) struct ListingRefused;

impl JobTransport for Refusing {
    type Error = ListingRefused;

    fn run(&self, _request: &JobRequest<'_>) -> Result<JobRows, Self::Error> {
        Err(ListingRefused)
    }

    fn validate(&self, _request: &JobRequest<'_>) -> Result<(), Self::Error> {
        Err(ListingRefused)
    }

    fn list_tables(&self, _at: &DatasetAddress) -> Result<HeldTables, Self::Error> {
        Err(ListingRefused)
    }

    fn listing_was_refused(&self, _error: &Self::Error) -> bool {
        true
    }

    #[cfg(feature = "fixtures")]
    fn apply(&self, _request: &JobRequest<'_>) -> Result<(), Self::Error> {
        Err(ListingRefused)
    }
}

/// A transport that fails, for the one arm that needs the endpoint to say no.
pub(super) struct Broken;

/// The endpoint's own failure, as a transport would report one.
#[derive(Debug, thiserror::Error)]
#[error("the endpoint refused the job")]
pub(super) struct EndpointSaidNo;

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

pub(super) fn source() -> SourceName {
    SourceName::parse("warehouse").expect("a test source is a source")
}

pub(super) fn day(iso: &str) -> Date {
    Date::parse(iso).expect("a test date is a date")
}

/// The posture this adapter is opened with in every test here.
///
/// Shared, and it is the honest declaration rather than a convenience: one service account reaches
/// the dataset for everybody who asks, which is what the capability constant says out loud.
pub(super) fn shared_posture() -> SourcePosture {
    SourcePosture::SharedServiceUser {
        declared: SharedIdentityDeclared::of(
            AcknowledgementReason::parse("one service account reaching the dataset for everybody who asks")
                .expect("a fixture reason is a reason"),
        ),
    }
}

/// A posture whose acknowledgement is a DIFFERENT sentence, for the disagreement case.
pub(super) fn other_posture() -> SourcePosture {
    SourcePosture::SharedServiceUser {
        declared: SharedIdentityDeclared::of(
            AcknowledgementReason::parse("some other operator's reason entirely").expect("a fixture reason is a reason"),
        ),
    }
}

/// The posture the per-subject tests open this adapter with.
pub(super) fn impersonating_posture() -> SourcePosture {
    SourcePosture::ImpersonationAtSource
}

/// A token presented as the asker's own, for the impersonating posture.
pub(super) fn a_subject_token(raw: &str) -> Presented {
    Presented::SubjectToken {
        material: sutura_domain::identity::Secret::new(raw),
    }
}

pub(super) fn leg_of(posture: &SourcePosture) -> Presented {
    match *posture {
        SourcePosture::SharedServiceUser { ref declared } => Presented::SharedServiceUser {
            declared: declared.clone(),
        },
        SourcePosture::ImpersonationAtSource => a_subject_token("an-exchanged-token-for-the-asker"),
    }
}

pub(super) fn open<T>(transport: T, posture: SourcePosture) -> BigQueryWarehouse<T>
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
pub(super) fn plan() -> QueryPlan {
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
        ResultLabel::measure(&MetricName::parse("mrr").expect("a test metric is a metric")),
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
pub(super) type Case = (FieldType, Cell, Value);

/// One column and one row of it, for the value-mapping table.
pub(super) fn one_cell(kind: FieldType, cell: Cell) -> JobRows {
    JobRows::of(vec![Field::of(String::from("value"), kind)], vec![vec![cell]], 1)
}

/// A transport whose failure IS the endpoint declining to return the result at once.
///
/// Its own type rather than a flag on [`Broken`], because what is under test is that the adapter asks
/// the transport rather than guessing: two transports whose errors are indistinguishable to
/// `BigQueryError::Endpoint` and which answer the predicate differently is the only shape that can
/// show the delegation happening.
pub(super) struct Paged;

/// One page of a larger result, as a transport would report it. Same shape as [`EndpointSaidNo`], and
/// that is the point: the adapter cannot tell them apart and does not try.
#[derive(Debug, thiserror::Error)]
#[error("the endpoint answered with one page of a larger result")]
pub(super) struct OnePageOfMore;

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
