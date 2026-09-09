//! What the layering does, and what the posture refuses.
//!
//! Hermetic in the process environment: no test here reads a `SUTURA__*` variable, because
//! [`Sources::defaults`] supplies an empty variable map that replaces the real one, so a variable in
//! a developer's shell cannot change a verdict.
//!
//! **The configuration-layer tests do read a disk**, and cannot not: what they assert
//! is which files were found, which is not a question a source with no filesystem can be asked. They
//! write into a scratch directory named after the process and remove it again; the variable layer
//! stays empty, because [`Sources::with_directory`] does not reach for the process environment.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::{Environment, NotFitToServe, Settings, SettingsError, Sources};
use crate::proxy::ClientAddressSource;
use crate::runtime::WorkingSetCeiling;
use crate::security::TlsTermination;
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
         tls_termination: \"ingress\"\n"
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
    assert_eq!(settings.security().tls_termination(), TlsTermination::None);
    // The limiter is OFF here, and that is the environment and not the file: `defaults.yaml` no
    // longer names `rate_limit.enabled` at all. Production defaults the other way -
    // `the_limiter_follows_the_environment_when_nobody_writes_it_down` asserts both arms.
    assert!(!settings.rate_limit().enabled());
    assert!(!settings.rate_limit().enabled_was_explicit());
    // The keying default, and the reason it is the safe one: no header is believed, because no hop
    // is named.
    assert_eq!(settings.rate_limit().client_address(), ClientAddressSource::Peer);
    assert!(settings.rate_limit().trusted_proxies().is_empty());
    assert!(settings.server().tls().is_none());
    assert!(settings.refusals().is_empty(), "the peer-only posture refuses nothing");
}

#[test]
fn the_defaults_alone_do_not_start_a_production_deployment() {
    // The single most important assertion in this file: shipping the defaults to production is a
    // refusal, not a permissive service. Two refusals, because two separate controls are missing.
    let error =
        Settings::load(&Sources::defaults(Environment::Production)).expect_err("the defaults are not a production posture");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
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
        // `true` and not `false`: in development `false` is now the environment default, so
        // asserting it would pass whether the variable was read or dropped on the floor.
        ("SUTURA__RATE_LIMIT__ENABLED", "true"),
    ]));
    let settings = Settings::load(&sources).expect("string-valued numbers coerce");
    assert_eq!(settings.server().request_timeout().seconds(), 45);
    assert_eq!(settings.server().max_body().bytes(), 2048);
    assert!(settings.rate_limit().enabled());
}

#[test]
fn a_misspelled_key_is_an_error_and_not_a_silently_ignored_override() {
    // `deny_unknown_fields` is what makes this an error. Without it the service runs on the
    // default the operator believed they had changed, and nothing anywhere says so.
    let sources = Sources::defaults(Environment::Development).with_overlay("server:\n  prot: 9000\n");
    let error = Settings::load(&sources).expect_err("a misspelled key is not a setting");
    assert!(matches!(*error.reason(), SettingsError::Source { .. }), "{error:?}");
    let rendered = format!("{:?}", core::error::Error::source(&error));
    assert!(rendered.contains("prot"), "the error should name the key: {rendered}");
}

#[test]
fn a_stray_variable_under_the_prefix_is_an_error() {
    // The same protection on the variable layer. A typo in a deployment manifest is otherwise a
    // variable nothing reads.
    let sources = Sources::defaults(Environment::Development).with_variables(variables(&[("SUTURA__SERVER__PROT", "9000")]));
    assert!(Settings::load(&sources).is_err_and(|error| matches!(error.reason(), SettingsError::Source { .. })));
}

#[test]
fn the_environment_is_not_a_configuration_key() {
    // Deliberate: the environment decides which file is layered, so a layer that could set it
    // would be self-referential. Both spellings are unknown fields rather than settings.
    let sources = Sources::defaults(Environment::Development).with_overlay("environment: production\n");
    assert!(Settings::load(&sources).is_err_and(|error| matches!(error.reason(), SettingsError::Source { .. })));
    let sources = Sources::defaults(Environment::Development).with_variables(variables(&[("SUTURA__ENVIRONMENT", "production")]));
    assert!(Settings::load(&sources).is_err_and(|error| matches!(error.reason(), SettingsError::Source { .. })));
}

