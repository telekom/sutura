//! `DuckDB` carries the raw condition, for a column pinned binary.

use super::PrefilterCapability;
use crate::warehouse::schema::TypeDialect;

#[test]
fn duckdb_is_a_capable_arm() {
    assert!(PrefilterCapability::of(TypeDialect::DuckDb).is_some());
}
