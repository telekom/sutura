//! #2106's review: `answer_sql` must never let a statement resolved only by naming its source
//! explicitly fall through to the single-source pass-through - that text is the caller's own,
//! never admission-checked, and the one listed source has no notion of a `source.schema.table`
//! qualifier being a sutura concept. Declared here rather than in `tests.rs`, so the base run,
//! which restores this file, does not compile it - it names `StatementStopped::SourcePrefixRefused`,
//! the variant this fix adds.

use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Audience, Column, ColumnType, Definitions, Description, Model};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::model::{ColumnName, ModelName, TableName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};
use sutura_domain::plan::{
    FederatedAnswerRefusal, FederatedPlan, FederationCombiner, Legs, NothingCombined, StatementAnswer, StatementLegs,
    StatementQuestion, StatementStopped, WorkersShort,
};
use sutura_domain::raw::{RawOutcome, RawRefusalReason, RawStatement};
use sutura_domain::warehouse::ResultBatches;

use super::{RunSqlScope, answer_sql};
use crate::tests::{asked_by_a_person, shared, source, test_deadline};
use crate::tests_support::{CountingBroker, RawCapableWarehouse, run_sql_sources};
use crate::{Warehouses, verify_and_validate};

/// A combiner that always answers the shape this review is about: a reference resolved only by
/// naming its source, which must refuse rather than let the tool fall back to running the
/// caller's own text against the single source.
#[derive(Debug, Clone, Copy, Default)]
struct RefusingPrefixedCombiner;

impl FederationCombiner for RefusingPrefixedCombiner {
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

    fn answer_statement<L>(&self, _question: StatementQuestion<'_>, _legs: &mut L) -> StatementAnswer<L::Stop>
    where
        L: StatementLegs,
    {
        Some(Err(StatementStopped::SourcePrefixRefused))
    }
}

fn bundle() -> PinnedDefinitions {
    let model = |name: &str| {
        Model::new(
            ModelName::parse(name).expect("a fixture model"),
            source(),
            TableName::parse(name).expect("a fixture table"),
            [Column::new(
                ColumnName::parse("id").expect("a fixture column"),
                Some(ColumnType::parse("BIGINT").expect("a fixture type")),
                Description::default(),
                Some(false),
            )],
            Description::default(),
        )
    };
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a fixture version"),
        Definitions::assemble(vec![model("visible_table").with_audience(Audience::Open)], vec![], vec![])
            .expect("the fixture is consistent"),
        Knowledge::none(),
        ContributionManifest::single(source(), Contribution::of(MetadataCapabilities::nothing())),
    )
    .expect("the fixture pins")
}

/// The resolver reporting `SourcePrefixRefused` (a source-prefixed reference that still did not
/// plan) must stop `answer_sql` there - never fall through to `pass_through`'s single-source
/// retry, which would send the caller's own `source.schema.table` text verbatim.
#[test]
fn a_statement_resolved_only_by_naming_its_source_refuses_without_falling_through_to_the_single_source() {
    let warehouses = Warehouses::of(RawCapableWarehouse::answering_rows(source(), shared(), 1));
    let pinned = verify_and_validate(bundle(), &warehouses).expect("the declared schemas validate");
    let broker = CountingBroker::default();
    let statement = RawStatement::parse("select id from somewhere.visible_table").expect("a bounded query");
    let scope = RunSqlScope::new(run_sql_sources(vec![source()]), true);
    let answer = answer_sql(
        &pinned,
        &asked_by_a_person(),
        &statement,
        &broker,
        &warehouses,
        &RefusingPrefixedCombiner,
        &scope,
        1 << 30,
        test_deadline(),
    )
    .expect("a refusal is an answer");
    let RawOutcome::Refusal { reason } = answer.into_outcome() else {
        panic!("the query must be refused");
    };
    assert_eq!(reason, RawRefusalReason::StatementFailed);
    assert_eq!(
        broker.asked(),
        0,
        "a source-prefixed reference must never fall through to the pass-through mint"
    );
}
