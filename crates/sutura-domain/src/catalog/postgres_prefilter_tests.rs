//! PostgreSQL is the one prefilter arm that asks the pinned column, because it binds a collation to each.

use super::PrefilterCapability;
use crate::warehouse::schema::TypeDialect;

#[test]
fn postgres_is_the_one_prefilter_arm_that_asks_the_pinned_column() {
    assert_eq!(
        PrefilterCapability::of(TypeDialect::Postgres),
        Some(PrefilterCapability::Postgres)
    );
    for capability in [
        PrefilterCapability::DataFusion,
        PrefilterCapability::Oracle,
        PrefilterCapability::Postgres,
    ] {
        assert_eq!(
            capability.requires_binary_column(),
            capability == PrefilterCapability::Postgres,
            "{capability:?}"
        );
    }
}

#[test]
fn the_engines_without_a_prefilter_arm_exclude_postgres() {
    let capable = [
        (TypeDialect::DataFusion, PrefilterCapability::DataFusion),
        (TypeDialect::Oracle, PrefilterCapability::Oracle),
    ];
    for (dialect, capability) in capable {
        assert_eq!(PrefilterCapability::of(dialect), Some(capability), "{dialect:?}");
    }
    for dialect in [
        TypeDialect::DuckDb,
        TypeDialect::Cube,
        TypeDialect::ClickHouse,
        TypeDialect::BigQuery,
    ] {
        assert_eq!(PrefilterCapability::of(dialect), None, "{dialect:?}");
    }
}