#[test]
fn a_per_source_working_set_ceiling_is_refused_at_parse() {
    // 0009 Decision 3, mechanised. The working-set ceiling is QUERY-wide: there is one working set,
    // so a per-source ceiling would be a number with nothing to bound - and the decision is that a
    // source declaration carrying one is REFUSED rather than ignored, because somebody will tune a
    // setting that silently does nothing and believe the result.
    //
    // **What "per source" means today, stated rather than implied.** The working set is QUERY-wide,
    // so it belongs under `runtime` and nowhere else; `server` and `prompt` are map groups that a
    // misplaced key could otherwise be written under, and a `catalogs:` entry - the nearest thing a
    // deployment has to a metadata-source declaration - is checked the same way. The mechanism is
    // `deny_unknown_fields` on every shape in `raw.rs`.
    for group in ["server", "prompt"] {
        let sources =
            Sources::defaults(Environment::Development).with_overlay(format!("{group}:\n  working_set_max_bytes: 2048\n"));
        let error = Settings::load(&sources).expect_err("a ceiling on anything but the runtime group is not a setting");
        assert!(matches!(*error.reason(), SettingsError::Source { .. }), "{group}: {error:?}");
        let rendered = format!("{:?}", core::error::Error::source(&error));
        assert!(
            rendered.contains("working_set_max_bytes"),
            "{group}: the error should name the key: {rendered}"
        );
    }
    // A catalog entry is where a per-metadata-source ceiling would now live, and it is refused there
    // too, naming the key rather than silently ignoring it.
    let sources = Sources::defaults(Environment::Development).with_overlay(
        "catalogs:\n  - name: model\n    kind: markdown\n    dir: catalog\n    data_dir: data\n    version: test-1\n    working_set_max_bytes: 2048\n",
    );
    let error = Settings::load(&sources).expect_err("a ceiling on a catalog entry is not a setting");
    assert!(matches!(*error.reason(), SettingsError::Source { .. }), "{error:?}");
    let rendered = format!("{:?}", core::error::Error::source(&error));
    assert!(
        rendered.contains("working_set_max_bytes"),
        "the error should name the key: {rendered}"
    );
    // And the one place it IS a setting still is, so this test cannot pass by the key being unknown
    // everywhere.
    let sources = Sources::defaults(Environment::Development).with_overlay("runtime:\n  working_set_max_bytes: 2048\n");
    let settings = Settings::load(&sources).expect("the runtime group is where the ceiling lives");
    assert_eq!(settings.runtime().working_set().bytes().get(), 2048);
}

#[test]
fn the_embedded_default_ceiling_is_the_number_the_type_declares() {
    // Two places hold this number - `defaults.yaml` and `WorkingSetCeiling::DEFAULT_BYTES` - and one
    // of them is what a composition root with no settings to read uses. Asserted rather than trusted,
    // because a drift between them would mean the command-line tool and the service bound the same
    // engine differently while both looked configured.
    let settings = Settings::load(&Sources::defaults(Environment::Development)).expect("the defaults load");
    let ceiling = settings.runtime().working_set();
    assert_eq!(
        u64::try_from(ceiling.bytes().get()).expect("a gibibyte fits a u64"),
        WorkingSetCeiling::DEFAULT_BYTES
    );
    // A gibibyte, spelled out, so a change to the constant is visible in the diff of this test
    // rather than only in the constant.
    assert_eq!(WorkingSetCeiling::DEFAULT_BYTES, 1024 * 1024 * 1024);
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
        (
            "catalogs:\n  - name: model\n    kind: markdown\n    dir: catalog\n    data_dir: data\n    version: \"\"\n",
            |e| matches!(*e, SettingsError::Version { .. }),
        ),
        (
            "catalogs:\n  - name: model\n    kind: markdown\n    dir: \"\"\n    data_dir: data\n    version: test-1\n",
            |e| matches!(*e, SettingsError::Catalog { .. }),
        ),
    ];
    for &(overlay, is_expected) in cases {
        let sources = Sources::defaults(Environment::Development).with_overlay(overlay);
        let error = Settings::load(&sources).unwrap_err();
        assert!(is_expected(error.reason()), "{overlay:?} produced {error:?}");
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
    assert!(
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(past))
            .is_err_and(|error| matches!(error.reason(), SettingsError::Bound { .. }))
    );
}

// --------------------------------------------------------- posture refusals ----

