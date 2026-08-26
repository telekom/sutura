//! What the layering does, and what the posture refuses.
//!
//! Hermetic: no test here reads a file or the process environment. [`Sources::defaults`] supplies
//! an empty variable map, which replaces the real environment, so a `SUTURA__*` variable in a
//! developer's shell cannot change a verdict.

use std::collections::BTreeMap;

use super::{Environment, NotFitToServe, Settings, SettingsError, Sources};
use crate::server::{BodyLimit, RequestTimeout};
use crate::telemetry::LogFormat;

/// A token that satisfies the length floor, for the cases that need one.
const TOKEN: &str = "0123456789abcdef0123456789abcdef";

fn variables(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|&(key, value)| (String::from(key), String::from(value)))
        .collect()
}

/// A production configuration that is otherwise fine, so a test can break exactly one thing.
fn production_overlay() -> String {
    format!(
        "server:\n  host: \"0.0.0.0\"\n  port: 8080\nsecurity:\n  access_token: \"{TOKEN}\"\n  \
         expose_beyond_loopback: true\n"
    )
}

// ------------------------------------------------------------------ defaults ----

#[test]
fn the_embedded_defaults_are_a_loopback_development_service() {
    // The posture claim in `defaults.yaml`, asserted. If a default is ever loosened, this is what
    // fails - which is the point of embedding them rather than shipping a file.
    let settings = Settings::load(&Sources::defaults(Environment::Development)).expect("the defaults load");
    assert!(settings.server().bind().is_loopback());
    assert_eq!(settings.server().bind().port(), 8080);
    assert!(settings.security().access_token().is_none());
    assert!(!settings.security().expose_beyond_loopback());
    assert!(settings.rate_limit().enabled());
    assert!(settings.refusals().is_empty());
}

#[test]
fn the_defaults_alone_do_not_start_a_production_deployment() {
    // The single most important assertion in this file: shipping the defaults to production is a
    // refusal, not a permissive service. Two refusals, because two separate controls are missing.
    let error =
        Settings::load(&Sources::defaults(Environment::Production)).expect_err("the defaults are not a production posture");
    let SettingsError::NotFitToServe { ref refusals } = error else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert!(
        refusals
            .iter()
            .any(|r| matches!(*r, NotFitToServe::AccessTokenRequired { .. })),
        "{refusals:?}"
    );
}

// ------------------------------------------------------- the layering itself ----

#[test]
fn an_overlay_beats_the_defaults_and_a_variable_beats_the_overlay() {
    // The precedence contract in one test. Three layers set the same key; the last one wins.
    let sources = Sources::defaults(Environment::Development)
        .with_overlay("server:\n  port: 9000\n")
        .with_variables(variables(&[("SUTURA__SERVER__PORT", "9100")]));
    let settings = Settings::load(&sources).expect("three layers of one key still load");
    assert_eq!(settings.server().bind().port(), 9100);

    // And without the variable, the overlay is the last word rather than the default.
    let sources = Sources::defaults(Environment::Development).with_overlay("server:\n  port: 9000\n");
    let settings = Settings::load(&sources).expect("two layers of one key still load");
    assert_eq!(settings.server().bind().port(), 9000);
}

#[test]
fn a_numeric_value_arriving_as_a_variable_string_is_still_a_number() {
    // Worth its own test because it is a property of the layering rather than of our code: every
    // environment variable is a string, and the tree these deserialize into is typed. If the
    // coercion ever stops happening, every numeric override starts failing at startup.
    let sources = Sources::defaults(Environment::Development).with_variables(variables(&[
        ("SUTURA__SERVER__REQUEST_TIMEOUT_SECONDS", "45"),
        ("SUTURA__SERVER__MAX_BODY_BYTES", "2048"),
        ("SUTURA__RATE_LIMIT__ENABLED", "false"),
    ]));
    let settings = Settings::load(&sources).expect("string-valued numbers coerce");
    assert_eq!(settings.server().request_timeout().seconds(), 45);
    assert_eq!(settings.server().max_body().bytes(), 2048);
    assert!(!settings.rate_limit().enabled());
}

