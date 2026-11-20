//! Which engine families may carry the raw necessary condition, and which keep the redacted-only path.

use super::{Collation, PrefilterCapability};
use crate::warehouse::schema::TypeDialect;

#[test]
fn only_engines_with_comparison_evidence_carry_the_raw_condition() {
    let capable = [
        (TypeDialect::DataFusion, PrefilterCapability::DataFusion),
        (TypeDialect::Oracle, PrefilterCapability::Oracle),
        (TypeDialect::BigQuery, PrefilterCapability::BigQuery),
    ];
    for (dialect, capability) in capable {
        assert_eq!(PrefilterCapability::of(dialect), Some(capability), "{dialect:?}");
    }
    for dialect in [
        TypeDialect::DuckDb,
        TypeDialect::Cube,
        TypeDialect::Postgres,
        TypeDialect::ClickHouse,
    ] {
        assert_eq!(PrefilterCapability::of(dialect), None, "{dialect:?}");
    }
}

#[test]
fn bigquery_admits_a_column_only_when_the_catalog_pins_it_binary() {
    let bigquery = PrefilterCapability::BigQuery;
    assert!(bigquery.admits(&Collation::Binary));
    for collation in [Collation::Unknown, Collation::named("und:ci"), Collation::named("binary")] {
        assert!(!bigquery.admits(&collation), "{collation:?}");
    }
    for engine in [PrefilterCapability::DataFusion, PrefilterCapability::Oracle] {
        assert!(
            engine.admits(&Collation::Unknown),
            "{engine:?} decides without a pinned collation"
        );
    }
}
