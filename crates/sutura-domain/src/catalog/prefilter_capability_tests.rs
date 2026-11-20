//! Which engine families may carry the raw necessary condition, and which keep the redacted-only path.

use super::PrefilterCapability;
use crate::warehouse::schema::TypeDialect;

#[test]
fn only_engines_with_comparison_evidence_carry_the_raw_condition() {
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
        TypeDialect::Postgres,
        TypeDialect::ClickHouse,
        TypeDialect::BigQuery,
    ] {
        if dialect == TypeDialect::DuckDb {
            assert!(PrefilterCapability::of(dialect).is_some(), "{dialect:?}");
        } else {
            assert_eq!(PrefilterCapability::of(dialect), None, "{dialect:?}");
        }
    }
}
