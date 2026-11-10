#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use sutura_config::{Environment, Settings, Sources};

    const NATIVE: &str =
        "transport_mode: native-encryption\n    native_encryption_acknowledged_because: the listener requires native encryption";
    const VERIFIED: &str = "transport_mode: verified\n    transport_anchors: /etc/sutura/oracle-ca.pem";
    const LEGACY: &str =
        "legacy_password_verifier: true\n    legacy_password_verifier_acknowledged_because: this account cannot be reset";

    fn source(channel: &str, extra: &str) -> String {
        format!(
            "security:\n  identity: single-user\n  single_user_because: a test\nsources:\n  warehouse:\n    kind: oracle\n    host: db.example.com\n    port: 1521\n    service_name: FREEPDB1\n    user: reader\n    password_file: /run/secrets/oracle\n    posture: shared-service-user\n    {channel}\n    {extra}\n"
        )
    }

    fn dictionary(channel: &str, extra: &str) -> String {
        let connection = source(channel, extra)
            .split_once("    host:")
            .expect("source has a host")
            .1
            .replace("    posture: shared-service-user\n", "");
        let connection = format!("dialect: oracle\n    host:{connection}");
        let mut indented = String::new();
        for line in connection.lines() {
            indented.push_str("      ");
            indented.push_str(line.trim_start());
            indented.push('\n');
        }
        format!(
            "security:\n  identity: single-user\n  single_user_because: a test\nsources:\n  warehouse:\n    kind: files\n    data_dir: /srv/data\n    posture: shared-service-user\ncatalogs:\n  - name: dictionary\n    kind: rdbms\n    version: dictionary-1\n    environment: test\n    source_alias: warehouse\n    connection:\n{indented}"
        )
    }

    fn load(overlay: &str) -> Result<Settings, sutura_config::SettingsLoadError> {
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
    }

    #[test]
    fn native_source_and_dictionary_require_their_own_reason_and_refuse_tls_material() {
        for config in [source, dictionary] {
            load(&config(NATIVE, "")).expect("acknowledged native encryption loads");
            for channel in [
                "transport_mode: native-encryption",
                "transport_mode: native-encryption\n    native_encryption_acknowledged_because: ' '",
            ] {
                assert!(load(&config(channel, "")).is_err(), "a native mode requires a reason");
            }
            for extra in [
                "transport_anchors: /etc/sutura/ca.pem",
                "client_certificate: /etc/sutura/client.pem",
                "client_key: /etc/sutura/client.key",
            ] {
                assert!(load(&config(NATIVE, extra)).is_err(), "native encryption cannot read {extra}");
            }
            load(&config(VERIFIED, "native_encryption_acknowledged_because: unused"))
                .expect_err("verified TLS cannot consume a native-mode reason");
        }
    }

    #[test]
    fn legacy_source_and_dictionary_require_an_independent_reason_and_an_encrypted_mode() {
        for config in [source, dictionary] {
            for channel in [NATIVE, VERIFIED] {
                load(&config(channel, LEGACY)).expect("independently acknowledged legacy login loads");
                load(&config(channel, "legacy_password_verifier: false")).expect("legacy login can remain explicitly disabled");
                load(&config(channel, "legacy_password_verifier: true")).expect_err("legacy login requires its own reason");
                load(&config(
                        channel,
                        "legacy_password_verifier: true\n    legacy_password_verifier_acknowledged_because: ' '"
                    ))
                    .expect_err("legacy login refuses a blank reason");
                load(&config(
                        channel,
                        "legacy_password_verifier: false\n    legacy_password_verifier_acknowledged_because: unused"
                    ))
                    .expect_err("disabled legacy login cannot consume a reason");
                load(&config(channel, "legacy_password_verifier_acknowledged_because: unused"))
                    .expect_err("a legacy reason requires the opt-in");
            }
            let plaintext = config("transport_mode: plaintext", LEGACY).replace("db.example.com", "127.0.0.1");
            assert!(load(&plaintext).is_err(), "legacy password logins cannot use plaintext");
        }
    }

    #[test]
    fn oracle_only_options_are_refused_on_other_sources_and_dictionary_dialects() {
        load(&source(NATIVE, LEGACY)).expect("Oracle consumes its options");
        for extra in [
            "native_encryption_acknowledged_because: native transport",
            "legacy_password_verifier: false",
            "legacy_password_verifier_acknowledged_because: old account",
        ] {
            for kind in ["files", "postgres", "clickhouse", "bigquery", "duckdb", "cube", "node"] {
                let config = format!(
                    "security:\n  identity: single-user\n  single_user_because: a test\nsources:\n  warehouse:\n    kind: {kind}\n    posture: shared-service-user\n    {extra}\n"
                );
                let error = load(&config).expect_err("an Oracle-only field is refused");
                let key = extra.split(':').next().expect("key");
                let mut found = error.reason().source();
                let mut named = error.reason().to_string().contains(key);
                while let Some(cause) = found {
                    named |= cause.to_string().contains(key);
                    found = cause.source();
                }
                assert!(named, "the refusal must name {key}: {error:?}");
            }
            let postgres = dictionary("transport_mode: plaintext", extra)
                .replace("dialect: oracle", "dialect: postgres")
                .replace("service_name: FREEPDB1", "database: dictionary")
                .replace("db.example.com", "127.0.0.1");
            assert!(
                load(&postgres).is_err(),
                "a Postgres dictionary cannot consume Oracle options"
            );
        }
    }

    #[test]
    fn native_mode_cannot_carry_oracle_external_token_authentication() {
        let delegation = "delegation:\n      - token_endpoint: https://idp.example.com/token\n        client_id: sutura\n        client_secret_file: /run/secrets/delegation\n        audience: https://db.example.com";
        let config = |channel| {
            source(channel, delegation)
            .replace("shared-service-user", "impersonation-at-source")
            .replace("sources:\n", "  inbound:\n    mode: direct\n    resource: https://sutura.example.com\n    authorization_server: https://issuer.example.com\n    key_set_file: /etc/sutura/jwks.json\n    algorithms: [RS256]\nsources:\n")
        };
        load(&config(VERIFIED)).expect("otherwise-valid external-token configuration loads over verified TLS");
        let error = load(&config(NATIVE)).expect_err("token authentication still requires verified TCPS");
        assert!(
            matches!(
                error.reason(),
                sutura_config::SettingsError::Sources {
                    cause: sutura_config::InvalidSourceRegistry::TlsNotDeliverable {
                        mode: "native-encryption with token authentication (verified TLS required)",
                        ..
                    }
                }
            ),
            "{error:?}"
        );
    }

    #[test]
    fn orphan_oracle_acknowledgements_are_refused_even_without_the_oracle_feature() {
        load(&source(NATIVE, "")).expect("the encrypted declaration parses without linking a driver");
        for extra in [
            "native_encryption_acknowledged_because: unused",
            "legacy_password_verifier_acknowledged_because: unused",
        ] {
            let config = source("transport_mode: plaintext", extra).replace("db.example.com", "127.0.0.1");
            assert!(load(&config).is_err(), "unlinked Oracle must not skip option validation");
        }
    }
}
