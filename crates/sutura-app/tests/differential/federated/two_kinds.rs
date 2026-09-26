//! [`TwoKinds`]: the differential's third two-source side, and the only one that mixes ADAPTER
//! TYPES rather than two instances of one.
//!
//! Carved out of the parent file by the same file-length gate every other split in this workspace
//! answers to. `#[path]` for `tests/golden.rs`'s reason: a bare `mod two_kinds;` in a submodule of
//! a test target resolves beside the target root, not beside `federated.rs`.
//!
//! Needs a closed enum for the reason `sutura_app::warehouses`'s own header states -
//! `Warehouses<W>` is generic in one `W`. **Not the production enum** -
//! `sutura_cli::serve::kind::AnyWarehouse` mixes the SHIPPED kinds behind `sutura-cli`'s own
//! features, and this crate takes no adapter dependency at all - this is the same SHAPE proved
//! with two dev-only adapters this test target already links. `telekom/sutura#112`'s point:
//! `Warehouse::executes_legs` is what makes asking that per LEG, rather than per registry,
//! possible - both variants declare `EXECUTES_LEGS = true`, and only the instance method can
//! answer that per variant.

use sutura_domain::identity::Presented;
use sutura_domain::model::SourceName;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, ResultBatches, Warehouse};

use super::corpus::{derived, lookup_source};
use super::harness::{Side, duckdb_on, engine_on};
use crate::adapters::source;

pub(super) enum TwoKinds {
    DuckDb(sutura_exec_duckdb::DuckDbWarehouse),
    DataFusion(sutura_exec_datafusion::DataFusionWarehouse),
}

/// [`TwoKinds`]'s error, erased the same way the adapter is.
#[derive(Debug, thiserror::Error)]
pub(super) enum TwoKindsError {
    #[error(transparent)]
    DuckDb(sutura_exec_duckdb::DuckDbError),
    #[error(transparent)]
    DataFusion(sutura_exec_datafusion::DataFusionError),
}

impl Warehouse for TwoKinds {
    type Error = TwoKindsError;

    // Unread off the enum - see the module header, and `AnyWarehouse`'s own doc.
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn source(&self) -> &SourceName {
        match self {
            Self::DuckDb(w) => w.source(),
            Self::DataFusion(w) => w.source(),
        }
    }

    fn posture(&self) -> &SourcePosture {
        match self {
            Self::DuckDb(w) => w.posture(),
            Self::DataFusion(w) => w.posture(),
        }
    }

    fn executes_legs(&self) -> bool {
        match self {
            Self::DuckDb(w) => w.executes_legs(),
            Self::DataFusion(w) => w.executes_legs(),
        }
    }

    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        match self {
            Self::DuckDb(w) => w.execute(executable, presented, deadline).map_err(TwoKindsError::DuckDb),
            Self::DataFusion(w) => w.execute(executable, presented, deadline).map_err(TwoKindsError::DataFusion),
        }
    }

    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        // `two_kinds`'s own call to `sutura_app::verify_and_validate` is the boot-only caller;
        // this enum has no anchor logic of its own, it forwards to whichever adapter it holds.
        #[expect(
            clippy::disallowed_methods,
            reason = "delegates the boot path's one call to the concrete adapter this variant holds"
        )]
        let result = match self {
            Self::DuckDb(w) => w.verify_anchor(plan).map_err(TwoKindsError::DuckDb),
            Self::DataFusion(w) => w.verify_anchor(plan).map_err(TwoKindsError::DataFusion),
        };
        result
    }
}

/// The two-kinds side: the FACT leg on `DuckDB`, the LOOKUP leg on the engine.
pub(super) fn two_kinds(pinned: PinnedDefinitions) -> Side<TwoKinds> {
    let data = &derived().data;
    let warehouses = sutura_app::Warehouses::of(TwoKinds::DuckDb(duckdb_on(data, &source(), &pinned)))
        .and(TwoKinds::DataFusion(engine_on(data, &lookup_source(), &pinned)))
        .expect("two sources, one registry, two kinds");
    Side {
        bundle: sutura_app::verify_and_validate(pinned, &warehouses)
            .expect("the anchors and the declarations hold across two kinds"),
        warehouses,
    }
}

#[cfg(test)]
mod tests {
    use super::TwoKinds;
    use sutura_domain::source::SourcePosture;
    use sutura_domain::warehouse::Warehouse as _;

    fn source(name: &str) -> sutura_domain::model::SourceName {
        sutura_domain::model::SourceName::parse(name).expect("a test source is a source")
    }

    fn shared() -> SourcePosture {
        SourcePosture::SharedServiceUser {
            declared: sutura_domain::source::SharedIdentityDeclared::of(
                sutura_domain::source::AcknowledgementReason::parse("a fake over no data system").expect("a test reason"),
            ),
        }
    }

    /// Both variants declare `EXECUTES_LEGS = true` on their own adapter type, and this is the
    /// mechanism the differential test above depends on: [`TwoKinds::executes_legs`] must read
    /// EACH wrapped instance's own answer rather than one constant for the enum, because the enum
    /// itself can carry no truthful `EXECUTES_LEGS` of its own (see the module header).
    #[test]
    fn each_variant_answers_its_own_wrapped_adapters_capability() {
        let duckdb = TwoKinds::DuckDb(
            sutura_exec_duckdb::DuckDbWarehouse::in_memory(source("a"), shared()).expect("an in-memory database opens"),
        );
        let engine = TwoKinds::DataFusion(
            sutura_exec_datafusion::DataFusionWarehouse::new(
                source("b"),
                shared(),
                sutura_exec_datafusion::WorkingSet::of_bytes(core::num::NonZeroUsize::new(1024 * 1024).expect("nonzero")),
            )
            .expect("an in-process engine starts"),
        );
        assert!(duckdb.executes_legs(), "DuckDbWarehouse declares EXECUTES_LEGS = true");
        assert!(engine.executes_legs(), "DataFusionWarehouse declares EXECUTES_LEGS = true");
    }
}
