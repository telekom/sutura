//! Fakes for the ports, and a bundle to serve.
//!
//! Compiled only under `cfg(test)`. Fakes rather than a stubbed transport, which is the whole reason
//! the surface is a port: the handler, the tool schema and every refusal are exercisable with no
//! catalog directory, no data system and no process to spawn.
//!
//! The bundle here is deliberately the smallest thing that can be *validated*: one model, one
//! metric, one anchor. A bundle with no anchor validates against any warehouse at all, which would
//! make the readiness gate look like it worked when nothing had been executed - so the tests that
//! answer go through the real `sutura_app::LocalService`, and a service that exists is one whose
//! anchor reproduced the number its author certified.
//!
//! [`FailingSurface`] and [`HoldingSurface`] are the exceptions, and both are fakes of the *driving*
//! port rather than of a driven one. `SurfaceFailure` is what a transport must not confuse with a
//! refusal, and the honest instrument for it is a surface that fails - not a warehouse that has to
//! answer an anchor first and then stop, which would need interior mutability to say something a
//! two-line fake says plainly. [`HoldingSurface`] is there for the same reason one frame up: what the
//! admission bound is about is how many callers are inside `Surface::answer`, so that is where the
//! count belongs.
//!
//! This duplicates `sutura_http`'s own fixtures, and it is the same duplication `crate::wire`
//! explains: `testing` there is `cfg(test)`, so there is nothing to share even if an adapter were
//! allowed to reach into another adapter, which it is not.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use sutura_app::surface::{Surface, SurfaceFailure};
use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Anchor, AnchorValue, Definitions, Description, Dimension, DimensionValue, Metric, Model};
use sutura_domain::identity::{
    CredentialBroker, CredentialsDoNotCoverThePlan, Expiry, LegCredentials, Minted, Presented, RequestContext, SourceSet,
};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, ModelName, SourceName, TableName};
use sutura_domain::pinned::{
    CatalogKind, Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions, SemanticCatalog,
};
use sutura_domain::plan::Executable;
use sutura_domain::query::{Query, ToolOutcome};
use sutura_domain::source::{
    AcknowledgementReason, ImpersonationCapability, SharedIdentityDeclared, SourcePosture, UniformlyExecuted,
};
use sutura_domain::warehouse::{AnchorRows, PreFlight, RowSet, Value, Warehouse};

/// The number the anchor certifies, and the number the answering fake reproduces.
pub(crate) const ANCHORED_VALUE: i64 = 197_122;

pub(crate) fn source() -> SourceName {
    SourceName::parse("local").expect("a test source is a source")
}

fn description(raw: &str) -> Description {
    Description::parse(raw).expect("a test description is a description")
}

fn column(raw: &str) -> ColumnName {
    ColumnName::parse(raw).expect("a test column is a column")
}

pub(crate) fn june() -> TimeRange {
    TimeRange::new(
        Date::parse("2026-06-01").expect("a test date is a date"),
        Date::parse("2026-07-01").expect("a test date is a date"),
    )
    .expect("June is a range")
}

/// One model, one anchored metric, one filterable dimension.
///
/// The two descriptions are DIFFERENT text on purpose: a test asserting that neither reaches a
/// caller would pass on the metric's half alone if one string served both.
pub(crate) fn bundle() -> PinnedDefinitions {
    described_bundle("Revenue, in minor units.", "Sales region.")
}