#[test]
fn a_non_loopback_bind_needs_a_tls_termination_declaration_in_every_environment() {
    // Not a production-only rule, and that is deliberate: binding the wildcard is the single
    // change that turns a local tool into a network service, and a laptop on a shared network is
    // where that is least expected.
    //
    // What is refused is the SILENCE, not the bind. The test below binds the same wildcard with a
    // declaration and starts, because that is the normal deployment.
    for environment in [Environment::Development, Environment::Test, Environment::Production] {
        let sources = Sources::defaults(environment).with_overlay(format!(
            "server:\n  host: \"0.0.0.0\"\nsecurity:\n  access_token: \"{TOKEN}\"\n"
        ));
        let error = Settings::load(&sources).expect_err("an undeclared wildcard bind is refused");
        let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
            panic!("expected a posture refusal for {environment}, got {error:?}");
        };
        assert!(
            refusals
                .iter()
                .any(|r| matches!(*r, NotFitToServe::TlsTerminationUndeclared { .. })),
            "{environment}: {refusals:?}"
        );
    }
}

#[test]
fn a_declared_non_loopback_bind_still_needs_a_token() {
    // The two controls are separate questions - "what protects the path to this" and "who may
    // reach it" - so answering only the first is still a refusal.
    let sources = Sources::defaults(Environment::Development)
        .with_overlay("server:\n  host: \"0.0.0.0\"\nsecurity:\n  tls_termination: \"ingress\"\n");
    let error = Settings::load(&sources).expect_err("an off-host bind with no token is refused");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(
        *refusals,
        vec![NotFitToServe::AccessTokenRequired {
            because: "this service is bound where other hosts can reach it and no inbound identity is configured"
        }]
    );
}

#[test]
fn both_controls_together_start_an_off_host_development_service() {
    // The other side of the two refusals above, so they cannot be satisfied by a rule that
    // refuses everything. This is also the SHAPE OF THE NORMAL DEPLOYMENT: a plaintext listener on
    // every interface, with something in front of it that terminates TLS.
    for declared in ["sidecar", "ingress"] {
        let sources = Sources::defaults(Environment::Development).with_overlay(format!(
            "server:\n  host: \"0.0.0.0\"\nsecurity:\n  access_token: \"{TOKEN}\"\n  tls_termination: \"{declared}\"\n"
        ));
        let settings = Settings::load(&sources).expect("a declared and tokenised off-host bind is servable");
        assert!(!settings.server().bind().is_loopback());
        assert!(settings.security().access_token().is_some());
        assert!(settings.security().tls_termination().is_declared());
        // And what it declares is a terminator somewhere else, not encryption here.
        assert!(!settings.security().tls_termination().terminates_here());
    }
}

#[test]
fn a_forwarded_header_is_not_believed_without_a_named_hop() {
    // The refusal that keeps the limiter from being worse than useless. With no trusted hop, the
    // header is a value every caller writes, so every bucket is theirs to choose.
    let sources = Sources::defaults(Environment::Development).with_overlay("rate_limit:\n  client_address: \"forwarded\"\n");
    let error = Settings::load(&sources).expect_err("a believed header with no named hop is refused");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(*refusals, vec![NotFitToServe::ForwardedWithoutTrustedProxies]);
}

#[test]
fn a_trusted_proxy_list_that_nothing_reads_is_refused() {
    // The other direction, and it is refused for the reason every unknown key here is an error: a
    // list that has no effect reads as a control that is in place.
    let sources = Sources::defaults(Environment::Development)
        .with_overlay("rate_limit:\n  trusted_proxies:\n    - \"10.0.0.0/8\"\n    - \"10.1.0.0/16\"\n");
    let error = Settings::load(&sources).expect_err("a list nothing reads is refused");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(*refusals, vec![NotFitToServe::TrustedProxiesWithoutForwarding { count: 2 }]);
}

#[test]
fn forwarded_keying_with_a_named_hop_loads() {
    // The positive case, without which the two refusals above are satisfied by refusing every
    // forwarded configuration there is.
    let sources = Sources::defaults(Environment::Development)
        .with_overlay("rate_limit:\n  client_address: \"forwarded\"\n  trusted_proxies:\n    - \"10.0.0.0/8\"\n");
    let settings = Settings::load(&sources).expect("a named hop and a header source load");
    assert_eq!(settings.rate_limit().client_address(), ClientAddressSource::Forwarded);
    assert!(
        settings
            .rate_limit()
            .trusted_proxies()
            .trusts("10.1.2.3".parse().expect("a test address is an address"))
    );
}