#[test]
fn a_misspelled_key_is_an_error_and_not_a_silently_ignored_override() {
    // `deny_unknown_fields` is what makes this an error. Without it the service runs on the
    // default the operator believed they had changed, and nothing anywhere says so.
    let sources = Sources::defaults(Environment::Development).with_overlay("server:\n  prot: 9000\n");
    let error = Settings::load(&sources).expect_err("a misspelled key is not a setting");
    assert!(matches!(error, SettingsError::Source { .. }), "{error:?}");
    let rendered = format!("{:?}", core::error::Error::source(&error));
    assert!(rendered.contains("prot"), "the error should name the key: {rendered}");
}

#[test]
fn a_stray_variable_under_the_prefix_is_an_error() {
    // The same protection on the variable layer. A typo in a deployment manifest is otherwise a
    // variable nothing reads.
    let sources = Sources::defaults(Environment::Development).with_variables(variables(&[("SUTURA__SERVER__PROT", "9000")]));
    assert!(matches!(Settings::load(&sources), Err(SettingsError::Source { .. })));
}

#[test]
fn the_environment_is_not_a_configuration_key() {
    // Deliberate: the environment decides which file is layered, so a layer that could set it
    // would be self-referential. Both spellings are unknown fields rather than settings.
    let sources = Sources::defaults(Environment::Development).with_overlay("environment: production\n");
    assert!(matches!(Settings::load(&sources), Err(SettingsError::Source { .. })));
    let sources = Sources::defaults(Environment::Development).with_variables(variables(&[("SUTURA__ENVIRONMENT", "production")]));
    assert!(matches!(Settings::load(&sources), Err(SettingsError::Source { .. })));
}

// ------------------------------------------------------ per-value refusals ----

#[test]
fn an_invalid_value_fails_at_startup_rather_than_falling_back_to_a_default() {
    // The requirement this crate exists for. Each of these is a value somebody wrote down that
    // cannot mean what they intended, and none of them resolves to a default.
    /// Which error a case is expected to produce.
    ///
    /// A named alias because the inline form is over the complexity threshold in `clippy.toml`,
    /// and matching on the variant rather than on a rendered message is what keeps this a test of
    /// the contract rather than of the wording.
    type Expected = fn(&SettingsError) -> bool;

    let cases: &[(&str, Expected)] = &[
        ("server:\n  host: \"not-an-address\"\n", |e| {
            matches!(*e, SettingsError::Bind { .. })
        }),
        ("server:\n  request_timeout_seconds: 0\n", |e| {
            matches!(*e, SettingsError::Bound { .. })
        }),
        ("server:\n  max_body_bytes: 0\n", |e| {
            matches!(*e, SettingsError::Bound { .. })
        }),
        ("security:\n  access_token: \"short\"\n", |e| {
            matches!(*e, SettingsError::AccessToken { .. })
        }),
        ("rate_limit:\n  api_per_second: 0\n", |e| {
            matches!(*e, SettingsError::Quota { .. })
        }),
        ("telemetry:\n  format: \"logfmt\"\n", |e| {
            matches!(*e, SettingsError::Format { .. })
        }),
        ("telemetry:\n  filter: \"\"\n", |e| matches!(*e, SettingsError::Filter { .. })),
        ("telemetry:\n  service_name: \"has a space\"\n", |e| {
            matches!(*e, SettingsError::Service { .. })
        }),
        ("catalog:\n  version: \"\"\n", |e| matches!(*e, SettingsError::Version { .. })),
        ("catalog:\n  dir: \"\"\n", |e| matches!(*e, SettingsError::Catalog { .. })),
    ];
    for &(overlay, is_expected) in cases {
        let sources = Sources::defaults(Environment::Development).with_overlay(overlay);
        let error = Settings::load(&sources).unwrap_err();
        assert!(is_expected(&error), "{overlay:?} produced {error:?}");
    }
}

