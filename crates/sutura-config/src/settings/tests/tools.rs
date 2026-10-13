//! `tools.run_sql.enabled`'s own boot refusal - the raw SQL tool runs in `multi-user` mode only over
//! an acknowledged shared source.
//!
//! A file of its own for the reason `settings/tests/sources.rs` has one: `settings/tests.rs`
//! reached the thousand-line limit `cargo xtask max-lines` enforces.

use sutura_domain::model::SourceName;

use crate::settings::{Environment, NotFitToServe, Settings, SettingsError, Sources};

const MULTI_USER_RUN_SQL: &str = "security:\n  identity: \"multi-user\"\ntools:\n  run_sql:\n    enabled: true\n";
const LOCAL: &str = "sources:\n  local:\n    kind: \"files\"\n    data_dir: \"/srv/sutura/data\"\n";
const SHARED: &str = "    posture: \"shared-service-user\"\n";
const ACKNOWLEDGED: &str = "    acknowledged_because: \"a read-only reporting replica every caller may see\"\n";
const IMPERSONATING: &str = "    posture: \"impersonation-at-source\"\n    workload_identity:\n      audience: \
     \"//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso\"\n";

/// What boot refuses, or nothing when it serves - so a cell's own assertion is what fails either way.
fn refusals_of(overlay: String) -> Vec<NotFitToServe> {
    match Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay)) {
        Ok(_) => Vec::new(),
        Err(error) => match *error.reason() {
            SettingsError::NotFitToServe { ref refusals } => refusals.clone(),
            ref other => panic!("expected a posture refusal, got {other:?}"),
        },
    }
}

#[test]
fn the_raw_tool_in_multi_user_mode_with_no_source_is_not_fit_to_serve() {
    let refusals = refusals_of(String::from(MULTI_USER_RUN_SQL));
    assert_eq!(refusals, vec![NotFitToServe::RunSqlEnabledInMultiUserMode]);
    let rendered = refusals.first().expect("one refusal").to_string();
    assert!(rendered.contains("tools.run_sql.enabled"), "{rendered}");
    assert!(rendered.contains("multi-user"), "{rendered}");
    assert!(
        rendered.contains("every caller can then read what the shared account can read"),
        "{rendered}"
    );
}

#[test]
fn the_raw_tool_in_multi_user_mode_over_only_an_impersonating_source_is_not_fit_to_serve() {
    let refusals = refusals_of(format!("{MULTI_USER_RUN_SQL}{LOCAL}{IMPERSONATING}"));
    assert_eq!(refusals, vec![NotFitToServe::RunSqlEnabledInMultiUserMode]);
}

#[test]
fn the_raw_tool_in_multi_user_mode_over_an_unacknowledged_shared_source_is_not_fit_to_serve() {
    // The missing acknowledgement fires first, and the raw tool's own refusal follows it: an
    // unacknowledged source is no acknowledged one.
    let refusals = refusals_of(format!("{MULTI_USER_RUN_SQL}{LOCAL}{SHARED}"));
    assert_eq!(
        refusals,
        vec![
            NotFitToServe::SharedSourceNotAcknowledged {
                alias: SourceName::parse("local").expect("a legal identifier")
            },
            NotFitToServe::RunSqlEnabledInMultiUserMode,
        ]
    );
}

#[test]
fn the_raw_tool_in_multi_user_mode_over_an_acknowledged_shared_source_is_fit_to_serve() {
    let refusals = refusals_of(format!("{MULTI_USER_RUN_SQL}{LOCAL}{SHARED}{ACKNOWLEDGED}"));
    assert!(refusals.is_empty(), "{refusals:?}");
}

#[test]
fn the_raw_tool_in_multi_user_mode_over_a_shared_and_an_impersonating_source_is_fit_to_serve() {
    // One acknowledged shared source is enough: an `impersonation-at-source` source beside it does
    // not bring the refusal back.
    let other = format!("  other:\n    kind: \"files\"\n    data_dir: \"/srv/sutura/other\"\n{IMPERSONATING}");
    let refusals = refusals_of(format!("{MULTI_USER_RUN_SQL}{LOCAL}{SHARED}{ACKNOWLEDGED}{other}"));
    assert!(refusals.is_empty(), "{refusals:?}");
}

#[test]
fn the_raw_tool_enabled_in_multi_user_mode_is_not_fit_to_serve() {
    let sources = Sources::defaults(Environment::Development)
        .with_overlay("security:\n  identity: \"multi-user\"\ntools:\n  run_sql:\n    enabled: true\n");
    let error = Settings::load(&sources).expect_err("the raw tool over a shared identity does not serve");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(*refusals, vec![NotFitToServe::RunSqlEnabledInMultiUserMode]);
    let rendered = refusals.first().expect("one refusal").to_string();
    assert!(rendered.contains("tools.run_sql.enabled"), "{rendered}");
    assert!(rendered.contains("multi-user"), "{rendered}");
}

#[test]
fn the_raw_tool_enabled_in_single_user_mode_is_fit_to_serve() {
    // The other half of the same predicate, so the refusal cannot be satisfied by refusing every
    // deployment that turns the tool on - only the mode it names.
    let sources = Sources::defaults(Environment::Development).with_overlay(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"one operator, their own files\"\ntools:\n  run_sql:\n    enabled: true\n",
    );
    let settings = Settings::load(&sources).expect("single-user with the raw tool on serves");
    assert!(settings.tools().run_sql_enabled());
}

#[test]
fn the_raw_tool_off_in_multi_user_mode_is_fit_to_serve() {
    // And the tool being off is never what this refusal is about - a multi-user deployment that
    // never turns it on is the ordinary case `docs/adr/0013` calls unaffected.
    let sources = Sources::defaults(Environment::Development).with_overlay("security:\n  identity: \"multi-user\"\n");
    let settings = Settings::load(&sources).expect("multi-user with the raw tool off serves");
    assert!(!settings.tools().run_sql_enabled());
}
