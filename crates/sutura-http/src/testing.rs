//! Fakes for the two ports, and a bundle to serve.
//!
//! Compiled only under `cfg(test)`. Fakes rather than a stubbed HTTP layer, which is the whole
//! reason the surface is a port: the router, the refusals, the token gate and the limiter are all
//! exercisable with no catalog directory, no data system and no process to spawn.
//!
//! The bundle here is deliberately the smallest thing that can be *validated*: one model, one
//! metric, one anchor. A bundle with no anchor validates against any warehouse at all, which would
//! make the readiness gate look like it worked when nothing had been executed.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Anchor, Definitions, Description, Dimension, DimensionValue, Metric, Model, Relationship};
use sutura_domain::identity::{
    CredentialBroker, CredentialsDoNotCoverThePlan, Expiry, LegCredentials, Minted, Presented, RequestContext, SourceSet,
};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{
    Aggregate, ColumnName, DimensionName, Grain, JoinType, MetricName, ModelName, RelationshipName, SourceName, TableName,
};
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog};
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::{AcknowledgementReason, ImpersonationCapability, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::{AnchorRows, PreFlight, RowSet, Value, Warehouse};

/// The number the anchor certifies, and the number the answering fake reproduces.
pub(crate) const ANCHORED_VALUE: &str = "197122";

/// The one data system name every fixture here uses.
pub(crate) fn source() -> SourceName {
    SourceName::parse("local").expect("a test source is a source")
}

pub(crate) fn metric_name() -> MetricName {
    MetricName::parse("revenue").expect("a test metric is a metric")
}

fn description(raw: &str) -> Description {
    Description::parse(raw).expect("a test description is a description")
}

fn declared_value(raw: &str) -> DimensionValue {
    DimensionValue::parse(raw).expect("a test value is a value")
}

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a test column is a column")
}

fn june() -> TimeRange {
    TimeRange::new(
        Date::parse("2026-06-01").expect("a test date is a date"),
        Date::parse("2026-07-01").expect("a test date is a date"),
    )
    .expect("June is a range")
}

/// One model, one anchored metric, one filterable dimension.
pub(crate) fn bundle() -> PinnedDefinitions {
    pinned(Some(Anchor::new(june(), String::from(ANCHORED_VALUE))))
}

/// The same bundle with no anchor, so it validates against a data system that answers nothing.
///
/// **Only for tests about what happens AFTER startup.** A bundle with no anchor validates against
/// any warehouse at all - see the module documentation - which is exactly why it is not the default
/// fixture: it makes the readiness gate look like it worked when nothing had been executed. Here it
/// is what lets a service start over a warehouse that then fails every question.
pub(crate) fn unanchored_bundle() -> PinnedDefinitions {
    pinned(None)
}

/// A question the bundle above can answer, for a test that needs to reach the warehouse.
pub(crate) fn a_question() -> sutura_domain::query::Query {
    sutura_domain::query::Query::new(metric_name(), Grain::Month, june(), Vec::new(), Vec::new())
}

fn pinned(anchor: Option<Anchor>) -> PinnedDefinitions {
    let model = Model::new(
        ModelName::parse("orders").expect("a test model is a model"),
        source(),
        TableName::parse("orders").expect("a test table is a table"),
        BTreeSet::from([column("amount_cents"), column("order_date"), column("region")]),
        description("Orders, one row per order."),
    );
    let region = Dimension::new(
        DimensionName::parse("region").expect("a test dimension is a dimension"),
        column("region"),
        None,
        Some(BTreeSet::from([declared_value("north"), declared_value("south")])),
        description("Sales region."),
    );
    let revenue = Metric::new(
        metric_name(),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Day, Grain::Month]),
        BTreeMap::from([(
            DimensionName::parse("region").expect("a test dimension is a dimension"),
            region,
        )]),
        anchor,
        description("Revenue, in minor units."),
    );
    let definitions = Definitions::assemble(vec![model], vec![], vec![revenue]).expect("the test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions,
        Knowledge::none(),
    )
    .expect("the test definitions hash")
}

/// A catalog port that hands back a bundle somebody else built.
pub(crate) struct FixedCatalog {
    bundle: PinnedDefinitions,
}

/// Never returned by [`FixedCatalog`]; the port requires an error type.
#[derive(Debug, thiserror::Error)]
#[error("a fixed catalog cannot fail")]
pub(crate) struct Infallible;

