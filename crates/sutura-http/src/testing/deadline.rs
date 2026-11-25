//! Fakes for `docs/adr/0029`'s own cells: a data system whose failure IS the deadline, and one that
//! reports what was left of the deadline a raw statement arrived with.
//!
//! Split out of `testing.rs` for that file's own `max-lines` reason.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Column, ColumnType, Definitions, Description, Model};
use sutura_domain::identity::Presented;
use sutura_domain::knowledge::Knowledge;
use sutura_domain::model::{ColumnName, ModelName, SourceName, TableName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};
use sutura_domain::plan::{
    AnchorPlan, ArmLegs, DrillAcrossCombiner, DrillAcrossPlan, Executable, FederatedAnswerRefusal, FederatedPlan,
    FederationCombiner, Legs, NothingCombined, PolicyRead, StatementAnswer, StatementLegs, StatementQuestion, StatementStopped,
    WorkersShort,
};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::schema::TypeDialect;
use sutura_domain::warehouse::{AnchorRows, RawExecution, RawRows, ResultBatches, RowSet, Value, Warehouse};

use super::{ConnectionRefused, HELD_AT_MOST, StatementRejected, canned, shared_posture, source};

/// What this fake's own stand-in for "the deadline fired" says. Never actually timed - nothing here
/// reads a clock while executing - because what is under test is the MAPPING from
/// [`Warehouse::deadline_exceeded`] answering `true` to `RefusalReason::DeadlineExceeded`, audited,
/// and not a real clock racing a real statement (`docs/adr/0029`'s engine slice).
#[derive(Debug, thiserror::Error)]
#[error("the deadline fired at the data system")]
pub(crate) struct DeadlineFired;

/// A data system whose `execute` fails, reporting the deadline as the cause.
///
/// The sibling `WarehouseThatWillNotPage` is modelled on, for `Warehouse::deadline_exceeded` rather
/// than `Warehouse::result_did_not_fit`: `harness`'s `a_deadline_exceeded_answer_is_422_and_not_a_503`
/// pins that the mapping reaches a caller as `422 deadline_exceeded`, rather than the retryable `503`
/// a dead data system produces.
pub(crate) struct WarehouseThatOutranItsDeadline {
    source: SourceName,
    posture: SourcePosture,
}

impl WarehouseThatOutranItsDeadline {
    pub(crate) fn new(source: SourceName) -> sutura_app::Warehouses<Self> {
        sutura_app::Warehouses::of(Self {
            source,
            posture: shared_posture(),
        })
    }
}

impl Warehouse for WarehouseThatOutranItsDeadline {
    type Error = DeadlineFired;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn execute(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        Err(DeadlineFired)
    }

    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(DeadlineFired)
    }

    fn deadline_exceeded(&self, _error: &Self::Error) -> bool {
        true
    }
}

/// A data system whose raw path holds `"hold me"` while armed and records what was left of the
/// deadline any other statement arrived with - so a statement queued behind the held one shows
/// whether `/v1/sql/run`'s deadline opened before admission or after it.
pub(crate) struct RawDeadlineProbe {
    source: SourceName,
    posture: SourcePosture,
    probed: Arc<Probed>,
}

/// The switch that holds a [`RawDeadlineProbe`], and what it observed.
#[derive(Default)]
pub(crate) struct Probed {
    held: AtomicBool,
    holding: AtomicBool,
    remaining: OnceLock<Duration>,
}

impl Probed {
    /// From here on, `"hold me"` does not come back.
    pub(crate) fn arm(&self) {
        self.held.store(true, Ordering::SeqCst);
    }

    /// Let `"hold me"` finish, so the blocking pool drains with the test.
    pub(crate) fn release(&self) {
        self.held.store(false, Ordering::SeqCst);
    }

    /// Whether `"hold me"` is inside the port.
    pub(crate) fn holding(&self) -> bool {
        self.holding.load(Ordering::SeqCst)
    }

