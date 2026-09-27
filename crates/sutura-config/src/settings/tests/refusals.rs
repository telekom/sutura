//! Each cell drives an untrusted overlay value through the public `Settings::load` entry point
//! and asserts the typed variant. A file of its own because `settings/tests.rs` is at the
//! thousand-line cap, matching the seam the other sub-modules already use.

use crate::settings::{Environment, Settings, SettingsError, Sources};

/// A development overlay that is otherwise fine, so a test can break exactly one thing.
fn dev_overlay(body: &str) -> String {
    format!("server:\n  host: \"127.0.0.1\"\n  port: 8080\n{body}")
}

#[test]
fn an_invalid_tls_termination_word_is_a_settings_error() {
    let sources =
        Sources::defaults(Environment::Development).with_overlay(dev_overlay("security:\n  tls_termination: \"not-a-place\"\n"));
    let error = Settings::load(&sources).expect_err("an invalid tls_termination word is refused");
    assert!(matches!(error.reason(), SettingsError::TlsTermination { .. }), "{error:?}");
}

#[test]
fn a_zero_credential_cache_capacity_is_a_settings_error() {
    let sources = Sources::defaults(Environment::Development)
        .with_overlay(dev_overlay("security:\n  credential_cache:\n    capacity: 0\n"));
    let error = Settings::load(&sources).expect_err("a zero credential cache capacity is refused");
    assert!(matches!(error.reason(), SettingsError::CredentialCache { .. }), "{error:?}");
}

#[test]
fn a_relative_outbound_anchor_path_is_a_settings_error() {
    let sources = Sources::defaults(Environment::Development).with_overlay(dev_overlay(
        "security:\n  outbound:\n    transport_anchors: \"relative/bundle.pem\"\n",
    ));
    let error = Settings::load(&sources).expect_err("a relative outbound anchor path is refused");
    assert!(matches!(error.reason(), SettingsError::Outbound { .. }), "{error:?}");
}

#[test]
fn an_unusable_audience_mapping_identifier_is_a_settings_error() {
    let sources = Sources::defaults(Environment::Development).with_overlay(dev_overlay(
        "security:\n  audience_mapping:\n    some_group:\n      - \"has spaces!\"\n",
    ));
    let error = Settings::load(&sources).expect_err("a bad audience mapping identifier is refused");
    assert!(matches!(error.reason(), SettingsError::AudienceMapping { .. }), "{error:?}");
}

#[test]
fn half_configured_tls_material_is_a_settings_error() {
    let sources = Sources::defaults(Environment::Development)
        .with_overlay("server:\n  host: \"127.0.0.1\"\n  port: 8080\n  tls_certificate: \"/tls/chain.pem\"\n");
    let error = Settings::load(&sources).expect_err("a half-configured TLS pair is refused");
    assert!(matches!(error.reason(), SettingsError::TlsMaterial { .. }), "{error:?}");
}

#[test]
fn an_unknown_client_address_source_is_a_settings_error() {
    let sources =
        Sources::defaults(Environment::Development).with_overlay(dev_overlay("rate_limit:\n  client_address: \"unknown\"\n"));
    let error = Settings::load(&sources).expect_err("an unknown client address source is refused");
    assert!(matches!(error.reason(), SettingsError::ClientAddress { .. }), "{error:?}");
}

#[test]
fn a_bad_trusted_proxy_entry_is_a_settings_error() {
    let sources = Sources::defaults(Environment::Development)
        .with_overlay(dev_overlay("rate_limit:\n  trusted_proxies:\n    - \"not-an-address\"\n"));
    let error = Settings::load(&sources).expect_err("a bad trusted proxy entry is refused");
    assert!(matches!(error.reason(), SettingsError::TrustedProxy { .. }), "{error:?}");
}

#[test]
fn an_unknown_catalog_kind_is_a_settings_error() {
    let sources = Sources::defaults(Environment::Development).with_overlay(dev_overlay(
        "catalogs:\n  - name: model\n    kind: \"no-such-kind\"\n    dir: /d\n    data_dir: /d\n    version: v1\n",
    ));
    let error = Settings::load(&sources).expect_err("an unknown catalog kind is refused");
    assert!(matches!(error.reason(), SettingsError::CatalogKind { .. }), "{error:?}");
}

#[test]
fn a_bad_catalog_name_is_a_settings_error() {
    let sources = Sources::defaults(Environment::Development).with_overlay(dev_overlay(
        "catalogs:\n  - name: \"has space\"\n    kind: markdown\n    dir: /d\n    data_dir: /d\n    version: v1\n",
    ));
    let error = Settings::load(&sources).expect_err("a bad catalog name is refused");
    assert!(matches!(error.reason(), SettingsError::CatalogName { .. }), "{error:?}");
}

#[test]
fn an_unknown_catalog_prose_word_is_a_settings_error() {
    let sources =
        Sources::defaults(Environment::Development).with_overlay(dev_overlay("prompt:\n  catalog_prose: \"verbatim\"\n"));
    let error = Settings::load(&sources).expect_err("an unknown catalog prose word is refused");
    assert!(matches!(error.reason(), SettingsError::CatalogProse { .. }), "{error:?}");
}

#[test]
fn an_unknown_identity_mode_is_a_settings_error() {
    let sources =
        Sources::defaults(Environment::Development).with_overlay(dev_overlay("security:\n  identity: \"no-such-mode\"\n"));
    let error = Settings::load(&sources).expect_err("an unknown identity mode is refused");
    assert!(matches!(error.reason(), SettingsError::Identity { .. }), "{error:?}");
}

#[test]
fn a_bad_source_kind_is_a_settings_error() {
    let sources = Sources::defaults(Environment::Development).with_overlay(dev_overlay(
        "sources:\n  local:\n    kind: \"no-such-kind\"\n    data_dir: /srv/sutura/data\n    posture: shared-service-user\n",
    ));
    let error = Settings::load(&sources).expect_err("a bad source kind is refused");
    assert!(matches!(error.reason(), SettingsError::Sources { .. }), "{error:?}");
}
