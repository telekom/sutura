//! ClickHouse raw prefilter admission is tied to a binary column fact.

use super::{Collation, PrefilterCapability};
use crate::warehouse::schema::TypeDialect;

#[test]
fn only_engines_with_comparison_evidence_carry_the_raw_condition() {
    let capable = [
        (TypeDialect::DataFusion, PrefilterCapability::DataFusion),
        (TypeDialect::Oracle, PrefilterCapability::Oracle),
        (TypeDialect::ClickHouse, PrefilterCapability::ClickHouse),
    ];
    for (dialect, capability) in capable {
        assert_eq!(PrefilterCapability::of(dialect), Some(capability), "{dialect:?}");
    }
    for dialect in [
        TypeDialect::DuckDb,
        TypeDialect::Cube,
        TypeDialect::Postgres,
        TypeDialect::BigQuery,
    ] {
        assert_eq!(PrefilterCapability::of(dialect), None, "{dialect:?}");
    }
}

#[test]
fn clickhouse_is_admitted_for_a_binary_column_only_and_the_other_arms_ignore_the_fact() {
    for collation in [Collation::Unknown, Collation::named("und:ci")] {
        assert!(!PrefilterCapability::ClickHouse.admits(&collation), "{collation:?}");
    }
    assert!(PrefilterCapability::ClickHouse.admits(&Collation::Binary));
    for capability in [PrefilterCapability::DataFusion, PrefilterCapability::Oracle] {
        assert!(capability.admits(&Collation::Unknown), "{capability:?}");
    }
}