    /// What was left of the first other statement's deadline, zero if it arrived spent; `None` if
    /// none arrived.
    pub(crate) fn remaining(&self) -> Option<Duration> {
        self.remaining.get().copied()
    }
}

impl RawDeadlineProbe {
    /// One probe reporting into `probed` - the same `Arc` the test reads.
    pub(crate) fn new(source: SourceName, probed: &Arc<Probed>) -> sutura_app::Warehouses<Self> {
        sutura_app::Warehouses::of(Self {
            source,
            posture: shared_posture(),
            probed: Arc::clone(probed),
        })
    }
}

impl Warehouse for RawDeadlineProbe {
    type Error = StatementRejected;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const ACCEPTS_RAW_STATEMENTS: bool = true;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn execute(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        Err(StatementRejected {
            cause: ConnectionRefused,
        })
    }

    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(StatementRejected {
            cause: ConnectionRefused,
        })
    }

    fn execute_raw(
        &self,
        statement: &sutura_domain::raw::RawStatement,
        _presented: &Presented,
        deadline: Deadline,
    ) -> RawExecution<Self::Error> {
        if statement.as_str() == "hold me" {
            self.probed.holding.store(true, Ordering::SeqCst);
            let cap = Instant::now() + HELD_AT_MOST;
            while self.probed.held.load(Ordering::SeqCst) && Instant::now() < cap {
                std::thread::sleep(Duration::from_millis(5));
            }
            self.probed.holding.store(false, Ordering::SeqCst);
        } else {
            self.probed
                .remaining
                .get_or_init(|| deadline.remaining_at(Instant::now()).unwrap_or_default());
        }
        Some(Ok(RawRows::of(vec![String::from("n")], vec![vec![Value::Integer(1)]])))
    }
}

/// How much later than its handed deadline [`WarehouseThatOutrunsAPolicyLeg`] answers - past the
/// transport's `enforce_timeout` backstop when it is handed the full request budget, and before it
/// when the leg has already been narrowed by `sutura_app::raw::POLICY_LEG_TEARDOWN_MARGIN`.
const PAST_DEADLINE: Duration = Duration::from_millis(1500);

/// A data system that answers its PLANNED path normally - [`Warehouse::execute`] and
/// [`Warehouse::verify_anchor`] hand back the one prepared result, exactly like `FakeWarehouse` in
/// the parent module - so an anchored bundle validates at startup. Only the RAW path is slow:
/// `execute_raw` sleeps past
/// WHATEVER [`Deadline`] it is actually handed, then fails with the error
/// [`Warehouse::deadline_exceeded`] recognises.
///
/// **Why the planned path must succeed.** This fake used to fail every method, including the two
/// `LocalService::start` calls to certify the bundle's own anchor - so an anchored bundle never
/// finished booting (`NotValidated { cause: AnchorNotExecuted { .. } }`) and no test built on it
/// ever reached the request path it exists to exercise. Nothing here reads a statement's text to
/// decide which leg it is: `execute`/`verify_anchor` are the startup-and-planned surface, and
/// `execute_raw` is the only raw-statement surface - which is also the only path
/// [`ForcesAPolicySearchLeg`] ever drives, so slowing `execute_raw` alone already reaches exactly
/// the policy-column search leg and nothing else.
///
/// Reads the deadline rather than assuming one, so the same fake proves `docs/adr/0029`'s seventh
/// amendment either way: run under [`ForcesAPolicySearchLeg`], this leg's own deadline is narrowed by
/// `sutura_app::raw::POLICY_LEG_TEARDOWN_MARGIN` once it clears `sutura_app::raw::POLICY_LEG_MIN_BUDGET`
/// - and ONLY the narrowed one returns before the transport's generic `408` fires.
pub(crate) struct WarehouseThatOutrunsAPolicyLeg {
    source: SourceName,
    posture: SourcePosture,
    result: RowSet,
    handed: Arc<HandedBudget>,
}

