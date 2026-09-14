//! Opt-in measurement construction for bounded DataFusion execution.
//!
//! [`MeasuredWarehouse`](crate::measurement::MeasuredWarehouse) changes no ordinary construction
//! path. It keeps a bounded recording pool beside a fresh adapter so a child can read only the engine
//! operators' reservation peak. It does not measure driver buffering, collected batches, domain-row
//! conversion, or the process resident set.

use std::path::Path;
use std::sync::Arc;

use datafusion::prelude::SessionConfig;
use sutura_domain::identity::Presented;
use sutura_domain::model::{SourceName, TableName};
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::cardinality::{DeclaredKey, KeyUniqueness};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, PreFlight, RowSet, Warehouse};

use crate::{DataFusionError, DataFusionWarehouse, WorkingSet};
use datafusion::execution::memory_pool::{MemoryPool as _, PeakRecordingPool};

/// A measured warehouse and its persistent operator-reservation observer.
///
/// It delegates the execution port unchanged. Ordinary [`DataFusionWarehouse`] construction retains
/// its direct [`GreedyMemoryPool`](datafusion::execution::memory_pool::GreedyMemoryPool), so recording
/// costs nothing outside an explicit measurement child.
pub struct MeasuredWarehouse {
    warehouse: DataFusionWarehouse,
    pool: Arc<PeakRecordingPool>,
}

impl MeasuredWarehouse {
    pub fn new(source: SourceName, posture: SourcePosture, working_set: WorkingSet) -> Result<Self, DataFusionError> {
        let (environment, pool) = crate::pool::recording_environment(working_set)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|cause| DataFusionError::Runtime { cause })?;
        let warehouse =
            DataFusionWarehouse::from_bounded(source, posture, working_set, SessionConfig::new(), environment, runtime);
        Ok(Self { warehouse, pool })
    }

    /// The persistent operator reservation peak in bytes.
    #[must_use]
    pub fn pool_peak(&self) -> usize {
        self.pool.peak_reserved()
    }

    /// The reservation at the end of the observation window.
    #[must_use]
    pub fn pool_reserved(&self) -> usize {
        self.pool.reserved()
    }

    /// Starts a new pool observation window.
    pub fn reset_peak(&self) {
        self.pool.reset_peak();
    }

    /// Attaches one CSV table to the measured child.
    pub fn attach_csv(&self, table: &TableName, path: &Path) -> Result<(), DataFusionError> {
        self.warehouse.attach_csv(table, path)
    }
}

impl Warehouse for MeasuredWarehouse {
    type Error = DataFusionError;

    const IMPERSONATION: ImpersonationCapability = <DataFusionWarehouse as Warehouse>::IMPERSONATION;
    const EXECUTES_LEGS: bool = <DataFusionWarehouse as Warehouse>::EXECUTES_LEGS;

    fn source(&self) -> &SourceName {
        self.warehouse.source()
    }

    fn posture(&self) -> &SourcePosture {
        self.warehouse.posture()
    }

    fn dry_run(&self, executable: Executable<'_>, presented: &Presented, deadline: Deadline) -> Result<PreFlight, Self::Error> {
        self.warehouse.dry_run(executable, presented, deadline)
    }

    fn execute(&self, executable: Executable<'_>, presented: &Presented, deadline: Deadline) -> Result<RowSet, Self::Error> {
        self.warehouse.execute(executable, presented, deadline)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "measurement wrapper delegates the boot-only anchor check"
    )]
    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        self.warehouse.verify_anchor(plan)
    }

    fn working_set_exhausted(&self, error: &Self::Error) -> Option<u64> {
        self.warehouse.working_set_exhausted(error)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "measurement wrapper delegates the boot-only declared-key check"
    )]
    fn declared_key(&self, key: DeclaredKey<'_>) -> Result<KeyUniqueness, Self::Error> {
        self.warehouse.declared_key(key)
    }
}
