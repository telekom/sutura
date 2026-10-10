//! The two-source fixture the federated capability gate needs, split out of `testing.rs` by that
//! file's own `max-lines` reason - it sits at the cap on its own.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::plan::{
    AnchorPlan, Executable, FederatedAnswerRefusal, FederatedPlan, FederationCombiner, Legs, NothingCombined,
};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::arrow::of_rows;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, PreFlight, ResultBatches, RowSet, Value, Warehouse};

use super::FakeWarehouse;

/// Both of `two_source_bundle`'s sources open, each a [`FakeWarehouse`] taking the port's
/// defaulted `executes_legs` - unlike `fake_warehouse` alone: the per-leg gate looks a source up
/// before asking its capability, so one source open would refuse `SourceUnavailable` for the
/// other before either leg's capability is asked.
pub(crate) fn two_source_fake_warehouse() -> sutura_app::Warehouses<FakeWarehouse> {
    let result = RowSet::new(vec![String::from("revenue")], vec![vec![Value::Integer(197_122)]])
        .expect("a one-cell result is a result set");
    let local = FakeWarehouse {
        source: super::source(),
        posture: super::shared_posture(),
        result: result.clone(),
        held: Arc::new(AtomicBool::new(false)),
    };
    let elsewhere = FakeWarehouse {
        source: SourceName::parse("elsewhere").expect("a test source is a source"),
        posture: super::shared_posture(),
        result,
        held: Arc::new(AtomicBool::new(false)),
    };
    sutura_app::Warehouses::of(local)
        .and(elsewhere)
        .expect("two distinct sources, one registry")
}

/// A [`FakeWarehouse`] that runs a federated leg, which is the one thing the fake itself declines.
///
/// A wrapper rather than a flag on the fake: `EXECUTES_LEGS` is an associated const of the port, so
/// a fixture that answers `true` has to be its own type.
pub(crate) struct LegExecutingWarehouse(FakeWarehouse);

impl Warehouse for LegExecutingWarehouse {
    type Error = <FakeWarehouse as Warehouse>::Error;

    const IMPERSONATION: ImpersonationCapability = FakeWarehouse::IMPERSONATION;
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &SourceName {
        self.0.source()
    }

    fn posture(&self) -> &SourcePosture {
        self.0.posture()
    }

    fn dry_run(&self, executable: Executable<'_>, presented: &Presented, deadline: Deadline) -> Result<PreFlight, Self::Error> {
        self.0.dry_run(executable, presented, deadline)
    }

    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        self.0.execute(executable, presented, deadline)
    }

    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(self.0.result.clone()))
    }
}

/// [`two_source_fake_warehouse`]'s pair, each able to run a leg.
pub(crate) fn two_source_leg_warehouses() -> sutura_app::Warehouses<LegExecutingWarehouse> {
    two_source_fake_warehouse().into_mapped(LegExecutingWarehouse)
}

/// A combiner whose combined answer is `rows` rows of one column, whatever the legs returned.
///
/// A fake over the port, never a mocked engine: it is the one that lets a transport test reach the
/// federated bounds `RefusingCombiner` is built to keep it away from.
pub(crate) struct CombinesTo(pub(crate) usize);

impl FederationCombiner for CombinesTo {
    type Error = NothingCombined;

    fn combine(&self, _plan: &FederatedPlan, _legs: Legs<'_>, _working_set_bytes: u64) -> Result<ResultBatches, Self::Error> {
        let columns = [String::from("revenue")];
        // A one-column answer is always well formed, so `NothingCombined` is the arm nothing reaches.
        of_rows(&columns, &vec![vec![Value::Integer(1)]; self.0]).map_err(|_malformed| NothingCombined)
    }

    fn working_set_exhausted(&self, _error: &Self::Error) -> Option<u64> {
        None
    }

    fn answer_not_well_formed(&self, _error: &Self::Error) -> Option<FederatedAnswerRefusal> {
        None
    }
}