impl SemanticCatalog for FixedCatalog {
    type Error = Infallible;

    /// Everything, and for a pass-through that is the accurate answer rather than the convenient one:
    /// this adapter hands back whatever bundle it was constructed with, so there is no kind it could
    /// not carry. Nothing checks it, because a fake is not registered in the conformance matrix -
    /// `crates/sutura-app/tests/adapters/mod.rs` says why.
    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::everything()
    }

    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        Ok(self.bundle.clone())
    }
}

pub(crate) fn catalog_of(bundle: PinnedDefinitions) -> FixedCatalog {
    FixedCatalog { bundle }
}

/// The audit sink every fixture here starts a service with: the real writer.
///
/// **The real one and not a fake, on purpose.** `LocalService::start` requires a sink, so every
/// fixture has to name one, and naming the shipped writer means the assertions in `crate::harness`
/// that read the log are reading what a deployment would actually get. A fake here would have made
/// those tests pass against a channel nothing ships.
pub(crate) fn sink() -> sutura_runtime::TracingAuditSink {
    sutura_runtime::TracingAuditSink::new()
}

/// Everything one sink was handed, in the order it arrived, as text.
///
/// For the two properties the real writer cannot be asked about: that a record exists **by the time
/// `answer` returns**, and what chain it carried. Both are about the call rather than about the
/// bytes, and reading them back out of a rendered log would be asserting on a rendering.
#[expect(
    clippy::disallowed_types,
    reason = "a synchronous test collector: the guard is taken and dropped inside one statement and no runtime is involved, which is the same argument sutura_runtime::testing::Capture records"
)]
#[derive(Debug, Default)]
pub(crate) struct RecordingSink {
    lines: std::sync::Mutex<Vec<String>>,
}

impl RecordingSink {
    /// What was recorded, oldest first.
    pub(crate) fn lines(&self) -> Vec<String> {
        self.lines.lock().map_or_else(|_poisoned| Vec::new(), |lines| lines.clone())
    }
}

impl sutura_domain::audit::AuditSink for RecordingSink {
    fn record(&self, record: &sutura_domain::audit::CallRecord<'_>) {
        use sutura_domain::audit::RecordedOutcome;
        use sutura_domain::identity::Attribution;

        let chain = record.chain();
        let who = match chain.attribution() {
            Attribution::BareSubject { subject } => format!("subject={}", subject.established()),
            Attribution::ActingFor { subject, actors } => format!("subject={} acting={actors}", subject.established()),
        };
        let named = chain.subject().id().map_or_else(String::new, |id| format!(" id={id}"));
        let how = match *record.outcome() {
            RecordedOutcome::Answered { rows, .. } => format!("answered rows={rows}"),
            RecordedOutcome::Refused { reason } => format!("refused reason={reason:?}"),
        };
        if let Ok(mut lines) = self.lines.lock() {
            lines.push(format!("{who}{named} {how}"));
        }
    }
}

/// The driver's own complaint, one level below the adapter's.
#[derive(Debug, thiserror::Error)]
#[error("no such file: catalog/")]
pub(crate) struct DirectoryMissing;

/// What a catalog adapter returns, with the driver's cause underneath it.
#[derive(Debug, thiserror::Error)]
#[error("the catalog directory could not be read")]
pub(crate) struct CatalogUnreadable {
    #[source]
    cause: DirectoryMissing,
}

/// A catalog port that fails every read, with a cause worth keeping.
pub(crate) struct FailingCatalog;

impl SemanticCatalog for FailingCatalog {
    type Error = CatalogUnreadable;

    /// Nothing, because this adapter never returns a bundle. `nothing()` is a legitimate declaration
    /// rather than a broken one, and it is reachable here only by writing it: the port has no default.
    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::nothing()
    }

    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        Err(CatalogUnreadable { cause: DirectoryMissing })
    }
}

/// The data system's own complaint.
#[derive(Debug, thiserror::Error)]
#[error("connection refused")]
pub(crate) struct ConnectionRefused;

/// What a warehouse adapter returns.
#[derive(Debug, thiserror::Error)]
#[error("the data system rejected the statement")]
pub(crate) struct StatementRejected {
    #[source]
    cause: ConnectionRefused,
}

/// A data system that fails every statement.
pub(crate) struct FailingWarehouse {
    source: SourceName,
    posture: SourcePosture,
}