/// The same bundle with the two descriptions supplied, for walking the shared injection corpus
/// through a real [`Description`] rather than through a wire type built by hand.
pub(crate) fn described_bundle(metric_prose: &str, dimension_prose: &str) -> PinnedDefinitions {
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
        Some(BTreeSet::from([
            DimensionValue::parse("north").expect("a test value is a value"),
            DimensionValue::parse("south").expect("a test value is a value"),
        ])),
        description(dimension_prose),
    );
    let revenue = Metric::new(
        MetricName::parse("revenue").expect("a test metric is a metric"),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Day, Grain::Month]),
        vec![region],
        Some(Anchor::new(
            june(),
            AnchorValue::parse(ANCHORED_VALUE.to_string()).expect("a test anchor value is a value"),
        )),
        description(metric_prose),
    )
    .expect("one dimension cannot duplicate another");
    let definitions = Definitions::assemble(vec![model], vec![], vec![revenue]).expect("the test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions.clone(),
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            // The fixture bundle flows through `LocalService::start_composed`, which holds each
            // contributor to its own declaration: the manifest record has to say what the content
            // actually carries, or the fidelity check refuses a fixture with content "nothing"
            // declared. Deriving it from the content is the honest shape.
            Contribution::of(MetadataCapabilities::produced(&definitions, &Knowledge::none())),
        ),
    )
    .expect("the test definitions hash")
}

/// A catalog port that hands back the bundle above.
pub(crate) struct FixedCatalog;

/// Never returned by [`FixedCatalog`] or [`FakeWarehouse`]; both ports require an error type.
#[derive(Debug, thiserror::Error)]
#[error("a fixture cannot fail")]
pub(crate) struct Unreachable;

impl SemanticCatalog for FixedCatalog {
    type Error = Unreachable;

    /// Golden, matching what `capabilities` below declares: the bundle is Rust literals in this
    /// repository, so there is no kind this fixture could not express.
    const KIND: CatalogKind = CatalogKind::Golden;

    /// Everything, and the reason is the one `sutura-catalog-local` gives for its own `all()`: the
    /// bundle one function above is Rust literals in this repository, so there is no kind this
    /// adapter could not express.
    ///
    /// **Nothing checks it, and that decides which way to be wrong.** A fake is not registered in the
    /// conformance matrix, so no test compares this against what `bundle` actually carries - and of
    /// the two ways to be inaccurate, over-declaring is the safe one: a declared absence that is not
    /// one misleads whoever reads the declaration to decide what to trust, while a declared capability
    /// that this fixture happens not to exercise misleads nobody.
    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::everything()
    }

    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        Ok(bundle())
    }
}

/// A data system that answers every statement with one prepared result.
///
/// One result for every plan, which is exactly enough: nothing in the transport depends on the
/// number, and the anchor check only needs the metric's own column to carry it.
pub(crate) struct FakeWarehouse {
    source: SourceName,
    posture: SourcePosture,
    result: RowSet,
}

impl Warehouse for FakeWarehouse {
    type Error = Unreachable;

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

    fn execute(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<RowSet, Self::Error> {
        Ok(self.result.clone())
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(self.result.clone()))
    }
}

/// A credential broker that grants the shared posture for whatever it is asked about.
///
/// The honest fake for these fixtures, for the reason [`shared_posture`] is the honest posture: a
/// fake over no data system has nowhere for a subject's own credential to arrive. This duplicates
/// `sutura_http`'s fixture of the same shape, and it is the same duplication the module header
/// explains - both `testing` modules are `cfg(test)`, and an adapter may not reach into another.
///
/// **Its own error type rather than `Unreachable`, and a review is why it is not a refusal.** The one
/// thing minting can fail on here is `LegCredentials::minted` refusing a set that does not cover the
/// sources it was asked about, and the map is built from those sources - so it is unreachable. It used
/// to be answered as `Minted::Refused`, which is the ONE outcome the transport tests here assert on: a
/// fixture that silently produced it would have made the `credential_unavailable` sentence pass for
/// the wrong reason.
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
        // The `Err` arm is unreachable: the map above is built from `sources`, so it covers them. It
        // leaves as this fixture's OWN failure rather than as `Minted::Refused`, because the refusal
        // is the one outcome the tests here assert on - a fixture that could fabricate it would let
        // those assertions pass without the code under test deciding anything.
        LegCredentials::minted(context.chain().subject().clone(), Expiry::NothingExpires, sources, presented)
            .map(|credentials| Minted::Granted { credentials })
            .map_err(|cause| FixtureBrokerDefect { cause })
    }
}

