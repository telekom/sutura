//! The metrics credential's two startup refusals.
//!
//! A file of its own because `settings/tests.rs` reached the thousand-line limit `cargo xtask
//! max-lines` enforces. The seam is the one the security tree already has: [`crate::security`] owns
//! the token and [`crate::settings::posture`] owns the refusals, so these sit beside the tests for
//! the rest of the security group rather than inside them.

use super::{METRICS_TOKEN, TOKEN};
use crate::settings::{Environment, NotFitToServe, Settings, SettingsError, Sources};

#[test]
fn a_metrics_token_equal_to_the_api_token_is_refused() {
    // The separation `docs/adr/0015` Decision 1 exists for, and the one failure nothing at runtime
    // would show: one token behind both surfaces hands the monitoring system every ability a holder
    // of the deployment token has. The control is the test below, so this cannot pass by refusing
    // every configuration that names both credentials.
    let sources = Sources::defaults(Environment::Development).with_overlay(format!(
        "security:\n  access_token: \"{TOKEN}\"\n  metrics_token: \"{TOKEN}\"\n"
    ));
    let error = Settings::load(&sources).expect_err("one token behind both surfaces is refused");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(*refusals, vec![NotFitToServe::MetricsTokenSharesTheApiToken]);
}

#[test]
fn two_different_credentials_start_a_loopback_development_service() {
    // The other side of the refusal above: distinct values are the shape that serves.
    let sources = Sources::defaults(Environment::Development).with_overlay(format!(
        "security:\n  access_token: \"{TOKEN}\"\n  metrics_token: \"{METRICS_TOKEN}\"\n"
    ));
    let settings = Settings::load(&sources).expect("two distinct credentials serve");
    assert!(settings.security().access_token().is_some());
    assert!(settings.security().metrics_token().is_some());
}

#[test]
fn a_metrics_token_that_is_not_a_token_is_a_settings_error() {
    // The credential has the same floor as the API token, through the same parser - a short value is
    // a typed error naming the key, not a startup that accepts it and compares it as if it did.
    let sources = Sources::defaults(Environment::Development).with_overlay("security:\n  metrics_token: \"too-short\"\n");
    let error = Settings::load(&sources).expect_err("a short metrics token is refused");
    assert!(matches!(error.reason(), SettingsError::MetricsToken { .. }), "{error:?}");
}