/// What [`WarehouseThatOutrunsAPolicyLeg::execute_raw`] was actually handed - the same `Arc` a test
/// reads, so a failed narrowing reads as the unnarrowed budget it recorded rather than only as
/// which side of `enforce_timeout`'s backstop the response landed on.
#[derive(Default)]
pub(crate) struct HandedBudget(OnceLock<Duration>);

impl HandedBudget {
    /// The remaining budget `execute_raw` was handed, `None` if it never ran.
    pub(crate) fn remaining(&self) -> Option<Duration> {
        self.0.get().copied()
    }
}

impl WarehouseThatOutrunsAPolicyLeg {
    /// One warehouse reporting into `handed` - the same `Arc` the test reads.
    pub(crate) fn new(source: SourceName, handed: &Arc<HandedBudget>) -> sutura_app::Warehouses<Self> {
        let result = RowSet::new(vec![String::from("revenue")], vec![vec![Value::Integer(197_122)]])
            .expect("a one-cell result is a result set");
        sutura_app::Warehouses::of(Self {
            source,
            posture: shared_posture(),
            result,
            handed: Arc::clone(handed),
        })
    }
}

/// A one-model, one-column, no-metric bundle for [`WarehouseThatOutrunsAPolicyLeg`] - `testing.rs`'s
/// own [`super::bundle`] cannot stand in: its "orders" model declares every column as a bare
/// [`ColumnName`], which [`From<ColumnName>`](Column) turns into a [`Column`] with `data_type: None`,
/// which never maps to an Arrow type, which `sutura_domain::warehouse::schema::PinnedSchemas` records
/// in `fell_back` for every such column - and `sutura_app::raw::tables` drops a model with ANY
/// fallen-back column from `StatementQuestion::tables` entirely. [`ForcesAPolicySearchLeg`] reads
/// `question.tables().first()` to pick the one source to force through the policy-leg path; against
/// `bundle()` that call is `None` before it ever runs, and the statement falls through to the
/// unnarrowed pass-through instead - the bug this fixture exists to not reproduce. One column with an
/// explicit `"text"` type (mapped by every [`TypeDialect`]) keeps the model in `tables()`.
pub(crate) fn policy_leg_bundle() -> PinnedDefinitions {
    let model = Model::new(
        ModelName::parse("orders").expect("a test model name is a model name"),
        source(),
        TableName::parse("orders").expect("a test table name is a table name"),
        [Column::new(
            ColumnName::parse("region").expect("a test column name is a column name"),
            Some(ColumnType::parse("text").expect("\"text\" is a column type")),
            Description::default(),
            None,
        )],
        Description::default(),
    );
    let definitions = Definitions::assemble(vec![model], vec![], vec![]).expect("one typed model is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-policy-leg-1").expect("a test version is a version"),
        definitions.clone(),
        Knowledge::none(),
        ContributionManifest::single(
            source(),
            Contribution::of(MetadataCapabilities::produced(&definitions, &Knowledge::none())),
        ),
    )
    .expect("the test definitions hash")
}

impl Warehouse for WarehouseThatOutrunsAPolicyLeg {
    type Error = DeadlineFired;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const ACCEPTS_RAW_STATEMENTS: bool = true;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn execute(
        &self,
        _executable: Executable<'_>,
        _presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        Ok(canned(&self.result))
    }

    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(self.result.clone()))
    }

    fn execute_raw(
        &self,
        _statement: &sutura_domain::raw::RawStatement,
        _presented: &Presented,
        deadline: Deadline,
    ) -> RawExecution<Self::Error> {
        let remaining = deadline.remaining_at(Instant::now()).unwrap_or_default();
        self.handed.0.get_or_init(|| remaining);
        std::thread::sleep(remaining + PAST_DEADLINE);
        Some(Err(DeadlineFired))
    }

    fn deadline_exceeded(&self, _error: &Self::Error) -> bool {
        true
    }

    fn type_dialect(&self) -> Option<TypeDialect> {
        // Any dialect does: `ForcesAPolicySearchLeg` discards the resolved table set and runs the
        // caller's own statement text unchanged. What matters is `Some`, so `sutura_app::raw::tables`
        // keeps this source's models - `None`, the default, would drop every one and leave
        // `StatementQuestion::tables` empty.
        Some(TypeDialect::DataFusion)
    }
}

