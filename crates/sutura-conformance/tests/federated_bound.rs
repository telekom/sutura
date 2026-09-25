#![forbid(unsafe_code)]
#![expect(
    clippy::expect_used,
    reason = "every value below is a literal or a known-good fixture, so a failure is a broken test rather than an input to handle"
)]
//! The two-warehouse arm, bound over two in-process `DataFusion` engines on two sources and the
//! `DataFusion` combiner above them.
//!
//! **What this establishes:** the federated pack executes one leg on each warehouse, combines them,
//! and lands on `corpus::federated_cases`' hand-built rows. **What it does not:** a second KIND of
//! warehouse (both legs are one engine), order, or anything about identity - every leg runs under
//! the corpus's one shared posture.

use sutura_conformance::execute::federated_content_agrees;
use sutura_conformance::{Fixture, Outcome, corpus};
use sutura_domain::model::{SourceName, TableName};
use sutura_exec_datafusion::{DataFusionCombiner, DataFusionWarehouse, WorkingSet};

/// One engine as `source`, with `table` attached from `csv` - a gibibyte ceiling, as the
/// single-warehouse binding writes it.
fn engine(source: SourceName, table: &TableName, csv: &std::path::Path) -> DataFusionWarehouse {
    let ceiling = core::num::NonZeroUsize::new(1024 * 1024 * 1024).expect("a gibibyte is positive");
    let engine =
        DataFusionWarehouse::new(source, corpus::posture(), WorkingSet::of_bytes(ceiling)).expect("an in-process engine starts");
    engine.attach_csv(table, csv).expect("the engine attaches its table");
    engine
}

fn fact() -> DataFusionWarehouse {
    engine(corpus::source(), &corpus::table(), &corpus::on_disk())
}

fn lookup() -> DataFusionWarehouse {
    engine(corpus::lookup_source(), &corpus::lookup_table(), &corpus::lookup_on_disk())
}

fn combiner() -> DataFusionCombiner {
    DataFusionCombiner::new().expect("an in-process combiner starts")
}

fn open_fact() -> Fixture<DataFusionWarehouse> {
    Fixture::standing(fact())
}

fn open_lookup() -> Fixture<DataFusionWarehouse> {
    Fixture::standing(lookup())
}

sutura_conformance::execute_packs! {
    adapter: datafusion_federated,
    fact_warehouse: sutura_exec_datafusion::DataFusionWarehouse,
    lookup_warehouse: sutura_exec_datafusion::DataFusionWarehouse,
    open_fact: crate::open_fact,
    open_lookup: crate::open_lookup,
    combiner: crate::combiner,
}

#[cfg(test)]
mod pack {
    use super::{Outcome, combiner, fact, federated_content_agrees, lookup};

    /// The same pack as the emitted cell above, asserted HERE: a macro-emitted cell panics at its
    /// invocation line, which `just causality`'s claim arm cannot attribute to a test fn.
    #[test]
    fn two_in_process_legs_combine_to_the_reference_rows() {
        let outcome = federated_content_agrees(&(fact(), lookup(), combiner()));
        assert!(matches!(outcome, Ok(Outcome::Held)), "{outcome:?}");
    }
}
