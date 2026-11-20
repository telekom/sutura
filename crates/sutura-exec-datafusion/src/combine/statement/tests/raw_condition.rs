//! The raw condition renders through each dialect's own identifier quoting.

use super::super::text::raw_column;
use super::super::text_prefilter::raw_condition;
use super::*;
use sutura_domain::catalog::PrefilterCapability;

#[test]
fn a_backtick_dialect_renders_the_raw_condition_with_quoted_qualified_identifiers() {
    for dialect in [TypeDialect::BigQuery, TypeDialect::ClickHouse] {
        let column = || raw_column(dialect, "notes", "body");
        assert_eq!(
            raw_condition(PrefilterCapability::DataFusion, column(), "TERM", false).to_string(),
            "(`notes`.`body` LIKE '%TERM%')",
            "{dialect:?}"
        );
        assert_eq!(
            raw_condition(PrefilterCapability::Oracle, column(), "TERM", true).to_string(),
            "CASE WHEN SYS_CONTEXT('USERENV', 'NLS_SORT') COLLATE BINARY = 'BINARY' THEN CASE WHEN UPPER(`notes`.`body`) LIKE '%TERM%' THEN 1 ELSE 0 END ELSE 1 END = 1",
            "{dialect:?}"
        );
    }
}
