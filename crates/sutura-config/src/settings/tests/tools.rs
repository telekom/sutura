//! `tools.run_sql.enabled`'s own boot refusal - `docs/adr/0013`'s raw SQL tool.
//!
//! A file of its own for the reason `settings/tests/sources.rs` has one: `settings/tests.rs`
//! reached the thousand-line limit `cargo xtask max-lines` enforces.

use crate::settings::{Environment, NotFitToServe, Settings, SettingsError, Sources};

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
