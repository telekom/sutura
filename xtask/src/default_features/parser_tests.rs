//! The default build links no SQL parser: no shipped root may resolve `sqlparser` or `datafusion-sql`
//! at cargo's default feature set.

use super::inspect_features;

const CLEAN: &str = "root v0.1.0|\nsutura-domain v0.1.0|\n";

#[test]
fn a_shipped_root_resolving_an_sql_parser_is_refused() {
    for parser in ["sqlparser v0.62.0|default,std", "datafusion-sql v55.1.0|unicode_expressions"] {
        assert_eq!(
            inspect_features(&format!("{CLEAN}{parser}\n"), "root"),
            Err("an SQL parser (sqlparser or datafusion-sql) is in the default build's normal target dependencies"),
            "{parser}"
        );
    }
    for clean in [
        "sqlparser_derive v0.4.0|",
        "datafusion-sql-extra v0.1.0|",
        "datafusion v55.1.0|parquet",
    ] {
        assert_eq!(inspect_features(&format!("{CLEAN}{clean}\n"), "root"), Ok(()), "{clean}");
    }
}
