//! Which engine families may carry the raw necessary condition, and which keep the redacted-only path.

use super::PrefilterCapability;
use crate::warehouse::schema::TypeDialect;

#[test]
fn only_engines_with_comparison_evidence_carry_the_raw_condition() {
    let capable = [
        (TypeDialect::DataFusion, PrefilterCapability::DataFusion),
        (TypeDialect::Oracle, PrefilterCapability::Oracle),
        (TypeDialect::DuckDb, PrefilterCapability::DuckDb),
    ];
    for (dialect, capability) in capable {
        assert_eq!(PrefilterCapability::of(dialect), Some(capability), "{dialect:?}");
        assert_eq!(
            capability.needs_binary_column(),
            capability == PrefilterCapability::DuckDb,
            "{dialect:?}"
        );
    }
    for dialect in [
        TypeDialect::Cube,
        TypeDialect::Postgres,
        TypeDialect::ClickHouse,
        TypeDialect::BigQuery,
    ] {
        assert_eq!(PrefilterCapability::of(dialect), None, "{dialect:?}");
    }
}
