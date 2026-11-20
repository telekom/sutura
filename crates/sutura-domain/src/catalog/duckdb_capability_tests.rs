//! `DuckDB` carries the raw condition only for a column pinned binary.

use super::PrefilterCapability;
use crate::warehouse::schema::TypeDialect;

#[test]
fn duckdb_is_a_capable_arm_that_needs_a_binary_column() {
    let capability = PrefilterCapability::of(TypeDialect::DuckDb).expect("DuckDB has a comparison arm");
    assert_eq!(capability, PrefilterCapability::DuckDb);
    assert!(capability.needs_binary_column());
    for other in [PrefilterCapability::DataFusion, PrefilterCapability::Oracle] {
        assert!(!other.needs_binary_column(), "{other:?}");
    }
}