impl FailingWarehouse {
    /// One, registered - which is what `LocalService::start` takes.
    ///
    /// The posture is `shared-service-user` with a reason this file wrote, and that is honest for a
    /// fake over no data system at all: there is nowhere for a subject to arrive, which is the same
    /// answer the shipped engine gives.
    pub(crate) fn new(source: SourceName) -> sutura_app::Warehouses<Self> {
        sutura_app::Warehouses::of(Self {
            source,
            posture: shared_posture(),
        })
    }
}

impl Warehouse for FailingWarehouse {
    type Error = StatementRejected;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn dry_run(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<PreFlight, Self::Error> {
        Err(StatementRejected {
            cause: ConnectionRefused,
        })
    }

    fn execute(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<RowSet, Self::Error> {
        Err(StatementRejected {
            cause: ConnectionRefused,
        })
    }

    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(StatementRejected {
            cause: ConnectionRefused,
        })
    }
}

/// A data system that answers every statement with one prepared result.
///
/// One result for every plan, which is exactly enough: nothing in the transport layer depends on
/// the number, and the anchor check only needs the metric's own column to carry it.
pub(crate) struct FakeWarehouse {
    source: SourceName,
    posture: SourcePosture,
    result: RowSet,
    held: Arc<AtomicBool>,
}

impl Warehouse for FakeWarehouse {
    type Error = StatementRejected;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn dry_run(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<PreFlight, Self::Error> {
        Ok(PreFlight::NotAsked)
    }

    /// The anchor path, which is deliberately NOT held.
    ///
    /// `Held` is armed only after `start`, and this is the method `start` goes through: a held anchor
    /// would hold startup rather than the request the test is about.
    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(self.result.clone()))
    }