/// The broker every fixture here starts a service with.
pub(crate) const fn broker() -> GrantsTheSharedIdentity {
    GrantsTheSharedIdentity
}

/// The acknowledgement witness the fake posture and the fake broker's legs both carry.
fn declared_shared() -> SharedIdentityDeclared {
    SharedIdentityDeclared::of(
        AcknowledgementReason::parse("a transport-layer fake over no data system, in this process")
            .expect("a fixture reason is a reason"),
    )
}

/// The posture every fake here is handed.
///
/// A fake executes nothing over no data system, so there is nowhere for a subject's credential to
/// arrive - which makes the shared posture the true declaration rather than a convenient one.
fn shared_posture() -> SourcePosture {
    SourcePosture::SharedServiceUser {
        declared: declared_shared(),
    }
}

/// The execution record an answer from this fixture carries.
pub(crate) fn ran_shared() -> UniformlyExecuted {
    UniformlyExecuted::of(source(), shared_posture())
}

/// A registry holding one warehouse whose answer reproduces the anchor, so the bundle validates.
///
/// It returns a `Warehouses` because that is what `LocalService::start` takes: a deployment holds as
/// many data systems as its catalog names, and this transport's fixture holds one.
pub(crate) fn fake_warehouse() -> sutura_app::Warehouses<FakeWarehouse> {
    sutura_app::Warehouses::of(FakeWarehouse {
        source: source(),
        posture: shared_posture(),
        result: RowSet::new(vec![String::from("revenue")], vec![vec![Value::Integer(ANCHORED_VALUE)]])
            .expect("a one-cell result is a result set"),
    })
}

/// The data system's own complaint, under the surface failure that carries it.
#[derive(Debug, thiserror::Error)]
#[error("connection refused")]
pub(crate) struct ConnectionRefused;

/// A surface that answers nothing, so a transport's failure path can be asserted.
///
/// It still hands back a real bundle, because a `Surface` is a validated bundle plus a data system
/// and only the second half is broken here.
pub(crate) struct FailingSurface {
    definitions: PinnedDefinitions,
}

impl FailingSurface {
    pub(crate) fn new() -> Self {
        Self { definitions: bundle() }
    }
}

impl Surface for FailingSurface {
    fn definitions(&self) -> &PinnedDefinitions {
        &self.definitions
    }

    fn answer(&self, _context: &sutura_domain::identity::RequestContext, _query: &Query) -> Result<ToolOutcome, SurfaceFailure> {
        Err(SurfaceFailure::Warehouse {
            cause: Box::new(ConnectionRefused),
        })
    }
}

/// A sink that counts, because what this crate's tests need from the audit port is that a call
/// produced a record - not what the record said. `sutura-http` has a `RecordingSink` that renders
/// its content; duplicating that here would be a second renderer to keep in step with no test
/// asserting on it. The record's *content* is covered at the port and at
/// `sutura_runtime::TracingAuditSink`.
#[derive(Default)]
pub(crate) struct CountingSink {
    calls: std::sync::atomic::AtomicUsize,
}

