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
use sutura_domain::catalog::{
    Anchor, AnchorValue, Definitions, Description, Dimension, DimensionValue, Metric, Model, Relationship,
};
use sutura_domain::identity::{
    CredentialBroker, CredentialsDoNotCoverThePlan, Expiry, LegCredentials, Minted, Presented, RequestContext, SourceSet,
};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{
    Aggregate, ColumnName, DimensionName, Grain, JoinType, MetricName, ModelName, RelationshipName, SourceName, TableName,
};
use sutura_domain::pinned::{
    CatalogKind, Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions, SemanticCatalog,
};
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::{AcknowledgementReason, ImpersonationCapability, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::{AnchorRows, PreFlight, RowSet, Value, Warehouse};

use crate::state::ServiceState;

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

/// The anchor every bundle below is certified with: one range, one parsed value.
fn anchor(value: &str) -> Anchor {
    Anchor::new(june(), AnchorValue::parse(value).expect("a test anchor value is a value"))
}

/// One model, one anchored metric, one filterable dimension.
pub(crate) fn bundle() -> PinnedDefinitions {
    pinned(Some(anchor(ANCHORED_VALUE)))
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

/// The same bundle, with prose a catalog author wrote to be hostile.
///
/// For the corpus tests: `sutura_app::untrusted::PROSE` goes through a REAL
/// [`sutura_domain::catalog::Description`] rather than straight into a wire type, so what the test
/// walks is prose a catalog could actually hold - a description the domain refuses is not an input
/// this surface can ever be handed.
pub(crate) fn described_bundle(prose: &str) -> PinnedDefinitions {
    pinned_described(Some(anchor(ANCHORED_VALUE)), prose, prose)
}

fn pinned(anchor: Option<Anchor>) -> PinnedDefinitions {
    // The two default descriptions are DIFFERENT text, and a test asserting that neither reaches a
    // caller is the reason: one string for both would make the dimension's half of that assertion
    // pass on the metric's.
    pinned_described(anchor, "Revenue, in minor units.", "Sales region.")
}

fn pinned_described(anchor: Option<Anchor>, prose: &str, dimension_prose: &str) -> PinnedDefinitions {
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
        description(dimension_prose),
    );
    let revenue = Metric::new(
        metric_name(),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Day, Grain::Month]),
        vec![region],
        anchor,
        description(prose),
    )
    .expect("one dimension cannot duplicate another");
    let definitions = Definitions::assemble(vec![model], vec![], vec![revenue]).expect("the test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions.clone(),
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(MetadataCapabilities::produced(&definitions, &Knowledge::none())),
        ),
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

    /// Golden, matching what `capabilities` below declares: this pass-through hands back whatever it
    /// was given, so there is no kind it could not carry, and it is reference-shaped in consequence.
    const KIND: CatalogKind = CatalogKind::Golden;

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

    /// Declaring, the class of an adapter that supplies part of the model and says which part.
    const KIND: CatalogKind = CatalogKind::Declaring;

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

/// What a data system says when it will not hand back a result this large.
#[derive(Debug, thiserror::Error)]
#[error("the data system would not return the whole result at once")]
pub(crate) struct WouldNotReturnAtOnce;

/// A data system that will not return the whole result at once.
///
/// **The instrument for the second half of `result_too_large`, reached past a `dry_run` that
/// accepts.** Its `execute` returns `Err` and [`Warehouse::result_did_not_fit`] answers `true`, so
/// the size bound leaves as a `413 result_too_large` rather than the `503` an outage produces. It is
/// deliberately NOT compared to [`FailingWarehouse`] here - that fake fails its `dry_run` first and
/// never reaches `execute`, so the two do not share a path. Its actual control is
/// [`WarehouseThatFailsToExecute`]: the same reach, the same `execute` that returns `Err`, and the
/// DEFAULT `false` predicate. The two are separate types rather than one fake with a flag, because a
/// flag would let one code path pretend to be both a bound and an outage.
///
/// `dry_run` is NOT overridden: it takes the port's `NotAsked` default, so a size bound is a property
/// of the reply and a check that reads no data cannot have hit one. That is also what makes this
/// reach the `execute` branch rather than being refused a step earlier.
pub(crate) struct WarehouseThatWillNotPage {
    source: SourceName,
    posture: SourcePosture,
}

impl WarehouseThatWillNotPage {
    pub(crate) fn new(source: SourceName) -> sutura_app::Warehouses<Self> {
        sutura_app::Warehouses::of(Self {
            source,
            posture: shared_posture(),
        })
    }
}

impl Warehouse for WarehouseThatWillNotPage {
    type Error = WouldNotReturnAtOnce;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn execute(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<RowSet, Self::Error> {
        Err(WouldNotReturnAtOnce)
    }

    /// Does NOT serve the anchor path: a failed `verify_anchor` stops a bundle that carries an anchor
    /// from booting, so the caller must use an unanchored bundle (`unanchored_bundle()`) - which is
    /// what `harness::an_answer_the_data_system_would_not_return_at_once...` does. The comment used
    /// to claim rows, which was false and exactly the kind of claim that misleads a later reuse.
    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(WouldNotReturnAtOnce)
    }

