//! Which engine families may carry the raw necessary condition, and which keep the redacted-only path.

use super::{Collation, PrefilterCapability};
use crate::warehouse::schema::TypeDialect;

#[test]
#[expect(
    clippy::match_same_arms,
    reason = "one arm per line, so each engine PR flips only its own line and merges cleanly"
)]
fn only_engines_with_comparison_evidence_carry_the_raw_condition() {
    // One arm per dialect and no wildcard: a new dialect is a compile error here until it is decided.
    let capable = [
        (TypeDialect::DataFusion, PrefilterCapability::DataFusion),
        (TypeDialect::Oracle, PrefilterCapability::Oracle),
        (TypeDialect::Postgres, PrefilterCapability::Postgres),
    ];
    for (dialect, capability) in capable {
        assert_eq!(PrefilterCapability::of(dialect), Some(capability), "{dialect:?}");
    }
    for dialect in [TypeDialect::DuckDb, TypeDialect::BigQuery] {
        let capability = PrefilterCapability::of(dialect).expect("the new engine arm exists");
        assert!(capability.admits(&Collation::Binary), "{dialect:?}");
        for collation in [Collation::Unknown, Collation::named("und:ci")] {
            assert!(!capability.admits(&collation), "{dialect:?} {collation:?}");
        }
    }
    let clickhouse = PrefilterCapability::of(TypeDialect::ClickHouse).expect("the ClickHouse arm exists");
    assert!(clickhouse.admits(&Collation::Binary));
    for collation in [Collation::named("und:ci"), Collation::Unknown] {
        assert!(!clickhouse.admits(&collation), "{collation:?}");
    }
    for dialect in [
        TypeDialect::DuckDb,
        TypeDialect::DataFusion,
        TypeDialect::Cube,
        TypeDialect::Postgres,
        TypeDialect::ClickHouse,
        TypeDialect::BigQuery,
        TypeDialect::Oracle,
    ] {
        let carries = match dialect {
            TypeDialect::DuckDb => true,
            TypeDialect::DataFusion => true,
            TypeDialect::Cube => false,
            TypeDialect::Postgres => true,
            TypeDialect::ClickHouse => true,
            TypeDialect::BigQuery => true,
            TypeDialect::Oracle => true,
        };
        assert_eq!(PrefilterCapability::of(dialect).is_some(), carries, "{dialect:?}");
    }
}