    fn execute(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<RowSet, Self::Error> {
        // Held rather than slept, and that is about the test suite rather than about realism. A
        // `spawn_blocking` task that sleeps keeps running after the assertion, and dropping a
        // `tokio` runtime waits for the blocking pool - so a fixed sleep long enough to outrun the
        // request bound would be added to the wall time of the whole suite. A flag the test clears
        // costs nothing once the assertion is made.
        //
        // The cap is a bug guard, not a timeout: it is what a forgotten `release` costs instead of
        // hanging the suite.
        let deadline = Instant::now() + HELD_AT_MOST;
        while self.held.load(Ordering::Relaxed) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(self.result.clone())
    }
}

/// The longest a held statement is held, whatever the test does.
const HELD_AT_MOST: Duration = Duration::from_secs(20);

/// Whether the fake is currently refusing to return.
///
/// A separate handle because the warehouse itself is moved into the service at `start`, and what a
/// test needs to hold is the switch rather than the adapter. It is armed only AFTER `start`, because
/// `start` re-executes every anchor and a held statement there would hold startup.
pub(crate) struct Held(Arc<AtomicBool>);

impl Held {
    /// From here on, a statement does not come back.
    pub(crate) fn arm(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Let whatever is waiting finish, so the blocking pool drains with the test.
    pub(crate) fn release(&self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

/// The posture every fake in this file is handed.
///
/// One definition, so a test that reads a posture out of an answer's provenance is reading the same
/// value every fixture declares. A fake executes nothing over no data system, so there is nowhere for
/// a subject's credential to arrive - which makes the shared posture the true declaration rather than
/// a convenient one, and the reason says so.
fn shared_posture() -> SourcePosture {
    SourcePosture::SharedServiceUser {
        declared: declared_shared(),
    }
}

/// A credential broker that grants the shared posture for whatever it is asked about.
///
/// **A fake of the broker port, and the honest one for these fixtures.** Every warehouse here is a
/// fake over no data system, so the only shape any of them can be handed is the deployment's own
/// identity for that source - which is what [`shared_posture`] declares and what this grants. A fake
/// that handed out subject material would provoke the adapter's wiring-defect error on every request
/// and prove nothing about the transport.
///
/// **Its own error type, and a review is why it is not a refusal.** The one thing minting can fail on
/// here is `LegCredentials::minted` refusing a set that does not cover the sources it was asked about,
/// and the map is built from those sources - so it is unreachable. It used to be answered as
/// `Minted::Refused`, which is the ONE outcome the transport tests here assert on: a fixture that
/// silently produced it would have made `403 credential_unavailable` pass for the wrong reason. It
/// leaves as the broker's own failure instead, which is a `503` with a different code.
pub(crate) struct GrantsTheSharedIdentity;

/// The fixture broker's own defect, which nothing in this suite can provoke.
#[derive(Debug, thiserror::Error)]
#[error("the fixture broker minted a set that does not cover the plan")]
pub(crate) struct FixtureBrokerDefect {
    #[source]
    cause: CredentialsDoNotCoverThePlan,
}

impl CredentialBroker for GrantsTheSharedIdentity {
    type Error = FixtureBrokerDefect;

    fn mint(&self, context: &RequestContext, sources: &SourceSet) -> Result<Minted, Self::Error> {
        let mut presented = BTreeMap::new();
        for name in sources.iter() {
            drop(presented.insert(
                name.clone(),
                Presented::SharedServiceUser {
                    declared: declared_shared(),
                },
            ));
        }
        // The `Err` arm is unreachable - the map above is built from `sources` - and it leaves as
        // this fixture's OWN failure rather than as `Minted::Refused`. That matters: the refusal is
        // the one outcome the tests here assert on, so a fixture that could fabricate it would let
        // those assertions pass without the code under test ever deciding anything.
        LegCredentials::minted(context.chain().subject().clone(), Expiry::NothingExpires, sources, presented)
            .map(|credentials| Minted::Granted { credentials })
            .map_err(|cause| FixtureBrokerDefect { cause })
    }
}

/// The broker every fixture here starts a service with.
pub(crate) const fn broker() -> GrantsTheSharedIdentity {
    GrantsTheSharedIdentity
}

/// The acknowledgement witness the fakes' posture and the fake broker's legs both carry.
fn declared_shared() -> SharedIdentityDeclared {
    SharedIdentityDeclared::of(
        AcknowledgementReason::parse("a transport-layer fake over no data system, in this process")
            .expect("a fixture reason is a reason"),
    )
}

/// The execution record an answer from these fixtures carries.
///
/// One leg on [`source`], under the posture [`shared_posture`] declares, which is what
/// `sutura_app::answer` would have read off the adapter. Built here rather than inline so a wire test
/// and the fixtures cannot disagree about which posture the fake was handed.
pub(crate) fn ran_shared() -> sutura_domain::source::ExecutedAs {
    sutura_domain::source::ExecutedAs::of(source(), shared_posture())
}

/// A registry holding one warehouse whose answer reproduces the anchor, so the bundle validates.
///
/// **These helpers return a `Warehouses` and not a warehouse**, because that is what
/// `LocalService::start` takes now: a deployment holds as many data systems as its catalog names, and
/// the transport's fixtures hold one.
pub(crate) fn fake_warehouse() -> sutura_app::Warehouses<FakeWarehouse> {
    warehouse_that_can_be_held().0
}

/// The same warehouse, with the switch that makes a statement outrun the request bound.
///
/// For the `408` assertion: the surface bounds the RESPONSE and not the work, so the way to observe
/// a timeout is a port call that has not returned yet.
pub(crate) fn warehouse_that_can_be_held() -> (sutura_app::Warehouses<FakeWarehouse>, Held) {
    let result = RowSet::new(vec![String::from("revenue")], vec![vec![Value::Integer(197_122)]])
        .expect("a one-cell result is a result set");
    // The SAME `Arc` on both sides, which is the whole point of the pair: the switch a test holds
    // and the flag the adapter reads have to be one cell. Building them separately compiles, and
    // then nothing is ever held - caught by the `408` test, which answered `200`.
    let held = Arc::new(AtomicBool::new(false));
    (
        sutura_app::Warehouses::of(FakeWarehouse {
            source: source(),
            posture: shared_posture(),
            result,
            held: Arc::clone(&held),
        }),
        Held(held),
    )
}

/// A warehouse over one prepared result, never held.
///
/// The three fixtures below differ only in the source they claim and the rows they hand back, so
/// they share this rather than each restating the struct. `held` starts cleared, which is the state
/// every one of them wants: holding is for the timeout and admission tests, which use
/// [`warehouse_that_can_be_held`].
fn answering(source: SourceName, result: RowSet) -> sutura_app::Warehouses<FakeWarehouse> {
    sutura_app::Warehouses::of(FakeWarehouse {
        source,
        posture: shared_posture(),
        result,
        held: Arc::new(AtomicBool::new(false)),
    })
}

/// A registry holding one warehouse registered under a name the bundle does not read.
///
/// For `SourceUnavailable`: `sutura_app::answer` looks the plan's source up in the registry and
/// refuses when nothing is registered under it, which is what catches a bundle pointed at a data
/// system this deployment did not configure. A fake registered under some other name is the whole
/// instrument - no real adapter can be asked to lie about its own name, and none should be able to.
///
/// Pair it with [`unanchored_bundle`]: `LocalService::start` re-executes every anchor, and an anchor
/// against this registry would be refused, so an anchored bundle would fail readiness rather than
/// reaching the request path this exists to exercise.
pub(crate) fn warehouse_pretending_to_be(name: &str) -> sutura_app::Warehouses<FakeWarehouse> {
    let result = RowSet::new(vec![String::from("revenue")], vec![vec![Value::Integer(197_122)]])
        .expect("a one-cell result is a result set");
    answering(SourceName::parse(name).expect("a test source is a source"), result)
}

/// A warehouse whose one answer carries one row more than the plan's row cap.
///
/// For `ResultTooLarge`, which is decided AFTER a data system has answered - so unlike every other
/// refusal it cannot be provoked by a question alone, and the honest instrument is a fake that
/// decides its own row count. One row past the cap and not a hundred, because that is the boundary:
/// the plan asks for `max_rows + 1`, so a result of exactly the cap is answerable and one more is
/// the smallest thing that is not.
pub(crate) fn warehouse_that_answers_past_the_row_cap() -> sutura_app::Warehouses<FakeWarehouse> {
    let cap = usize::try_from(sutura_domain::plan::MAX_ROWS).expect("the row cap fits a usize on every target this builds for");
    // The same cell in every row: what is under test is the COUNT, which is the only thing
    // `sutura_app::answer` compares against the cap, and distinct values would suggest otherwise.
    let rows: Vec<Vec<Value>> = vec![vec![Value::Integer(1)]; cap.saturating_add(1)];
    let result = RowSet::new(vec![String::from("revenue")], rows).expect("a one-column result is a result set");
    answering(source(), result)
}

/// A bundle whose two models sit on two data systems, so one question spans both.
///
/// For `PlanSpansTwoSources`. Not reachable from the single-model [`bundle`] above and it cannot be:
/// the refusal needs a catalog whose models are on different data systems, and making that the
/// default fixture would make every other test in this crate span two.
///
/// **Unanchored, deliberately.** An anchor is asked with no dimensions, so it would resolve to the
/// metric's own model and validate - but the point of this fixture is what happens on the request
/// path, and [`unanchored_bundle`] already documents why a bundle with no anchor is only ever for
/// that. `ManyToOne` is the join, because `Definitions::assemble` refuses a dimension reached
/// through a relationship whose declared cardinality may duplicate rows.
pub(crate) fn two_source_bundle() -> PinnedDefinitions {
    let orders = Model::new(
        ModelName::parse("orders").expect("a test model is a model"),
        source(),
        TableName::parse("orders").expect("a test table is a table"),
        BTreeSet::from([column("amount_cents"), column("order_date"), column("customer_id")]),
        description("Orders, one row per order."),
    );
    let customers = Model::new(
        ModelName::parse("customers").expect("a test model is a model"),
        SourceName::parse("elsewhere").expect("a test source is a source"),
        TableName::parse("customers").expect("a test table is a table"),
        BTreeSet::from([column("customer_id"), column("region")]),
        description("Customers, one row per customer."),
    );
    let joined = Relationship::new(
        RelationshipName::parse("order_customer").expect("a test relationship is a relationship"),
        ModelName::parse("orders").expect("a test model is a model"),
        column("customer_id"),
        ModelName::parse("customers").expect("a test model is a model"),
        column("customer_id"),
        JoinType::ManyToOne,
    );
    let region = Dimension::new(
        DimensionName::parse("region").expect("a test dimension is a dimension"),
        column("region"),
        Some(RelationshipName::parse("order_customer").expect("a test relationship is a relationship")),
        None,
        description("Sales region, from the customer."),
    );
    let revenue = Metric::new(
        metric_name(),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Day, Grain::Month]),
        BTreeMap::from([(
            DimensionName::parse("region").expect("a test dimension is a dimension"),
            region,
        )]),
        None,
        description("Revenue, in minor units."),
    );
    let definitions =
        Definitions::assemble(vec![orders, customers], vec![joined], vec![revenue]).expect("the test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions,
        Knowledge::none(),
    )
    .expect("the test definitions hash")
}