    fn result_did_not_fit(&self, _error: &Self::Error) -> bool {
        true
    }
}

/// A data system that is up but whose `execute` fails, and whose failure is NOT a size bound.
///
/// **The control for [`WarehouseThatWillNotPage`], and they are exactly one predicate apart:** both
/// accept a `dry_run` (via the port's `NotAsked` default), both return `Err` from `execute`, and one
/// answers [`Warehouse::result_did_not_fit`] `true` - leaving as a `413` size bound - while this one
/// takes the default `false` and leaves as the `503` an outage produces. `harness`'s
/// `an_execute_failure_that_is_not_a_size_bound...` asserts that an `execute` failure whose predicate
/// is false really does reach a caller as `503` and not as `413`, which is the cover
/// `WarehouseThatWillNotPage` alone could not provide.
pub(crate) struct WarehouseThatFailsToExecute {
    source: SourceName,
    posture: SourcePosture,
}

impl WarehouseThatFailsToExecute {
    pub(crate) fn new(source: SourceName) -> sutura_app::Warehouses<Self> {
        sutura_app::Warehouses::of(Self {
            source,
            posture: shared_posture(),
        })
    }
}

impl Warehouse for WarehouseThatFailsToExecute {
    type Error = StatementRejected;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
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
/// For the federated question (a `Compiled::Federated` plan). Not reachable from the single-model
/// [`bundle`] above and it cannot be: the plan needs a catalog whose models are on different data
/// systems, and making that the default fixture would make every other test in this crate span two.
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
        vec![region],
        None,
        description("Revenue, in minor units."),
    )
    .expect("one dimension cannot duplicate another");
    let definitions =
        Definitions::assemble(vec![orders, customers], vec![joined], vec![revenue]).expect("the test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions.clone(),
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(MetadataCapabilities::produced(&definitions, &Knowledge::none())),
        ),
    )
    .expect("the test definitions hash")
}

// ------------------------------------------------------ assembling the router ----

/// The real router over a bundle, a data system, a broker and - where a deployment declares one - a
/// gate.
///
/// **Five parameters, and each one is something a test varies.** Four modules had grown their own
/// copy of these two statements, differing only in which of the five they held fixed, so a bundle
/// that stopped validating in one of them would have gone on validating in the others. What none of
/// them may vary is the two statements themselves: the service is started through the real
/// `LocalService::start`, so every anchor is re-executed, and the router is the real one.
///
/// Generic in the adapter AND the broker, because the tests that need this need both: an impersonating
/// fake declares its own error type, and a broker that refuses is a different type from one that
/// grants. `ServiceState` erases the surface behind a `dyn Surface` one line later, so there is
/// nothing downstream for either parameter to reach.
pub(crate) fn serving<W, B>(
    pinned: PinnedDefinitions,
    warehouses: sutura_app::Warehouses<W>,
    broker: B,
    settings: sutura_config::Settings,
    gate: Option<crate::inbound::InboundGate>,
) -> axum::Router
where
    W: Warehouse + Send + Sync + 'static,
    W::Error: Send + Sync,
    B: sutura_domain::identity::CredentialBroker + Send + Sync + 'static,
    B::Error: Send + Sync,
{
    let service = crate::surface::LocalService::start(&catalog_of(pinned), warehouses, sink(), broker, 1 << 30)
        .expect("the test bundle validates");
    let mut state = state_over(Arc::new(service), settings);
    if let Some(gate) = gate {
        state = state.with_inbound_identity(Arc::new(gate));
    }
    crate::router(&state).expect("the test router assembles")
}

/// A request state over these settings, with the execution bound those settings describe.
///
/// **The test side of `telekom/sutura#340`.** `ServiceState::new` takes an `Admission` since that
/// issue, because deriving one inside the state made a second state a second permit set. A test is
/// another composition root, so it does what a root does - reads the bound off the settings it is
/// serving under - and does it in ONE place, because four modules had already grown four copies of
/// the two statements this replaces.
///
/// It takes the settings by value and hands back the state so the bound and the settings the router
/// is assembled from cannot come from two different documents.
pub(crate) fn state_over(surface: Arc<dyn crate::surface::Surface>, settings: sutura_config::Settings) -> ServiceState {
    let admission = sutura_runtime::Admission::from_settings(settings.runtime());
    ServiceState::new(surface, Arc::new(settings), admission)
}

/// The settings a test deployment loads, over one overlay.
pub(crate) fn settings_with(overlay: &str) -> sutura_config::Settings {
    sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(overlay),
    )
    .expect("the test settings load")
}

/// The inbound declaration an overlay carries, for a test that has to build the gate itself.
pub(crate) fn declared_inbound(settings: &sutura_config::Settings) -> sutura_config::InboundIdentity {
    settings
        .security()
        .inbound()
        .expect("this overlay declares an inbound identity")
        .clone()
}

// ---------------------------------------------------- driving the real router ----

