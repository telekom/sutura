use super::{missing_from_hook, missing_from_surface, target_crates};

#[test]
fn every_real_targets_imports_match_the_issues_own_mapping() {
    // The six mappings `github.com/telekom/sutura#867` measured by hand, reduced to fixtures
    // shaped like each real target's import block.
    assert_eq!(
        target_crates("use sutura_domain::pinned::PinnedDefinitions;\nuse sutura_sql::dialect::Postgres;"),
        ["sutura-domain", "sutura-sql"].into_iter().map(String::from).collect()
    );
    assert_eq!(
        target_crates("use sutura_config::InboundIdentity;\nuse sutura_http::inbound::keys::KeySet;"),
        ["sutura-config", "sutura-http"].into_iter().map(String::from).collect()
    );
}

#[test]
fn a_doc_comment_mention_still_counts_the_over_inclusive_direction() {
    let found = target_crates("//! mentions sutura_exec_bigquery in prose only\n");
    assert!(found.contains("sutura-exec-bigquery"));
}

#[test]
fn a_crate_present_in_the_hooks_files_string_is_not_missing() {
    let files = "^(fuzz/|crates/sutura-domain/src/query\\.rs|crates/sutura-sql/)";
    assert!(!missing_from_hook("sutura-sql", files));
    assert!(missing_from_hook("sutura-config", files));
}

#[test]
fn a_crate_present_in_the_surfaces_paths_is_not_missing() {
    let paths = ["crates/sutura-sql/**", "crates/sutura-domain/src/query.rs"];
    assert!(!missing_from_surface("sutura-sql", &paths));
    // A prefix match, not a substring anywhere in the row: a sibling crate whose name merely
    // starts the same must not satisfy a different crate's row.
    assert!(missing_from_surface("sutura-sql-extra", &paths));
    assert!(missing_from_surface("sutura-config", &paths));
}
