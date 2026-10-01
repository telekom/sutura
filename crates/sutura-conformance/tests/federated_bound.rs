#![forbid(unsafe_code)]
#![expect(
    clippy::expect_used,
    reason = "every value below is a literal or a known-good fixture, so a failure is a broken test rather than an input to handle"
)]
//! The two-warehouse arm, bound over two in-process `DataFusion` engines on two sources and the
//! `DataFusion` combiner above them.
//!
//! **What this establishes:** the federated pack executes one leg on each warehouse, combines them,
//! and lands on the rows each federated `.case` file states. **What it does not:** a second KIND of
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

/// Each stage of a federated answer that does not answer is named as that stage: a fake fails
/// exactly one of the fact leg, the lookup leg or the combine, and every other stage is real.
#[cfg(test)]
mod stages {
    use super::{combiner, corpus, fact, federated_content_agrees, lookup};
    use sutura_conformance::Fault;
    use sutura_conformance::execute::Stage;
    use sutura_domain::identity::Presented;
    use sutura_domain::model::SourceName;
    use sutura_domain::plan::{AnchorPlan, Executable, FederatedAnswerRefusal, FederatedPlan, FederationCombiner, Legs};
    use sutura_domain::source::{ImpersonationCapability, SourcePosture};
    use sutura_domain::warehouse::deadline::Deadline;
    use sutura_domain::warehouse::{AnchorRows, ResultBatches, Warehouse};

    #[derive(Debug, thiserror::Error)]
    #[error("the fake did not answer")]
    struct NotAnswered;

    struct FailingWarehouse {
        source: SourceName,
        posture: SourcePosture,
    }

    impl FailingWarehouse {
        fn as_source(source: SourceName) -> Self {
            Self {
                source,
                posture: corpus::posture(),
            }
        }
    }

    impl Warehouse for FailingWarehouse {
        type Error = NotAnswered;

        const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
        const EXECUTES_LEGS: bool = true;

        fn source(&self) -> &SourceName {
            &self.source
        }

        fn posture(&self) -> &SourcePosture {
            &self.posture
        }

        fn execute(&self, _: Executable<'_>, _: &Presented, _: Deadline) -> Result<ResultBatches, Self::Error> {
            Err(NotAnswered)
        }

        fn verify_anchor(&self, _: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
            Err(NotAnswered)
        }
    }

    struct FailingCombiner;

    impl FederationCombiner for FailingCombiner {
        type Error = NotAnswered;

        fn combine(&self, _: &FederatedPlan, _: Legs<'_>, _: u64) -> Result<ResultBatches, Self::Error> {
            Err(NotAnswered)
        }

        fn working_set_exhausted(&self, _: &Self::Error) -> Option<u64> {
            None
        }

        fn answer_not_well_formed(&self, _: &Self::Error) -> Option<FederatedAnswerRefusal> {
            None
        }
    }

    #[test]
    fn a_fact_leg_that_does_not_answer_faults_on_the_fact_stage() {
        let fault = federated_content_agrees(&(FailingWarehouse::as_source(corpus::source()), lookup(), combiner()))
            .expect_err("a failing fact leg faults");
        assert!(
            matches!(
                fault,
                Fault::NotAnswered {
                    cause: Stage::Fact(_),
                    ..
                }
            ),
            "{fault:?}"
        );
    }

    #[test]
    fn a_lookup_leg_that_does_not_answer_faults_on_the_lookup_stage() {
        let fault = federated_content_agrees(&(fact(), FailingWarehouse::as_source(corpus::lookup_source()), combiner()))
            .expect_err("a failing lookup leg faults");
        assert!(
            matches!(
                fault,
                Fault::NotAnswered {
                    cause: Stage::Lookup(_),
                    ..
                }
            ),
            "{fault:?}"
        );
    }

    #[test]
    fn a_combiner_that_does_not_answer_faults_on_the_combine_stage() {
        let fault = federated_content_agrees(&(fact(), lookup(), FailingCombiner)).expect_err("a failing combine faults");
        assert!(
            matches!(
                fault,
                Fault::NotAnswered {
                    cause: Stage::Combine(_),
                    ..
                }
            ),
            "{fault:?}"
        );
    }
}