/// A well formed question the fakes above answer.
///
/// One literal, because four test modules were each carrying their own copy and a question that
/// stopped being answerable in one of them would have gone on passing in the other three.
pub(crate) const A_QUESTION: &str = r#"{"metric":"revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}"#;

/// A request with a peer address attached.
///
/// **The peer address is the part that cannot be left out.** The limiter keys on the connection's
/// address, which `axum::serve` attaches via `into_make_service_with_connect_info`; a `oneshot` has no
/// connection, so this inserts one. Without it the limiter reports that it cannot extract a key and
/// limits nothing - which is also the failure mode in production if that call is ever dropped from
/// [`crate::server::serve`].
///
/// Here rather than in `crate::harness`, which is where it was, because four test modules had grown
/// four copies of it - and a copy that forgot the peer address would have looked like a passing test
/// of an unlimited surface.
pub(crate) fn request(method: &str, path: &str, token: Option<&str>, body: axum::body::Body) -> axum::extract::Request {
    let mut builder = axum::http::Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let mut request = builder
        .header("content-type", "application/json")
        .body(body)
        .expect("the test request is well formed");
    let peer: std::net::SocketAddr = "203.0.113.7:44444".parse().expect("a test peer address is an address");
    let _previous_peer = request.extensions_mut().insert(axum::extract::ConnectInfo(peer));
    request
}

/// Calls the router once and reads the whole response.
pub(crate) async fn call(app: &axum::Router, request: axum::extract::Request) -> (axum::http::StatusCode, String) {
    let answered = answer(app, request).await;
    (answered.status, answered.body)
}

/// Everything a test asks of one response: the status, the challenge and the body.
///
/// A named struct rather than a three-tuple, because the three modules that need it each wanted a
/// different two of the three and a tuple made every call site restate which.
pub(crate) struct Answered {
    pub(crate) status: axum::http::StatusCode,
    /// The RFC 6750 challenge, where the response carried one.
    pub(crate) challenge: Option<String>,
    pub(crate) body: String,
}

/// Asks [`A_QUESTION`] and reads everything back.
pub(crate) async fn asked(app: &axum::Router, method: &str, path: &str, token: Option<&str>) -> Answered {
    answer(app, request(method, path, token, axum::body::Body::from(A_QUESTION))).await
}

/// One call through the real router.
async fn answer(app: &axum::Router, request: axum::extract::Request) -> Answered {
    use tower::ServiceExt as _;

    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("the router is infallible as a service");
    let status = response.status();
    let challenge = response
        .headers()
        .get(axum::http::header::WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .map(String::from);
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("the test response body is readable");
    Answered {
        status,
        challenge,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

// --------------------------------------------------- the deployment's identity ----

/// What this deployment calls itself. The value an `aud` claim has to equal, byte for byte.
pub(crate) const RESOURCE: &str = "https://sutura.example.com";
/// Who mints tokens for it.
pub(crate) const ISSUER: &str = "https://issuer.example.com";
/// The key id the issuer publishes.
pub(crate) const KID: &str = "the-current-key";

/// A mock authorization server under this deployment's own names, holding one `P-256` key.
///
/// `sutura_dev::issuer` and not a fixture of this crate's own, which is the whole point of that
/// module: leg 1 is verified in the transport, minted-for in a broker and composed in a root, and a
/// fixture inside any one of those three cannot be driven from the other two.
pub(crate) fn an_issuer() -> sutura_dev::issuer::MockIssuer {
    sutura_dev::issuer::MockIssuer::generating(ISSUER, RESOURCE, KID).expect("a mock issuer generates a key pair")
}

/// Every scope this surface has, space-delimited per RFC 6749.
///
/// Read off `sutura_app::Capability` rather than written out, so a new capability widens what a test
/// token grants instead of leaving one route quietly unreachable.
pub(crate) fn every_scope() -> String {
    sutura_app::Capability::every()
        .map(sutura_app::Capability::scope)
        .collect::<Vec<&str>>()
        .join(" ")
}

/// A token this deployment would accept, granting every capability the surface has.
///
/// Every capability, because leg 1 says who is asking and the capability gate says what they may
/// invoke: a token with no `scope` claim reaches a handler for nothing.
pub(crate) fn accepted_by(subject: &str) -> sutura_dev::issuer::Token {
    sutura_dev::issuer::Token::for_subject(subject).granting(&every_scope())
}

/// A `direct` inbound declaration for `issuer`, reading its key set at `key_set_path`.
///
/// **Built from the issuer rather than from constants**, which is a property worth having and not
/// only less repetition: the deployment is configured with the issuer under test's own names, so a
/// test cannot verify against an issuer it did not configure.
pub(crate) fn direct_overlay(issuer: &sutura_dev::issuer::MockIssuer, key_set_path: &str) -> String {
    format!(
        "security:\n  inbound:\n    mode: \"direct\"\n    resource: \"{}\"\n    \
         authorization_server: \"{}\"\n    key_set_file: \"{key_set_path}\"\n    algorithms: [\"ES256\"]\n",
        issuer.audience(),
        issuer.issuer(),
    )
}