#[test]
fn in_process_tls_needs_material_and_material_needs_in_process_tls() {
    // Both halves of one question. Either mismatch is a listener that is not what the file says: a
    // declaration with no key cannot serve TLS, and a key with no declaration is never read.
    let sources = Sources::defaults(Environment::Development).with_overlay(format!(
        "server:\n  host: \"0.0.0.0\"\nsecurity:\n  access_token: \"{TOKEN}\"\n  tls_termination: \"in-process\"\n"
    ));
    let error = Settings::load(&sources).expect_err("in-process TLS with no material is refused");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert!(refusals.contains(&NotFitToServe::InProcessTlsWithoutMaterial), "{refusals:?}");

    let sources = Sources::defaults(Environment::Development)
        .with_overlay("server:\n  tls_certificate: \"/tls/chain.pem\"\n  tls_key: \"/tls/key.pem\"\n");
    let error = Settings::load(&sources).expect_err("material nothing reads is refused");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(
        *refusals,
        vec![NotFitToServe::TlsMaterialWithoutInProcessTermination { declared: "none" }]
    );
}

#[test]
fn in_process_tls_is_refused_when_the_binary_cannot_do_it() {
    // The failure mode that must never be quiet: a binary with no TLS implementation linked in,
    // asked to terminate TLS. The alternative to refusing is plaintext on a port an operator
    // configured to be encrypted.
    //
    // Both arms are asserted, because which one holds is a property of the BUILD rather than of the
    // configuration - and a test that only checked one would pass for the wrong reason in the other
    // feature state.
    let sources = Sources::defaults(Environment::Development).with_overlay(format!(
        "server:\n  tls_certificate: \"/tls/chain.pem\"\n  tls_key: \"/tls/key.pem\"\nsecurity:\n  \
         access_token: \"{TOKEN}\"\n  tls_termination: \"in-process\"\n"
    ));
    let loaded = Settings::load(&sources);
    if cfg!(feature = "tls") {
        let settings = loaded.expect("with the feature on, declared and provisioned TLS loads");
        assert!(settings.security().tls_termination().terminates_here());
        assert!(settings.server().tls().is_some());
    } else {
        let error = loaded.expect_err("without the feature there is nothing to terminate TLS with");
        let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
            panic!("expected a posture refusal, got {error:?}");
        };
        assert_eq!(*refusals, vec![NotFitToServe::InProcessTlsNotCompiledIn]);
    }
}

#[test]
fn production_refuses_to_start_with_rate_limiting_switched_off() {
    let sources = Sources::defaults(Environment::Production)
        .with_overlay(format!("{}rate_limit:\n  enabled: false\n", production_overlay()));
    let error = Settings::load(&sources).expect_err("production with no limiter is refused");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(*refusals, vec![NotFitToServe::RateLimitingDisabledInProduction]);
}

#[test]
fn production_refuses_an_ephemeral_port() {
    let sources = Sources::defaults(Environment::Production).with_overlay(format!(
        "server:\n  host: \"0.0.0.0\"\n  port: 0\nsecurity:\n  access_token: \"{TOKEN}\"\n  \
         tls_termination: \"ingress\"\n"
    ));
    let error = Settings::load(&sources).expect_err("production on a kernel-chosen port is refused");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
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
    assert!(Settings::load(&sources).is_err_and(|error| matches!(error.reason(), SettingsError::NotFitToServe { .. })));
}

