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
fn a_credential_cache_block_is_refused_as_an_unknown_key() {
    // `github.com/telekom/sutura#1259`: `security.credential_cache` configured a cache nothing
    // builds, so the key is gone. A config that still writes it is refused like any other unknown
    // key rather than read as a control that is in place.
    let sources = Sources::defaults(Environment::Development)
        .with_overlay(dev_overlay("security:\n  credential_cache:\n    enabled: true\n"));
    let error = Settings::load(&sources).expect_err("a credential cache block is not a setting");
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("unknown field") && rendered.contains("credential_cache"),
        "the refusal names the key: {rendered}"
    );
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

/// Diagnostics and logs show endpoint URLs without userinfo: the startup log prints the resolved
/// settings before a reader parses its endpoint, so a userinfo endpoint is refused at load, unquoted.
#[test]
fn a_catalog_endpoint_with_userinfo_is_refused_at_load_without_being_quoted() {
    for reader in ["kind: datahub\n    metric_property: m", "kind: openmetadata", "kind: cube"] {
        for endpoint in ["https://svc:s3cret@catalog.example", "https://svc/x:s3cret@catalog.example"] {
            let sources = Sources::defaults(Environment::Development).with_overlay(format!(
                "catalogs:\n  - name: catalog\n    {reader}\n    dir: catalog\n    data_dir: data\n    version: test-1\n    endpoint: {endpoint}\n    token_file: /nowhere/token\n"
            ));
            let error = Settings::load(&sources).expect_err("a userinfo endpoint is refused");
            assert!(matches!(error.reason(), SettingsError::Catalog { .. }), "{error:?}");
            let cause = core::error::Error::source(error.reason()).map_or_default(ToString::to_string);
            assert!(cause.contains("endpoint carries an `@`"), "{cause}");
            let rendered = format!("{error} {error:?} {cause}");
            assert!(!rendered.contains("s3cret"), "{rendered}");
        }
    }
}

/// Every message in the chain of what `Settings::load` refused `overlay` for, joined.
fn refused_for(overlay: &str) -> String {
    let sources = Sources::defaults(Environment::Development).with_overlay(overlay);
    let error = Settings::load(&sources).expect_err("the overlay is refused");
    let mut rendered = Vec::new();
    let mut cause: Option<&dyn core::error::Error> = Some(error.reason());
    while let Some(error) = cause {
        rendered.push(error.to_string());
        cause = error.source();
    }
    rendered.join(" | ")
}

/// An `oracle` source declared with `posture` and `extra`, under a single-user deployment that
/// verifies `direct` callers.
fn oracle_source(posture: &str, extra: &str) -> String {
    format!(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"a test\"\n{}sources:\n  warehouse:\n    \
         kind: \"oracle\"\n    host: \"db.example.com\"\n    port: 2484\n    service_name: \"orcl.example.com\"\n    \
         user: \"sutura\"\n    password_file: \"/run/secrets/oracle-password\"\n    transport_mode: \"verified\"\n    \
         transport_anchors: \"/run/secrets/oracle-ca.pem\"\n    posture: \"{posture}\"\n{extra}",
        super::inbound::DIRECT_INBOUND
    )
}

// The three refusals of `sources.<alias>.caller_session`, read through their messages: each names
// the key, and none repeats an operator's value.

#[test]
fn a_caller_session_on_a_shared_oracle_source_is_refused_at_load() {
    let rendered = refused_for(&oracle_source(
        "shared-service-user",
        "    acknowledged_because: \"a test\"\n    caller_session: \"end-user-context\"\n",
    ));
    assert!(
        rendered.contains(
            "`sources.warehouse.caller_session` is not usable: it is written on a source that is not \
             `impersonation-at-source`"
        ),
        "{rendered}"
    );
}

#[test]
fn a_caller_session_on_another_kind_is_refused_at_load() {
    for (kind, entry) in [
        ("files", "    kind: \"files\"\n    data_dir: \"/srv/sutura/data\"\n"),
        (
            "cube",
            "    kind: \"cube\"\n    endpoint: \"https://cube.example.com\"\n    token_file: \"/etc/sutura/cube-token\"\n",
        ),
    ] {
        let rendered = refused_for(&format!(
            "sources:\n  local:\n{entry}    posture: \"shared-service-user\"\n    acknowledged_because: \"a test\"\n    \
             caller_session: \"end-user-context\"\n"
        ));
        assert!(
            rendered.contains(&format!("`sources.local` is `kind: {kind}` and declares a `caller_session`")),
            "{kind}: {rendered}"
        );
        assert!(!rendered.contains("unknown field"), "{kind}: {rendered}");
    }
}

#[test]
fn an_unknown_caller_session_is_refused_without_repeating_it() {
    let delegation = "    delegation:\n      - token_endpoint: \"https://idp.example.com/token\"\n        \
                      client_id: \"sutura\"\n        client_secret_file: \"/run/secrets/idp-client\"\n        \
                      audience: \"database-client-id\"\n";
    let rendered = refused_for(&oracle_source(
        "impersonation-at-source",
        &format!("{delegation}    caller_session: \"shared-pool-7f3a\"\n"),
    ));
    assert!(
        rendered
            .contains("`sources.warehouse.caller_session` is not usable: the only values are `own-login` and `end-user-context`"),
        "{rendered}"
    );
    assert!(!rendered.contains("shared-pool-7f3a"), "{rendered}");
}
