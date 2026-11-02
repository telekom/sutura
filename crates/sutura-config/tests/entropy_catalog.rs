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
      - match:
          type: duckdb
          database: warehouse
        source: analytics";

    fn load(catalog: &str) -> Result<Settings, SettingsLoadError> {
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(format!("{DEPLOYMENT}{catalog}\n")))
    }

    fn refusal(catalog: &str) -> String {
        let refused = load(catalog).expect_err("the entry is refused");
        format!("{:?}", refused.reason())
    }

    /// One `bindings:` entry: `fields` under `match:`, one `name: value` each.
    fn binding(fields: &[&str], source: &str) -> String {
        format!(
            "\n      - match:\n          {}\n        source: {source}",
            fields.join("\n          ")
        )
    }

    /// An API-key entry binding each of `entries`.
    fn bound(entries: &[String]) -> String {
        format!("{HEAD}{API_KEY}\n    bindings:{}", entries.concat())
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
        assert!(held.contains(r#"kind: FieldValue("duckdb")"#), "{held}");
        assert!(held.contains(r#"FieldName("database"): FieldValue("warehouse")"#), "{held}");
    }

    const OIDC: &str = "
    oidc:
      token_endpoint: https://login.example.com/oauth2/token
      client_id: sutura
      client_secret_file: /run/secrets/entropy-client";

    #[test]
    fn an_entropy_catalog_signing_in_by_oidc_binds_a_postgres_server() {
        let settings = load(&format!(
            "{HEAD}{OIDC}
      scope: catalog.read
    bindings:
      - match:
          type: postgres
          host: db.example.com
          port: 5432
        source: pg"
        ))
        .expect("an oidc entry loads");
        let held = format!("{:?}", settings.catalogs().each().next().expect("one catalog"));
        assert!(
            held.contains(r#"token_endpoint: "https://login.example.com/oauth2/token""#),
            "{held}"
        );
        assert!(held.contains(r#"scope: Some("catalog.read")"#), "{held}");
        assert!(held.contains(r#"FieldName("port"): FieldValue("5432")"#), "{held}");
    }

    #[test]
    fn an_oidc_client_authenticates_by_http_basic_alone() {
        let refused = refusal(&format!("{HEAD}{OIDC}\n      client_auth: post{DUCKDB}"));
        assert!(refused.contains("unknown field `client_auth`"), "{refused}");
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
    fn a_binding_matches_a_type_and_a_field_and_names_a_declared_source() {
        let cases = [
            (binding(&["database: warehouse"], "analytics"), "NoType"),
            (binding(&["type: duckdb"], "analytics"), "NoField"),
            (
                binding(&["type: duckdb", r#"database: """#], "analytics"),
                r#"Blank { field: "database" }"#,
            ),
            (binding(&["type: duckdb", "database: warehouse"], "nowhere"), "UnknownSource"),
        ];
        for (entry, expected) in cases {
            let refused = refusal(&bound(&[entry]));
            assert!(refused.contains("Binding { position: 0"), "{refused}");
            assert!(refused.contains(expected), "{expected} not in {refused}");
        }
    }

    #[test]
    fn bindings_that_could_match_one_server_are_refused() {
        type Pair = [&'static [&'static str]; 2];
        const HOST: &str = "host: db.example.com";
        let overlapping: [Pair; 3] = [
            [&["type: postgres", HOST], &["type: postgres", HOST]],
            [&["type: postgres", HOST], &["type: postgres", HOST, "database: sutura"]],
            [&["type: postgres", "database: sutura"], &["type: postgres", "schema: sales"]],
        ];
        for [first, second] in overlapping {
            let refused = refusal(&bound(&[binding(first, "pg"), binding(second, "analytics")]));
            assert!(refused.contains("OverlappingBindings { first: 0, second: 1 }"), "{refused}");
        }
        let apart: [Pair; 2] = [
            [
                &["type: postgres", HOST, "schema: sales"],
                &["type: postgres", HOST, "schema: finance"],
            ],
            [&["type: postgres", HOST], &["type: postgresql", HOST]],
        ];
        for [first, second] in apart {
            load(&bound(&[binding(first, "pg"), binding(second, "analytics")]))
                .expect("another type or a field with different values keeps two bindings apart");
        }
    }

    #[test]
    fn a_server_type_sutura_does_not_name_binds_by_its_fields() {
        let entry = binding(
            &[
                "type: snowflake",
                "account: example-account",
                "database: FULFILLMENT_DB",
                "schema: SHELF_WARMERS",
            ],
            "analytics",
        );
        let settings = load(&bound(&[entry])).expect("any type binds by its fields");
        let held = format!("{:?}", settings.catalogs().each().next().expect("one catalog"));
        assert!(held.contains(r#"kind: FieldValue("snowflake")"#), "{held}");
        assert!(held.contains(r#"FieldName("schema"): FieldValue("SHELF_WARMERS")"#), "{held}");
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
