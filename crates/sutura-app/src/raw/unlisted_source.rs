//! #2106's review: a source this caller's scope does not list must stay unreachable even when a
//! statement names it explicitly - `tables()`'s pre-existing scope filter, not the resolver, is
//! what has to hold this, and the resolver gaining a source-prefix reading must not change it.
//! Declared here rather than in `tests.rs`, so the base run, which restores this file, does not
//! compile it.

use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Audience, Column, ColumnType, Definitions, Description, Model};
use sutura_domain::knowledge::Knowledge;
use sutura_domain::model::{ColumnName, ModelName, SourceName, TableName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};
use sutura_domain::raw::{RawOutcome, RawRefusalReason, RawStatement};
use sutura_exec_datafusion::DataFusionCombiner;

use super::{RunSqlScope, answer_sql};
use crate::tests::{asked_by_a_person, shared, source, test_deadline};
use crate::tests_support::{CountingBroker, RawCapableWarehouse, run_sql_sources};
use crate::{Warehouses, verify_and_validate};

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

/// `elsewhere` is a real, open source - just not one `tools.run_sql.sources` lists for this
/// caller. Naming it explicitly must reach no further than an unlisted source already did: the
/// caller's own table list never carries its model, so the resolver has nothing of its to match,
/// prefixed or not, and nothing is ever minted for it.
#[test]
fn a_source_the_scope_does_not_list_stays_unreachable_even_named_explicitly() {
    let elsewhere = SourceName::parse("elsewhere").expect("a fixture source name");
    let warehouses = Warehouses::of(RawCapableWarehouse::answering_rows(source(), shared(), 1))
        .and(RawCapableWarehouse::answering_rows(elsewhere.clone(), shared(), 1))
        .expect("two distinct sources register");
    let pinned = verify_and_validate(bundle(&elsewhere), &warehouses).expect("the declared schemas validate");
    let broker = CountingBroker::default();
    let statement = RawStatement::parse("select id from elsewhere.other_table").expect("a bounded query");
    let scope = RunSqlScope::new(run_sql_sources(vec![source()]), true);
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
