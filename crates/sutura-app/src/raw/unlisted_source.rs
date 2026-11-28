//! #2106's review: a source this caller's scope does not list must stay unreachable even when a
//! statement names it explicitly - `tables()`'s pre-existing scope filter, not the resolver, is
//! what has to hold this, and the resolver gaining a source-prefix reading must not change it.
//! Declared here rather than in `tests.rs`, so the base run, which restores this file, does not
//! compile it.

use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Audience, Column, ColumnType, Definitions, Description, Model};
use sutura_domain::identity::Presented;
use sutura_domain::knowledge::Knowledge;
use sutura_domain::model::{ColumnName, ModelName, SourceName, TableName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::raw::{RawOutcome, RawRefusalReason, RawStatement};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::schema::TypeDialect;
use sutura_domain::warehouse::{AnchorRows, PreFlight, RawExecution, ResultBatches, Warehouse};
use sutura_exec_datafusion::DataFusionCombiner;

use super::{RunSqlScope, answer_sql};
use crate::tests::{asked_by_a_person, shared, source, test_deadline};
use crate::tests_support::{CountingBroker, RawCapableWarehouse, run_sql_sources};
use crate::{Warehouses, verify_and_validate};

/// A dialect-bearing wrapper around [`RawCapableWarehouse`], mirroring `raw::planned_refusals`'s own
/// `Physical`. `RawCapableWarehouse::type_dialect` defaults to `None`, and `raw::tables` drops every
/// model at a source whose dialect is `None` - without this wrapper the whole bundle below would be
/// invisible to the real planner regardless of scope, and the refusal this test pins would hold for
/// the wrong reason (nothing to resolve) rather than the one it claims (the scope filter).
struct Physical(RawCapableWarehouse);

impl Warehouse for Physical {
    type Error = <RawCapableWarehouse as Warehouse>::Error;
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const ACCEPTS_RAW_STATEMENTS: bool = true;

    fn source(&self) -> &SourceName {
        self.0.source()
    }
    fn posture(&self) -> &SourcePosture {
        self.0.posture()
    }
    fn type_dialect(&self) -> Option<TypeDialect> {
        Some(TypeDialect::DuckDb)
    }
    fn dry_run(&self, plan: Executable<'_>, presented: &Presented, deadline: Deadline) -> Result<PreFlight, Self::Error> {
        self.0.dry_run(plan, presented, deadline)
    }
    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        self.0.execute(executable, presented, deadline)
    }
    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        #[expect(clippy::disallowed_methods, reason = "a fake delegating the boot path to the fake it wraps")]
        let rows = self.0.verify_anchor(plan);
        rows
    }
    fn execute_raw(&self, statement: &RawStatement, presented: &Presented, deadline: Deadline) -> RawExecution<Self::Error> {
        self.0.execute_raw(statement, presented, deadline)
    }
}

fn model_at(name: &str, source: SourceName) -> Model {
    Model::new(
        ModelName::parse(name).expect("a fixture model"),
        source,
        TableName::parse(name).expect("a fixture table"),
        [Column::new(
            ColumnName::parse("id").expect("a fixture column"),
            Some(ColumnType::parse("BIGINT").expect("a fixture type")),
            Description::default(),
            Some(false),
        )],
        Description::default(),
    )
}

fn bundle(elsewhere: &SourceName) -> PinnedDefinitions {
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a fixture version"),
        Definitions::assemble(
            vec![
                model_at("visible_table", source()).with_audience(Audience::Open),
                model_at("other_table", elsewhere.clone()).with_audience(Audience::Open),
            ],
            vec![],
            vec![],
        )
        .expect("the fixture is consistent"),
        Knowledge::none(),
        ContributionManifest::parse([
            (source(), Contribution::of(MetadataCapabilities::nothing())),
            (elsewhere.clone(), Contribution::of(MetadataCapabilities::nothing())),
        ])
        .expect("the fixture manifest parses"),
    )
    .expect("the fixture pins")
}

/// `elsewhere` is a real, open source, with a real dialect so its model is admissible to the
/// planner on that count alone - just not one `tools.run_sql.sources` lists for this caller.
/// Naming it explicitly must reach no further than an unlisted source already did: the caller's
/// own table list never carries its model, so the resolver has nothing of its to match, prefixed
/// or not, and nothing is ever minted for it. The listed source's own table, by contrast, must
/// answer - proving the exclusion is the scope filter, not a missing dialect.
#[test]
fn a_source_the_scope_does_not_list_stays_unreachable_even_named_explicitly() {
    let elsewhere = SourceName::parse("elsewhere").expect("a fixture source name");
    let warehouses = Warehouses::of(Physical(RawCapableWarehouse::answering_rows(source(), shared(), 1)))
        .and(Physical(RawCapableWarehouse::answering_rows(elsewhere.clone(), shared(), 1)))
        .expect("two distinct sources register");
    let pinned = verify_and_validate(bundle(&elsewhere), &warehouses).expect("the declared schemas validate");
    let scope = RunSqlScope::new(run_sql_sources(vec![source()]), true);

    let visible_broker = CountingBroker::default();
    let visible = RawStatement::parse("select id from visible_table").expect("a bounded query");
    let visible_answer = answer_sql(
        &pinned,
        &asked_by_a_person(),
        &visible,
        &visible_broker,
        &warehouses,
        &DataFusionCombiner::new().expect("the planner builds"),
        &scope,
        1 << 30,
        test_deadline(),
    )
    .expect("the listed table answers")
    .into_outcome();
    let RawOutcome::Rows { rows, .. } = visible_answer else {
        panic!("the listed table must answer, not {visible_answer:?}");
    };
    assert_eq!(rows.len(), 1, "the fixture warehouse answers one row");
    assert_eq!(
        visible_broker.asked(),
        1,
        "the listed source is minted for exactly once to answer its own table"
    );

    let broker = CountingBroker::default();
    let statement = RawStatement::parse("select id from elsewhere.other_table").expect("a bounded query");
    let answer = answer_sql(
        &pinned,
        &asked_by_a_person(),
        &statement,
        &broker,
        &warehouses,
        &DataFusionCombiner::new().expect("the planner builds"),
        &scope,
        1 << 30,
        test_deadline(),
    )
    .expect("a refusal is an answer");
    let RawOutcome::Refusal { reason } = answer.into_outcome() else {
        panic!("the query must be refused");
    };
    assert_eq!(reason, RawRefusalReason::UnknownTable);
    assert_eq!(
        broker.asked(),
        0,
        "an unlisted source must never be minted for, named explicitly or not"
    );
}