#[test]
fn every_refusal_is_reported_rather_than_only_the_first() {
    // A fix-and-restart loop that surfaces one problem at a time is how a deployment takes four
    // restarts to discover three mistakes.
    let sources = Sources::defaults(Environment::Production)
        .with_overlay("server:\n  host: \"0.0.0.0\"\n  port: 0\nrate_limit:\n  enabled: false\n");
    let error = Settings::load(&sources).expect_err("a wholly unsafe production config is refused");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
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
    assert!(
        settings.refusals().is_empty(),
        "a servable production posture refuses nothing"
    );
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

// -------------------------------------------- the limiter follows the environment ----

#[test]
fn the_limiter_follows_the_environment_when_nobody_writes_it_down() {
    // The split, asserted at the outermost layer: off on a laptop and in a test, on in production,
    // with nothing in any file saying so. The sibling assertion for `telemetry.format` and
    // `api.docs` is `the_log_format_and_the_documentation_surface_follow_the_environment`; this is
    // the third key that works that way and the first whose permissive answer is the OFF one.
    for permissive in [Environment::Development, Environment::Test] {
        let settings = Settings::load(&Sources::defaults(permissive)).expect("a permissive environment loads");
        assert!(!settings.rate_limit().enabled(), "{permissive} defaulted the limiter on");
        assert!(
            !settings.rate_limit().enabled_was_explicit(),
            "{permissive} recorded a defaulted switch as chosen"
        );
    }
    let production =
        Settings::load(&Sources::defaults(Environment::Production).with_overlay(production_overlay())).expect("production loads");
    assert!(production.rate_limit().enabled(), "production defaulted the limiter off");
    assert!(!production.rate_limit().enabled_was_explicit());
}

#[test]
fn an_explicit_switch_in_development_is_honoured_and_recorded_as_explicit() {
    // The developer who is testing the limiter itself. The environment default is a default and not
    // a ceiling.
    //
    // The first assertion is what makes the second one mean something: without it, a test that only
    // checked `enabled()` after writing `enabled: true` would pass just as well against a flat
    // default of `true`, which is the behaviour this change replaces.
    let bare = Settings::load(&Sources::defaults(Environment::Development)).expect("development loads");
    assert!(
        !bare.rate_limit().enabled(),
        "development is the environment that defaults off"
    );

    let sources = Sources::defaults(Environment::Development).with_overlay("rate_limit:\n  enabled: true\n");
    let settings = Settings::load(&sources).expect("an explicit switch loads");
    assert!(settings.rate_limit().enabled());
    // And the flag, which is what lets the startup log say a person chose this rather than that
    // nobody did. `true` in development looks identical either way once it is stored.
    assert!(settings.rate_limit().enabled_was_explicit());
}

#[test]
fn production_still_refuses_an_explicitly_disabled_limiter() {
    // THE assertion this change must not weaken, and the one test here that passed before the
    // change as well as after: default-off in development is a convenience, silently-off in
    // production is a security regression, and conflating the two is how the second arrives
    // disguised as the first. A guard that only started passing with the change would be a guard
    // that had not been in place.
    //
    // Written down as `false`, so this is the EXPLICIT case and not a defaulted one - the refusal
    // reads the loaded value and does not care which layer produced it, which is exactly the
    // property that makes an environment-derived default safe to add underneath it.
    let sources = Sources::defaults(Environment::Production)
        .with_overlay(format!("{}rate_limit:\n  enabled: false\n", production_overlay()));
    let error = Settings::load(&sources).expect_err("an explicit false in production is refused");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(*refusals, vec![NotFitToServe::RateLimitingDisabledInProduction]);
}

#[test]
fn a_variable_overrides_the_environment_default_in_both_directions() {
    // On where the environment says off. Both halves asserted from the same sources, because a test
    // that only checked the value AFTER the variable cannot tell an override from a default that
    // already agreed with it.
    let bare = Settings::load(&Sources::defaults(Environment::Development)).expect("development loads");
    assert!(
        !bare.rate_limit().enabled(),
        "development is the environment that defaults off"
    );
    let sources =
        Sources::defaults(Environment::Development).with_variables(variables(&[("SUTURA__RATE_LIMIT__ENABLED", "true")]));
    let settings = Settings::load(&sources).expect("a variable switching the limiter on loads");
    assert!(settings.rate_limit().enabled());
    assert!(settings.rate_limit().enabled_was_explicit());

    // And off where it says on - which in production is a refusal rather than a load, because that
    // is the one direction this crate does not let an operator take quietly. The variable layer is
    // applied last, so this is also the layer a deployment manifest would use.
    let sources = Sources::defaults(Environment::Production)
        .with_overlay(production_overlay())
        .with_variables(variables(&[("SUTURA__RATE_LIMIT__ENABLED", "false")]));
    let error = Settings::load(&sources).expect_err("a variable cannot switch the limiter off in production");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(*refusals, vec![NotFitToServe::RateLimitingDisabledInProduction]);
}

// ------------------------------------------------------ configuration layers ----

/// A scratch configuration directory, emptied first so a previous run cannot decide this one.
///
/// Named after the process and the case, which is the shape the catalog adapter's own filesystem
/// tests use: `CARGO_TARGET_TMPDIR` is defined for an integration target and not for a unit test
/// under `src/`.
fn scratch(case: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sutura-config-{case}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
    dir
}

#[test]
fn the_files_a_deployment_is_running_on_are_carried_out_of_the_load() {
    // THE BUG. `read` layered `base.yaml` and `<environment>.yaml`, both `.required(false)`, and
    // threw away which of them existed - so nothing downstream could say where a value came from.
    // Both files here, so the ORDER is asserted too: `base` first, then the environment's, which is
    // the precedence the doc comment on `Settings::load` calls the contract.
    let dir = scratch("both-layers");
    std::fs::write(dir.join("base.yaml"), "server:\n  port: 9001\n").expect("a scratch file is writable");
    std::fs::write(dir.join("development.yaml"), "server:\n  port: 9002\n").expect("a scratch file is writable");

    let settings = Settings::load(&Sources::defaults(Environment::Development).with_directory(dir.clone()))
        .expect("two layers over the defaults load");

    assert_eq!(
        settings.layers().files(),
        [dir.join("base.yaml"), dir.join("development.yaml")],
        "the layers are not what was on disk, in order"
    );
    // The later layer won, which is what makes the list an explanation of the value beside it rather
    // than a list of files that happen to be there.
    assert_eq!(settings.server().bind().to_string().rsplit(':').next(), Some("9002"));
    assert!(!settings.layers().is_empty());
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn only_the_files_that_are_there_are_reported() {
    // The half that makes the list worth reading: a directory with one of the two layers reports one,
    // not two. `.required(false)` means the absent one is not an error, and the old code could not
    // tell an operator which of the two that was.
    let dir = scratch("one-layer");
    std::fs::write(dir.join("base.yaml"), "server:\n  port: 9003\n").expect("a scratch file is writable");

    let settings =
        Settings::load(&Sources::defaults(Environment::Development).with_directory(dir.clone())).expect("one layer loads");

    assert_eq!(settings.layers().files(), [dir.join("base.yaml")]);
    assert_eq!(settings.layers().to_string(), dir.join("base.yaml").display().to_string());
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn a_deployment_on_embedded_defaults_says_so_and_a_wrong_directory_looks_the_same() {
    // The line that was missing from the startup log, and the honest limit on it. A configuration
    // directory that is not there resolves EXACTLY like no directory at all - that is what
    // `.required(false)` means - so what this reports is that no file contributed, not which of the
    // two situations produced it. A mistyped path is then visible as the absence of the file the
    // operator expected to see named, which is the whole of what the log can offer.
    let no_directory = Settings::load(&Sources::defaults(Environment::Development)).expect("the defaults load");
    assert!(no_directory.layers().is_empty());
    assert_eq!(no_directory.layers().to_string(), "embedded defaults only");

    let mistyped = Settings::load(
        &Sources::defaults(Environment::Development)
            .with_directory(std::env::temp_dir().join(format!("sutura-config-no-such-dir-{}", std::process::id()))),
    )
    .expect("a directory that is not there is not an error - that is the posture being reported");
    assert_eq!(mistyped.layers(), no_directory.layers());
}

#[test]
fn failed_loads_retain_the_observed_file_layers_in_order() {
    let dir = scratch("failed-layers");
    let base = dir.join("base.yaml");
    let production = dir.join("production.yaml");
    std::fs::write(&production, "telemetry:\n  service_name: \"layers\"\n").expect("the later layer is writable");
    let mut observations = Vec::new();
    for (case, yaml) in [
        ("source", "server:\n  prot: 8081\n"),
        ("typed", "server:\n  host: \"not an address\"\n"),
        ("posture", "server:\n  port: 8081\n"),
    ] {
        std::fs::write(&base, yaml).expect("the base layer is writable");
        match Settings::load(&Sources::defaults(Environment::Production).with_directory(dir.clone())) {
            Ok(_) => observations.push((case, false, String::from("the load did not refuse"))),
            Err(error) => {
                let correct_stage = matches!(
                    (case, error.reason()),
                    ("source", SettingsError::Source { .. })
                        | ("typed", SettingsError::Bind { .. })
                        | ("posture", SettingsError::NotFitToServe { .. })
                );
                observations.push((case, correct_stage, error.to_string()));
            }
        }
    }
    std::fs::remove_dir_all(&dir).expect("the fixture directory is removed before assertions");
    let base = base.display().to_string();
    let production = production.display().to_string();
    let missing: Vec<_> = observations
        .iter()
        .filter(|(_, correct_stage, message)| {
            !correct_stage
                || !matches!((message.find(&base), message.find(&production)), (Some(first), Some(second)) if first < second)
        })
        .collect();
    assert!(
        missing.is_empty(),
        "failed loads lost their observed layers or refusal stage: {missing:?}"
    );
}

#[test]
fn a_failed_load_without_files_names_the_embedded_defaults() {
    let parent = scratch("missing-failed-layers");
    let missing = parent.join("missing");
    let without_directory = Settings::load(&Sources::defaults(Environment::Production));
    let wrong_directory = Settings::load(&Sources::defaults(Environment::Production).with_directory(missing));
    std::fs::remove_dir_all(&parent).expect("the fixture directory is removed before assertions");
    let without_directory = without_directory.expect_err("production needs a credential");
    let wrong_directory = wrong_directory.expect_err("a missing directory leaves the same production refusal");
    assert_eq!(without_directory.to_string(), wrong_directory.to_string());
    assert!(
        wrong_directory.to_string().contains("embedded defaults only"),
        "the refused load hid that no file layer was found: {wrong_directory}"
    );
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

// ------------------------------------------------------------------ sources and the mode ----

/// One source entry, so a test changes exactly one thing about it.
///
/// The path is absolute because a relative one is refused at parse - that refusal has its own test in
/// [`crate::sources`] - and this helper exists to reach the *deployment-level* refusals underneath it.
fn source_overlay(posture: &str, extra: &str) -> String {
    let workload = if posture == "impersonation-at-source" {
        "    workload_identity:\n      audience: \"//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso\"\n      scope: \"https://www.googleapis.com/auth/bigquery.readonly\"\n"
    } else {
        ""
    };
    format!(
        "sources:\n  local:\n    kind: \"files\"\n    data_dir: \"/srv/sutura/data\"\n    posture: \"{posture}\"\n{workload}{extra}"
    )
}

#[test]
fn a_shared_source_in_a_multi_user_deployment_without_an_acknowledgement_is_not_fit_to_serve() {
    // THE refusal the shared posture exists to be caught by. The wrong outcome here is not a failure,
    // it is an answer: rows a caller's own permissions never filtered, served under a certified metric
    // name. Sutura declares no data sensitivity so it cannot tell whether that was fine; what it can do
    // is make the posture impossible to arrive at by accident and impossible to arrive at in silence.
    //
    // Asserted through `Settings::refusals` as well as through `Settings::load`, because that method is
    // public and side-effect-free precisely so a test can - and because the LIST is the contract: a
    // deployment with three unacknowledged sources should learn about all three on one restart.
    let sources = Sources::defaults(Environment::Development).with_overlay(format!(
        "security:\n  identity: \"multi-user\"\n{}",
        source_overlay("shared-service-user", "")
    ));
    let error = Settings::load(&sources).expect_err("a shared source nobody acknowledged is not fit to serve");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(
        *refusals,
        vec![NotFitToServe::SharedSourceNotAcknowledged {
            alias: String::from("local")
        }]
    );
    // The message names the entry to change and says there is no global acknowledgement, because an
    // operator who reads "acknowledge it" and writes one key at the top of the file has not.
    let rendered = refusals.first().expect("one refusal").to_string();
    assert!(rendered.contains("sources.local.acknowledged_because"), "{rendered}");
    assert!(rendered.contains("no global acknowledgement"), "{rendered}");

    // And with the acknowledgement on that source's own entry it serves - so the refusal is not
    // satisfied by refusing every shared source.
    let acknowledged = Sources::defaults(Environment::Development).with_overlay(format!(
        "security:\n  identity: \"multi-user\"\n{}",
        source_overlay(
            "shared-service-user",
            "    acknowledged_because: \"a read-only reporting replica every caller may see\"\n"
        )
    ));
    let settings = Settings::load(&acknowledged).expect("an acknowledged shared source is servable");
    assert!(settings.refusals().is_empty(), "{:?}", settings.refusals());
    assert_eq!(
        settings
            .sources()
            .get(&sutura_domain::model::SourceName::parse("local").expect("a name"))
            .and_then(crate::sources::ConfiguredSource::posture)
            .map(sutura_domain::source::SourcePosture::as_str),
        Some("shared-service-user")
    );
}

#[test]
fn a_single_user_deployment_needs_no_per_source_acknowledgement_and_nor_does_an_impersonating_source() {
    // The other two arms of the same rule, so the refusal above cannot be a check that fires on every
    // configured source.
    //
    // Single-user is a FIRST-CLASS deployment: the one user reads all, by design, and the configured
    // credential is that user's own - so the mode's own declaration is the acknowledgement, and
    // `examples/single-player` does not have to restate the obvious per source.
    let single = Sources::defaults(Environment::Development).with_overlay(format!(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"one operator, their own files\"\n{}",
        source_overlay("shared-service-user", "")
    ));
    let settings = Settings::load(&single).expect("single-user mode supplies the witness");
    assert!(settings.refusals().is_empty(), "{:?}", settings.refusals());

    // And an impersonating source is never the subject of this refusal: there is no shared identity to
    // acknowledge.
    let impersonating = Sources::defaults(Environment::Development).with_overlay(format!(
        "security:\n  identity: \"multi-user\"\n{}",
        source_overlay("impersonation-at-source", "")
    ));
    let settings = Settings::load(&impersonating).expect("an impersonating source needs no acknowledgement");
    assert!(settings.refusals().is_empty(), "{:?}", settings.refusals());
}

#[test]
fn a_deployment_that_configures_a_source_and_declares_no_mode_does_not_start() {
    // The mode has no default, and this is the refusal that makes that true rather than a sentence in a
    // comment. It is keyed on a source being configured rather than raised unconditionally - the
    // embedded defaults have to keep loading, and a deployment with no source cannot answer anything
    // anyway - so the assertion below has two halves: refused with a source, and silent without one.
    let sources = Sources::defaults(Environment::Development).with_overlay(source_overlay("impersonation-at-source", ""));
    let error = Settings::load(&sources).expect_err("a configured source with no declared mode does not start");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(*refusals, vec![NotFitToServe::DeploymentIdentityUndeclared { count: 1 }]);
    // The message says why no combination of source postures may answer it, because "derive it" is the
    // first thing anybody proposes here and it is unsound in the configuration that most needs it.
    let rendered = refusals.first().expect("one refusal").to_string();
    assert!(rendered.contains("all shared"), "{rendered}");

    // With no source configured there is nothing for a mode to govern, so the defaults still load -
    // which is what keeps every other test in this file, and the `prompt` command, reading a real tree.
    let defaults = Settings::load(&Sources::defaults(Environment::Development)).expect("the defaults load");
    assert!(defaults.sources().is_empty());
    assert!(defaults.security().identity().is_none());
    assert!(defaults.refusals().is_empty(), "the development defaults refuse nothing");
}

#[test]
fn two_sources_can_be_configured_and_each_says_what_it_is() {
    // The Done-when of the source registry, from the configuration side: more than one source, each
    // declaring its own posture, each with its own file location.
    let sources = Sources::defaults(Environment::Development).with_overlay(
        "security:\n  identity: \"multi-user\"\nsources:\n  \
         local:\n    kind: \"files\"\n    data_dir: \"/srv/sutura/local\"\n    posture: \"shared-service-user\"\n    \
         acknowledged_because: \"a directory of CSVs this deployment owns\"\n  \
         warehouse:\n    kind: \"files\"\n    data_dir: \"/srv/sutura/warehouse\"\n    posture: \"impersonation-at-source\"\n    \
         workload_identity:\n      audience: \"//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso\"\n      \
         scope: \"https://www.googleapis.com/auth/bigquery.readonly\"\n    \
         verification_identity: \"sutura_anchor_reader\"\n",
    );
    let settings = Settings::load(&sources).expect("two sources load");
    assert_eq!(settings.sources().count(), 2);
    assert_eq!(
        settings
            .sources()
            .each()
            .map(|(alias, source)| (
                alias.as_str(),
                source.posture().map(sutura_domain::source::SourcePosture::as_str),
                match *source.placement() {
                    crate::sources::placement::SourcePlacement::Files { ref data_dir } => data_dir.to_string_lossy().into_owned(),
                    crate::sources::placement::SourcePlacement::BigQuery { .. } =>
                        panic!("the fixture tree declares files sources"),
                }
            ))
            .collect::<Vec<(&str, Option<&str>, String)>>(),
        vec![
            ("local", Some("shared-service-user"), String::from("/srv/sutura/local")),
            (
                "warehouse",
                Some("impersonation-at-source"),
                String::from("/srv/sutura/warehouse")
            ),
        ]
    );
    // A per-source working-set ceiling is still an unknown key: there is one working set, so a
    // per-source bound would be a number with nothing to bound.
    let with_ceiling = Sources::defaults(Environment::Development).with_overlay(
        "security:\n  identity: \"multi-user\"\nsources:\n  local:\n    kind: \"files\"\n    data_dir: \"/srv/d\"\n    \
         posture: \"impersonation-at-source\"\n    working_set_max_bytes: 2048\n",
    );
    assert!(Settings::load(&with_ceiling).is_err_and(|error| matches!(error.reason(), SettingsError::Source { .. })));
}

/// Reading an inbound-identity declaration. Carved out because this file hit the line limit.
mod inbound;
