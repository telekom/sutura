#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use sutura_config::{Environment, Settings, SettingsLoadError, Sources};

    const DEPLOYMENT: &str = "
security:
  identity: single-user
  single_user_because: a test
sources:
  analytics:
    kind: duckdb
    database_file: /srv/sutura/warehouse.duckdb
    posture: shared-service-user
  pg:
    kind: postgres
    posture: shared-service-user
    acknowledged_because: one service role reaching the database for everybody who asks
    unix_socket: /tmp/sutura-pg
    port: 5432
    database: sutura
    user: sutura
    password_file: /etc/sutura/pg-password
    transport_mode: plaintext
catalogs:
";

    const HEAD: &str = "
  - name: marketplace
    kind: entropy
    version: market-1
    endpoint: https://marketplace.example.com
    metric_property: sutura_metrics";

    const API_KEY: &str = "
    api_key_file: /run/secrets/entropy-api-key";

    const DUCKDB: &str = "
    bindings:
      - type: duckdb
        database: warehouse
        source: analytics";

    fn load(catalog: &str) -> Result<Settings, SettingsLoadError> {
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(format!("{DEPLOYMENT}{catalog}\n")))
    }

    fn refusal(catalog: &str) -> String {
        let refused = load(catalog).expect_err("the entry is refused");
        format!("{:?}", refused.reason())
    }

    #[test]
    fn an_entropy_catalog_with_an_api_key_and_a_duckdb_binding_loads() {
        let settings = load(&format!("{HEAD}{API_KEY}{DUCKDB}")).expect("a complete entropy entry loads");
        let catalog = settings.catalogs().each().next().expect("one catalog");
        assert_eq!(catalog.kind().as_str(), "entropy");
        assert_eq!(catalog.endpoint(), Some("https://marketplace.example.com"));
        assert_eq!(catalog.metric_property(), Some("sutura_metrics"));
        assert_eq!(catalog.dir(), None);
        let held = format!("{catalog:?}");
        assert!(held.contains(r#"ApiKey { file: "/run/secrets/entropy-api-key" }"#), "{held}");
        assert!(held.contains(r#"Duckdb { database: "warehouse", schema: None }"#), "{held}");
    }

    #[test]
    fn an_entropy_catalog_signing_in_by_oidc_binds_a_postgres_server() {
        let settings = load(&format!(
            "{HEAD}
    oidc:
      token_endpoint: https://marketplace.example.com/oauth2/token
      client_id: sutura
      client_secret_file: /run/secrets/entropy-client
      client_auth: post
      scope: mcp:read
    bindings:
      - type: postgresql
        host: db.example.com
        port: 5432
        database: sutura
        schema: sales
        source: pg"
        ))
        .expect("an oidc entry loads");
        let held = format!("{:?}", settings.catalogs().each().next().expect("one catalog"));
        assert!(held.contains("client_auth: Post"), "{held}");
        assert!(held.contains(r#"scope: Some("mcp:read")"#), "{held}");
        assert!(
            held.contains(r#"Postgres { host: "db.example.com", port: 5432, database: "sutura", schema: "sales" }"#),
            "{held}"
        );
    }

    #[test]
    fn an_entropy_catalog_declares_exactly_one_sign_in() {
        let both = format!(
            "{HEAD}{API_KEY}
    oidc:
      token_endpoint: https://marketplace.example.com/oauth2/token
      client_id: sutura
      client_secret_file: /run/secrets/entropy-client{DUCKDB}"
        );
        for catalog in [both, format!("{HEAD}{DUCKDB}")] {
            let refused = refusal(&catalog);
            assert!(refused.contains("AuthMode"), "{refused}");
        }
    }

    #[test]
    fn an_entropy_catalog_refuses_what_it_does_not_read() {
        let cases = [
            (format!("{HEAD}{API_KEY}"), "NoBindings"),
            (
                format!("{HEAD}{API_KEY}\n    token_file: /run/secrets/token{DUCKDB}"),
                r#"Unread { key: "token_file" }"#,
            ),
            (
                format!("{HEAD}\n    api_key_file: secrets/entropy-api-key{DUCKDB}"),
                r#"RelativePath { key: "api_key_file""#,
            ),
            (
                format!("\n  - name: marketplace\n    kind: entropy\n    version: market-1{API_KEY}{DUCKDB}"),
                r#"MissingForReader { kind: "entropy", field: "endpoint" }"#,
            ),
        ];
        for (catalog, expected) in cases {
            let refused = refusal(&catalog);
            assert!(refused.contains(expected), "{expected} not in {refused}");
        }
    }

    #[test]
    fn a_binding_names_a_bindable_server_and_a_declared_source_of_its_kind() {
        let binding = |keys: &[&str]| format!("{HEAD}{API_KEY}\n    bindings:\n      - {}", keys.join("\n        "));
        let cases = [
            (
                binding(&["type: cloudsql", "database: warehouse", "source: analytics"]),
                "UnsupportedType",
            ),
            (
                binding(&["type: duckdb", "source: analytics"]),
                r#"MissingKey { system: "duckdb", key: "database" }"#,
            ),
            (
                binding(&[
                    "type: duckdb",
                    "database: warehouse",
                    "host: db.example.com",
                    "source: analytics",
                ]),
                r#"NotAKey { system: "duckdb", key: "host" }"#,
            ),
            (
                binding(&["type: duckdb", "database: warehouse", "source: nowhere"]),
                "UnknownSource",
            ),
            (
                binding(&[
                    "type: bigquery",
                    "project: example-project",
                    "dataset: sales",
                    "source: analytics",
                ]),
                r#"declared: "duckdb", system: "bigquery""#,
            ),
            (
                binding(&[
                    "type: postgres",
                    "host: db.example.com",
                    "port: 5432",
                    "database: other",
                    "schema: sales",
                    "source: pg",
                ]),
                r#"written: "other", dialed: "sutura""#,
            ),
        ];
        for (catalog, expected) in cases {
            let refused = refusal(&catalog);
            assert!(refused.contains("Binding { position: 0"), "{refused}");
            assert!(refused.contains(expected), "{expected} not in {refused}");
        }
    }

    #[test]
    fn two_bindings_of_one_server_are_refused_whatever_the_postgres_spelling() {
        let server =
            "host: db.example.com\n        port: 5432\n        database: sutura\n        schema: sales\n        source: pg";
        let refused = refusal(&format!(
            "{HEAD}{API_KEY}\n    bindings:\n      - type: postgres\n        {server}\n      - type: postgresql\n        {server}"
        ));
        assert!(refused.contains("DuplicateBinding { first: 0, second: 1 }"), "{refused}");
    }

    #[test]
    fn an_entropy_key_on_another_kind_is_refused_naming_it() {
        let refused = refusal(
            "
  - name: docs
    kind: markdown
    version: docs-1
    dir: /srv/catalog
    data_dir: /srv/data
    api_key_file: /run/secrets/entropy-api-key",
        );
        assert!(refused.contains("EntropyKeyOnOtherKind"), "{refused}");
        assert!(refused.contains(r#"key: "api_key_file""#), "{refused}");
    }
}