#[test]
fn a_bound_at_its_ceiling_loads_and_one_past_it_does_not() {
    let at = format!(
        "server:\n  request_timeout_seconds: {}\n  max_body_bytes: {}\n",
        RequestTimeout::MAX_SECONDS,
        BodyLimit::MAX_BYTES
    );
    let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(at))
        .expect("the ceilings themselves are loadable");
    assert_eq!(settings.server().request_timeout().seconds(), RequestTimeout::MAX_SECONDS);
    assert_eq!(settings.server().max_body().bytes(), BodyLimit::MAX_BYTES);
    let past = format!(
        "server:\n  request_timeout_seconds: {}\n",
        RequestTimeout::MAX_SECONDS.saturating_add(1)
    );
    assert!(matches!(
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(past)),
        Err(SettingsError::Bound { .. })
    ));
}

// --------------------------------------------------------- posture refusals ----

#[test]
fn a_non_loopback_bind_needs_an_explicit_acknowledgement_in_every_environment() {
    // Not a production-only rule, and that is deliberate: binding the wildcard is the single
    // change that turns a local tool into a network service, and a laptop on a shared network is
    // where that is least expected.
    for environment in [Environment::Development, Environment::Test, Environment::Production] {
        let sources = Sources::defaults(environment).with_overlay(format!(
            "server:\n  host: \"0.0.0.0\"\nsecurity:\n  access_token: \"{TOKEN}\"\n"
        ));
        let error = Settings::load(&sources).expect_err("an unacknowledged wildcard bind is refused");
        let SettingsError::NotFitToServe { ref refusals } = error else {
            panic!("expected a posture refusal for {environment}, got {error:?}");
        };
        assert!(
            refusals
                .iter()
                .any(|r| matches!(*r, NotFitToServe::BindsBeyondLoopback { .. })),
            "{environment}: {refusals:?}"
        );
    }
}

#[test]
fn an_acknowledged_non_loopback_bind_still_needs_a_token() {
    // The two controls are separate questions - "did you mean to publish this" and "who may reach
    // it" - so answering only the first is still a refusal.
    let sources = Sources::defaults(Environment::Development)
        .with_overlay("server:\n  host: \"0.0.0.0\"\nsecurity:\n  expose_beyond_loopback: true\n");
    let error = Settings::load(&sources).expect_err("an off-host bind with no token is refused");
    let SettingsError::NotFitToServe { ref refusals } = error else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(
        *refusals,
        vec![NotFitToServe::AccessTokenRequired {
            because: "this service is bound where other hosts can reach it"
        }]
    );
}

#[test]
fn both_controls_together_start_an_off_host_development_service() {
    // The other side of the two refusals above, so they cannot be satisfied by a rule that
    // refuses everything.
    let sources = Sources::defaults(Environment::Development).with_overlay(format!(
        "server:\n  host: \"0.0.0.0\"\nsecurity:\n  access_token: \"{TOKEN}\"\n  expose_beyond_loopback: true\n"
    ));
    let settings = Settings::load(&sources).expect("acknowledged and tokenised is servable");
    assert!(!settings.server().bind().is_loopback());
    assert!(settings.security().access_token().is_some());
}

#[test]
fn production_refuses_to_start_with_rate_limiting_switched_off() {
    let sources = Sources::defaults(Environment::Production)
        .with_overlay(format!("{}rate_limit:\n  enabled: false\n", production_overlay()));
    let error = Settings::load(&sources).expect_err("production with no limiter is refused");
    let SettingsError::NotFitToServe { ref refusals } = error else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(*refusals, vec![NotFitToServe::RateLimitingDisabledInProduction]);
}

