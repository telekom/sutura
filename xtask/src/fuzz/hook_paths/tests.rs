//! The correlation rules, exercised in isolation from the tree.
//!
//! A `mod tests;` of its own, deliberately: `crate::fuzz`'s inline `#[cfg(test)]` module already
//! carries the production plus test lines in ONE file, and adding this module's cases there would
//! put production and test changes in one file too - the shape `xtask/src/causality/remedies.rs`
//! answers `NOT MECHANICALLY SEPARABLE` with. A crate reader and its cases in this separate file
//! keep `check-fuzz`'s production change revertible apart from the tests that prove it, the same
//! split `crate::newtype_leaks` and `crate::branches::decide` make.

use super::{crates_of, fuzzed_tree_row, hook_regex, missing_from_regex, missing_from_row, regex_reaches, row_reaches};
use crate::hooks;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn a_targets_crates_are_read_off_its_sutura_identifiers() {
    let source = "use sutura_exec_bigquery::wire::document::decode_answer;\nuse sutura_domain::query::Query;\n// a comment sutura_domain::other\n";
    let got = crates_of(source);
    assert!(got.contains("crates/sutura-exec-bigquery"), "{got:?}");
    assert!(got.contains("crates/sutura-domain"), "{got:?}");
    // `_` -> `-` mapping applied, no trailing slash, and no other word became a crate.
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(!got.iter().any(|c| c.ends_with('/')), "{got:?}");
}

#[test]
fn a_crate_id_is_checked_against_both_surfaces() {
    // The regex and the row both reach the crate by prefix.
    assert!(regex_reaches(
        "(fuzz/|crates/sutura-exec-bigquery/|crates/sutura-sql/)",
        "crates/sutura-exec-bigquery"
    ));
    assert!(row_reaches(
        &["fuzz/**", "crates/sutura-exec-bigquery/**"],
        "crates/sutura-exec-bigquery"
    ));
    // And an omitted crate is red against BOTH.
    assert!(!regex_reaches("(fuzz/|crates/sutura-sql/)", "crates/sutura-exec-bigquery"));
    assert!(!row_reaches(
        &["fuzz/**", "crates/sutura-sql/**"],
        "crates/sutura-exec-bigquery"
    ));
}

#[test]
fn the_fuzz_hooks_regex_is_read_through_hooks() {
    let text = "- id: fuzz\n  name: fuzz (git delta)\n  files: ^(fuzz/|crates/sutura-sql/)\n- id: rust-fmt\n  name: cargo fmt\n";
    let declared = hooks::hooks(text);
    assert_eq!(hook_regex(&declared).as_deref(), Some("^(fuzz/|crates/sutura-sql/)"));
    // A hook with no files: yields None rather than an empty claim.
    let nested = hooks::hooks("- id: rust-fmt\n  name: cargo fmt\n");
    assert_eq!(hook_regex(&nested), None);
}

#[test]
fn the_fuzzed_tree_row_is_found_by_label() {
    let Some(paths) = fuzzed_tree_row() else {
        panic!("the surfaces table must carry a `fuzzed tree` row");
    };
    assert!(paths.contains(&"fuzz/**"), "{paths:?}");
    assert!(
        paths.iter().any(|glob| glob.starts_with("crates/sutura-exec-bigquery")),
        "{paths:?}"
    );
}

#[test]
fn a_crate_missing_from_both_surfaces_is_named_as_the_red_case() {
    let mut bound: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut crates = BTreeSet::new();
    crates.insert(String::from("crates/sutura-exec-bigquery"));
    crates.insert(String::from("crates/sutura-domain"));
    bound.insert(String::from("bigquery_answer"), crates);

    // Both surfaces reach the crate -> green.
    assert_eq!(
        missing_from_regex(
            &bound,
            Some("^(fuzz/|crates/sutura-exec-bigquery/|crates/sutura-domain/src/)")
        ),
        Vec::new()
    );
    assert_eq!(
        missing_from_row(
            &bound,
            Some(&["fuzz/**", "crates/sutura-exec-bigquery/**", "crates/sutura-domain/**"])
        ),
        Vec::new()
    );

    // Regex omits exec-bigquery -> that pair is the red case, naming target and crate.
    let missing_regex = missing_from_regex(&bound, Some("^(fuzz/|crates/sutura-domain/)"));
    assert_eq!(missing_regex.len(), 1, "{missing_regex:?}");
    assert_eq!(
        missing_regex[0],
        (String::from("bigquery_answer"), String::from("crates/sutura-exec-bigquery"))
    );

    // Row omits exec-bigquery -> red there too.
    let missing_row = missing_from_row(&bound, Some(&["fuzz/**", "crates/sutura-domain/**"]));
    assert_eq!(missing_row.len(), 1, "{missing_row:?}");
    assert_eq!(
        missing_row[0],
        (String::from("bigquery_answer"), String::from("crates/sutura-exec-bigquery"))
    );

    // A missing row (None) names everything, loudest honest answer.
    let none_row = missing_from_row(&bound, None);
    assert_eq!(none_row.len(), 2, "{none_row:?}");
}

#[test]
fn a_missing_regex_value_is_no_gap_only_where_it_is_the_unfiltered_spelling() {
    // None (no files:) -> the hook runs on every diff, so nothing to report.
    let mut bound: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut c = BTreeSet::new();
    c.insert(String::from("crates/sutura-exec-bigquery"));
    bound.insert(String::from("b"), c);
    assert_eq!(missing_from_regex(&bound, None), Vec::new());
    // An empty or whitespace files: value is filtered to None by `hook_regex`, so it is the
    // unfiltered spelling too - asserted here on the reader, not by reaching a run.
    let declared = hooks::hooks("- id: fuzz\n  name: fuzz (git delta)\n  files:\n");
    assert_eq!(hook_regex(&declared).as_deref(), None);
}
