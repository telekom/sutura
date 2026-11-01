//! `tools.run_sql`'s source list and federation switch against this build's `sql` feature.

use crate::settings::{Environment, NotFitToServe, Settings, SettingsError, Sources};

#[test]
fn federation_allowed_true_is_refused_only_without_the_sql_feature() {
    // Both arms, because which holds is a property of the build. Every venue running this crate's
    // tests passes `--all-features`, so the refusing arm is compiled here and run by none: the
    // `sutura-cli` cell under `serve`'s tests runs it in the default-feature lane.
    let loaded = Settings::load(
        &Sources::defaults(Environment::Development).with_overlay("tools:\n  run_sql:\n    federation_allowed: true\n"),
    );
    if cfg!(feature = "sql") {
        let settings = loaded.expect("a build with the SQL frontend accepts the switch");
        assert!(settings.tools().run_sql_federation_allowed());
        assert!(!settings.tools().run_sql_enabled(), "the switch does not turn the tool on");
    } else {
        let error = loaded.expect_err("a build with no SQL parser cannot federate a statement");
        let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
            panic!("expected a posture refusal, got {error:?}");
        };
        assert_eq!(*refusals, vec![NotFitToServe::RunSqlFederationNotCompiledIn]);
    }
}

#[test]
fn the_raw_tool_on_over_an_empty_list_is_refused_for_the_list() {
    let loaded = Settings::load(&Sources::defaults(Environment::Development).with_overlay(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"one operator\"\ntools:\n  run_sql:\n    enabled: true\n    sources: []\n",
    ));
    let error = loaded.expect_err("a raw tool over nothing listed does not serve");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(*refusals, vec![NotFitToServe::RunSqlListsNoSource]);
}