/// A federation combiner that forces the lone table a statement names through
/// [`StatementLegs::run_planned`] as a policy-column read applied at its source - standing in for
/// `text_prefilter::discover` recognising a `LIKE` shape over a declared policy column, which this
/// harness does not need a real statement for.
///
/// Every other method is [`sutura_domain::plan::RefusingCombiner`]'s own answer: nothing here joins
/// a federated or a drill-across question, because no test built on this fake asks one.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ForcesAPolicySearchLeg;

impl FederationCombiner for ForcesAPolicySearchLeg {
    type Error = NothingCombined;

    fn combine(&self, _plan: &FederatedPlan, _legs: Legs<'_>, _working_set_bytes: u64) -> Result<ResultBatches, Self::Error> {
        Err(NothingCombined)
    }

    fn working_set_exhausted(&self, _error: &Self::Error) -> Option<u64> {
        None
    }

    fn answer_not_well_formed(&self, _error: &Self::Error) -> Option<FederatedAnswerRefusal> {
        None
    }

    fn workers_not_ready(&self, _error: &Self::Error) -> Option<WorkersShort> {
        None
    }

    fn node_lost(&self, _error: &Self::Error) -> bool {
        false
    }

    fn answer_statement<L>(&self, question: StatementQuestion<'_>, legs: &mut L) -> StatementAnswer<L::Stop>
    where
        L: StatementLegs,
    {
        let source = question.tables().first()?.source().clone();
        let sources = BTreeSet::from([source.clone()]);
        Some(
            legs.open(&sources)
                .and_then(|()| legs.run_planned(&source, question.statement(), &BTreeSet::new(), PolicyRead::AtSource, None))
                .map_err(StatementStopped::Leg),
        )
    }
}

impl DrillAcrossCombiner for ForcesAPolicySearchLeg {
    fn merge(
        &self,
        _plan: &DrillAcrossPlan,
        _arms: &[ArmLegs<'_>],
        _working_set_bytes: u64,
    ) -> Result<ResultBatches, Self::Error> {
        Err(NothingCombined)
    }
}

/// [`super::serving`], generic in the combiner too - for the one suite that needs a combiner
/// driving `/v1/sql/run`'s `StatementLegs::run_planned` path rather than `serving`'s own
/// `RefusingCombiner`, which [`FederationCombiner::answer_statement`]'s default routes every raw
/// statement around. Lives beside [`ForcesAPolicySearchLeg`] rather than in `testing.rs` itself,
/// which is already at that file's own `max-lines` cap.
pub(crate) fn serving_with_combiner<W, B, C>(
    pinned: sutura_domain::pinned::PinnedDefinitions,
    warehouses: sutura_app::Warehouses<W>,
    broker: B,
    settings: sutura_config::Settings,
    gate: Option<crate::inbound::InboundGate>,
    combiner: C,
) -> axum::Router
where
    W: Warehouse + Send + Sync + 'static,
    W::Error: Send + Sync,
    B: sutura_domain::identity::CredentialBroker + Send + Sync + 'static,
    B::Error: Send + Sync,
    C: DrillAcrossCombiner + Send + Sync + 'static,
    C::Error: Send + Sync,
{
    let service = crate::surface::LocalService::start(
        &super::catalog_of(pinned),
        warehouses,
        super::sink(),
        broker,
        combiner,
        1 << 30,
    )
    .expect("the test bundle validates");
    let mut state = super::state_over(Arc::new(service), settings);
    if let Some(gate) = gate {
        state = state.with_inbound_identity(Arc::new(gate));
    }
    crate::router(&state).expect("the test router assembles")
}