impl CountingSink {
    /// How many records were written.
    pub(crate) fn calls(&self) -> usize {
        self.calls.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl sutura_domain::audit::AuditSink for CountingSink {
    fn record(&self, _record: &sutura_domain::audit::CallRecord<'_>) {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// A surface that answers, and can be made to stay inside [`Surface::answer`] while a test looks.
///
/// **The instrument F7 needs, and a fake of the DRIVING port on purpose.** The finding is about how
/// many questions are inside `Surface::answer` at once, so the count is taken there. A real
/// `LocalService` over a held warehouse would measure the same number one frame further down,
/// through a bundle, a plan and an anchor check the bound is not about - and its anchor runs at
/// startup, which is why `sutura_http`'s equivalent fixture has to arm its switch afterwards.
///
/// It records the PEAK occupancy and not the current one, because the assertion is *nine were asked
/// and N were inside*: a number that is true for an instant and cannot be sampled afterwards.
pub(crate) struct HoldingSurface {
    definitions: PinnedDefinitions,
    /// The answer this fake hands back, prepared at construction.
    ///
    /// Built here and not inside `answer`, because `answer` returns a `Result` and
    /// `clippy::unwrap_in_result` is denied even in a fixture - which is the right way round: a
    /// fallible construction inside the method would be a second failure mode the port's caller
    /// cannot tell from the one under test.
    rows: RowSet,
    occupancy: Arc<Occupancy>,
}

/// The one cell the fake writes and the test reads.
///
/// The SAME `Arc` on both sides, which is the point of the pair [`surface_that_can_be_held`]
/// returns: two cells compile and then nothing is ever held - the failure mode `sutura_http`'s own
/// held fixture records, where the timeout test answered `200`.
#[derive(Default)]
struct Occupancy {
    inside: AtomicUsize,
    peak: AtomicUsize,
    held: AtomicBool,
}

/// The longest a held answer is held, whatever the test does.
///
/// A bug guard rather than a timeout: it is what a forgotten `release` costs instead of hanging the
/// suite.
const HELD_AT_MOST: Duration = Duration::from_secs(20);

impl Surface for HoldingSurface {
    fn definitions(&self) -> &PinnedDefinitions {
        &self.definitions
    }

    fn answer(&self, _context: &RequestContext, _query: &Query) -> Result<ToolOutcome, SurfaceFailure> {
        let inside = self.occupancy.inside.fetch_add(1, Ordering::SeqCst) + 1;
        self.occupancy.peak.fetch_max(inside, Ordering::SeqCst);
        // Held rather than slept, for the reason `sutura_http`'s fixture gives: a `spawn_blocking`
        // task that sleeps keeps running after the assertion, and dropping a `tokio` runtime waits
        // for the blocking pool - so a fixed sleep long enough to outrun an admission window would
        // be added to the wall time of the whole suite.
        let deadline = Instant::now() + HELD_AT_MOST;
        while self.occupancy.held.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.occupancy.inside.fetch_sub(1, Ordering::SeqCst);
        // A real answer, so a test can tell *this call got in and was answered* from *this call was
        // shed at capacity* by the outcome rather than by a sentence.
        Ok(ToolOutcome::Answer {
            provenance: self.definitions.provenance(ran_shared()),
            rows: self.rows.clone(),
        })
    }
}

/// Whether the fake is currently refusing to leave `answer`, and how many callers are inside it.
///
/// A separate handle because the surface itself is moved into the served [`crate::AgentSurface`],
/// and what a test needs to hold is the switch rather than the port.
pub(crate) struct Holding(Arc<Occupancy>);

impl Holding {
    /// From here on, an answer does not come back.
    pub(crate) fn arm(&self) {
        self.0.held.store(true, Ordering::SeqCst);
    }

    /// Let whatever is inside finish, so the blocking pool drains with the test.
    pub(crate) fn release(&self) {
        self.0.held.store(false, Ordering::SeqCst);
    }

    /// How many callers are inside `answer` right now.
    pub(crate) fn inside(&self) -> usize {
        self.0.inside.load(Ordering::SeqCst)
    }

    /// The most that were ever inside `answer` at once.
    pub(crate) fn peak(&self) -> usize {
        self.0.peak.load(Ordering::SeqCst)
    }
}

/// A surface that answers, with the switch that keeps a question inside it.
pub(crate) fn surface_that_can_be_held() -> (HoldingSurface, Holding) {
    let occupancy = Arc::new(Occupancy::default());
    (
        HoldingSurface {
            definitions: bundle(),
            rows: RowSet::new(vec![String::from("revenue")], vec![vec![Value::Integer(ANCHORED_VALUE)]])
                .expect("a one-cell result is a result set"),
            occupancy: Arc::clone(&occupancy),
        },
        Holding(occupancy),
    )
}