#[test]
fn production_refuses_an_ephemeral_port() {
    let sources = Sources::defaults(Environment::Production).with_overlay(format!(
        "server:\n  host: \"0.0.0.0\"\n  port: 0\nsecurity:\n  access_token: \"{TOKEN}\"\n  \
         expose_beyond_loopback: true\n"
    ));
    let error = Settings::load(&sources).expect_err("production on a kernel-chosen port is refused");
    let SettingsError::NotFitToServe { ref refusals } = error else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(*refusals, vec![NotFitToServe::EphemeralPortInProduction]);
}

#[test]
fn a_variable_cannot_switch_a_production_control_off_behind_the_files_back() {
    // THE reason the refusals read the loaded value rather than a file. The variable layer is
    // applied last, so a manifest that sets this beats `production.yaml` - and a check that read
    // the file would have passed while the process ran without a limiter.
    let sources = Sources::defaults(Environment::Production)
        .with_overlay(production_overlay())
        .with_variables(variables(&[("SUTURA__RATE_LIMIT__ENABLED", "false")]));
    assert!(matches!(Settings::load(&sources), Err(SettingsError::NotFitToServe { .. })));
}

#[test]
fn every_refusal_is_reported_rather_than_only_the_first() {
    // A fix-and-restart loop that surfaces one problem at a time is how a deployment takes four
    // restarts to discover three mistakes.
    let sources = Sources::defaults(Environment::Production)
        .with_overlay("server:\n  host: \"0.0.0.0\"\n  port: 0\nrate_limit:\n  enabled: false\n");
    let error = Settings::load(&sources).expect_err("a wholly unsafe production config is refused");
    let SettingsError::NotFitToServe { ref refusals } = error else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(refusals.len(), 4, "{refusals:?}");
}

#[test]
fn a_production_deployment_that_answers_every_control_starts() {
    // Without this, every assertion above is satisfiable by refusing everything.
    let settings = Settings::load(&Sources::defaults(Environment::Production).with_overlay(production_overlay()))
        .expect("a production posture that answers every control is servable");
    assert!(settings.environment().is_production());
    assert!(settings.refusals().is_empty());
}

// ----------------------------------------------- the environment-driven split ----

#[test]
fn the_log_format_and_the_documentation_surface_follow_the_environment() {
    let development = Settings::load(&Sources::defaults(Environment::Development)).expect("development loads");
    assert_eq!(development.telemetry().format(), LogFormat::Pretty);
    assert!(development.api().docs_enabled());
    assert!(!development.telemetry().format_was_explicit());
    assert!(!development.api().docs_were_explicit());

    let production =
        Settings::load(&Sources::defaults(Environment::Production).with_overlay(production_overlay())).expect("production loads");
    assert_eq!(production.telemetry().format(), LogFormat::Bunyan);
    assert!(!production.api().docs_enabled());
}

#[test]
fn an_explicit_format_overrides_the_environment_and_is_recorded_as_explicit() {
    // The `explicit` flag is what lets the startup log tell "somebody chose this" from "nobody
    // did", which are worth different words when the value is a surprise.
    let sources = Sources::defaults(Environment::Production).with_overlay(format!(
        "{}telemetry:\n  format: pretty\napi:\n  docs: true\n",
        production_overlay()
    ));
    let settings = Settings::load(&sources).expect("an explicit override loads");
    assert_eq!(settings.telemetry().format(), LogFormat::Pretty);
    assert!(settings.telemetry().format_was_explicit());
    assert!(settings.api().docs_enabled());
    assert!(settings.api().docs_were_explicit());
}

// ------------------------------------------------------------------ secrets ----

#[test]
fn the_whole_settings_tree_can_be_logged_without_printing_the_token() {
    // The startup log prints this tree with `Debug`. The assertion belongs here as well as on
    // `SecuritySettings`, because it is the outermost type a log call will actually be given.
    let settings =
        Settings::load(&Sources::defaults(Environment::Production).with_overlay(production_overlay())).expect("production loads");
    let rendered = format!("{settings:?}");
    assert!(!rendered.contains(TOKEN), "{rendered}");
    assert!(rendered.contains("REDACTED"), "{rendered}");
}
