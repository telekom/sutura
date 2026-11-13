//! `catalog.kind: rdbms`, served for real: a composed deployment boots over a live Postgres
//! documentation schema and lists what it measures - issue #970's "a served-binary test per kind
//! that boots and lists" acceptance, for `rdbms`.
//!
//! The existing live-reader test (`crates/sutura-catalog-rdbms/tests/provisioned.rs`) exercises the
//! reader directly; #972's own thread calls out that it is not a served/MCP feature-on cell. This
//! is that cell: the COMPOSED BINARY opens the catalog, reads the dictionary, and serves `/catalog`.
//!
//! # Why this asks nothing, and lists rather than answers
//!
//! `sutura-catalog-rdbms` provides `Structure` and may-provide `Descriptions`, `Relationships`,
//! `ColumnTypes` and `ColumnDescriptions`, and declares no measure, no grain and no anchor. A
//! bundle it produces therefore has zero certified metrics, so there is nothing here to ask - the
//! honest coverage, exactly like `served/okf.rs` and `served/openmetadata.rs`, is that the process
//! boots and the `/catalog` route describes the bundle: zero metrics, the right version, and a
//! digest.
//!
//! # Why a `postgres` source, not `files`
//!
//! The rdbms catalog's dictionary rows carry a required `schema_name`, so `convert_table_address`
//! always builds a `QualifiedTable` (e.g. `public.orders`). The `files` engine refuses a qualified
//! model at boot (`crates/sutura-cli/src/serve/files.rs`), so the `source_alias` must name a
//! `postgres` source. The load pins each table's schema from the catalog and reads no source; the
//! fixture still runs a `CREATE TABLE` for it (`crates/sutura-cli/tests/common/fixtures.rs`). The
//! cell still needs no `FixtureLoadGuard` and no example CSV load.
//!
//! # The documentation-schema fixture
//!
//! A per-run schema holds a `columns` table with two rows describing
//! `sutura_fixture_orders.orders` (model `orders`, one `order_id` primary-key column and one
//! `amount` column), `environment: test`, `is_deleted: false` - the documented fixed schema
//! `PostgresReader` reads, the same shape `crates/sutura-catalog-rdbms/tests/provisioned.rs`
//! installs.
//!
//! # RED/GREEN
//!
//! The mutation this cell is meant to catch: drop every table description the reader decodes
//! (the killing patch at
//! `devco/claim-mutations/an_rdbms_catalog_boots_and_lists_from_the_served_binary.patch` filters
//! `table_description` to `None` in the shared `documentation::Assembly::decode` the Postgres reader
//! hands every row to), so the deployment boots and
//! `/catalog` responds but the served digest no longer matches the digest [`expected`] builds
//! independently from the same fixture rows - the cell's own `assert_eq!` fires, RED. GREEN is
//! this file as written. The implementation is already on main (#1105), so this is a claim cell:
//! the commit carries a `Claim-Cell:` trailer and the patch above is the mechanism that holds it.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sutura_domain::pinned::PinnedDefinitions;

    use crate::common::{CATALOG, ENVIRONMENT, SOURCE, install_documentation_schema};
    use crate::harness::{
        DiscoveredTier, LOOPBACK, SINGLE_USER, TOKEN, VERSION, config_path, derived_beside, discover_tier, source_entry,
        start_configured, v1,
    };

    /// A directory this test owns and removes on every path out - see `served/okf.rs`'s `CatalogDir`
    /// for why it is a sibling of the settings directory (`written()` clears the settings directory).
    struct DataDir(PathBuf);

    impl DataDir {
        fn prepared(case: &str) -> Self {
            let path = derived_beside(&config_path(case));
            drop(std::fs::remove_dir_all(&path));
            std::fs::create_dir_all(&path).expect("the data directory is creatable");
            Self(path)
        }
    }

    impl Drop for DataDir {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }

    /// The settings an `rdbms`-kind deployment needs: one `catalog.kind: rdbms` entry reading the
    /// documentation schema over the tier's loopback TLS connection, and one `kind: postgres`
    /// source named for the catalog's `source_alias` reaching the same tier - the `sources:` half
    /// built by [`source_entry`], the same helper `harness/postgres.rs`'s own fixtures use, so this
    /// cell cannot drift from how a served cell reaches the tier.
    fn settings(data: &DataDir, tier: &DiscoveredTier, documentation_schema: &str) -> String {
        let password_file = data.0.join("password");
        std::fs::write(&password_file, format!("{}\n", tier.password)).expect("the password file is writable");
        format!(
            "server:\n\
             {LOOPBACK}\
             security:\n\
             {SINGLE_USER}  access_token: \"{TOKEN}\"\n\
             telemetry:\n  \
               format: \"bunyan\"\n\
             catalogs:\n  \
               - name: \"{CATALOG}\"\n    \
                 kind: \"rdbms\"\n    \
                 version: \"{VERSION}\"\n    \
                 environment: \"{ENVIRONMENT}\"\n    \
                 source_alias: \"{SOURCE}\"\n    \
                 dictionary_schema: \"{documentation_schema}\"\n    \
                 connection:\n      \
                   host: \"127.0.0.1\"\n      \
                   port: {port}\n      \
                   database: \"{database}\"\n      \
                   user: \"{user}\"\n      \
                   password_file: \"{password_file}\"\n      \
                   transport_mode: \"verified\"\n      \
                   transport_anchors: \"{anchor}\"\n\
             sources:\n{source_entry}",
            port = tier.port,
            database = tier.database,
            user = tier.user,
            password_file = password_file.display(),
            anchor = tier.anchor,
            source_entry = source_entry(SOURCE, tier.port, &tier.database, &tier.user, &password_file, &tier.anchor),
        )
    }

    /// The bundle this cell expects - see [`crate::common::expected`].
    fn expected() -> PinnedDefinitions {
        crate::common::expected(VERSION)
    }

    /// An `rdbms`-kind deployment boots from the served binary and lists the bundle it measures:
    /// zero certified metrics (the dictionary carries no measure), the declared version, and a
    /// digest that matches [`expected`]'s independently built one.
    #[test]
    fn an_rdbms_catalog_boots_and_lists_from_the_served_binary() {
        let case = "rdbms-boot-and-list";
        let Some(tier) = discover_tier() else {
            return;
        };
        let data = DataDir::prepared(case);
        let documentation_schema = install_documentation_schema(&tier.host, tier.port);
        let deployment = start_configured(case, &settings(&data, &tier, &documentation_schema));

        let reply = deployment.get(&v1(sutura_http::constants::base_paths::CATALOG), Some(TOKEN));
        assert_eq!(reply.status, 200, "{}", reply.body);
        let body = reply.json();
        assert_eq!(
            body["metrics"].as_array().map(Vec::len),
            Some(0),
            "an RDBMS dictionary carries no measure, so a served listing must carry none: {}",
            reply.body
        );
        assert_eq!(
            body["provenance"]["definition_version"], VERSION,
            "the served bundle must be stamped with the declared version: {}",
            reply.body
        );
        assert_eq!(
            body["provenance"]["definition_digest"],
            expected().digest().as_str(),
            "the served RDBMS bundle differs from the independently built fixture digest: {}",
            reply.body
        );
    }
    #[cfg(feature = "sql")]
    mod native {
        use super::{DataDir, settings};
        use crate::harness::{
            DiscoveredTier, Reply, Served, TOKEN, discover_tier, refused_to_start, start_configured, v1, written,
        };
        use std::time::{Duration, Instant};
        use sutura_config::Environment;
        use sutura_domain::model::SourceName;
        use sutura_exec_postgres::adbc::{Channel, FixtureAdmin, PostgresDriver};
        use sutura_exec_postgres::fixture::FixtureCredential;

        struct Native {
            data: DataDir,
            schema: String,
            admin: FixtureAdmin,
        }

        impl Native {
            fn new(case: &str, tier: &DiscoveredTier) -> Self {
                let schema = format!("native_{}_{}", case.replace('-', "_"), std::process::id());
                let source = SourceName::parse("fixture").unwrap();
                let conninfo = FixtureCredential::from_env()
                    .unwrap()
                    .conninfo(&source, &tier.host, tier.port, Channel::Plaintext)
                    .unwrap();
                let driver = PostgresDriver::from_host().unwrap();
                let mut admin = FixtureAdmin::open(&driver, &conninfo).unwrap();
                admin
                    .run(&format!(
                        "CREATE SCHEMA {schema}; CREATE TABLE {schema}.events (id bigint, month date, secret text); \
                    INSERT INTO {schema}.events VALUES (7, DATE '2026-06-01', 'private value'); \
                    COMMENT ON TABLE {schema}.events IS 'Metadata; ''quotes'' stay descriptive.'; \
                    COMMENT ON COLUMN {schema}.events.id IS 'The event key.'"
                    ))
                    .unwrap();
                Self {
                    data: DataDir::prepared(&format!("{case}-fixture")),
                    schema,
                    admin,
                }
            }

            fn native_settings(&self, tier: &DiscoveredTier, extra: &str) -> String {
                let base = settings(&self.data, tier, &self.schema)
                    .replace("    environment: \"test\"\n", "")
                    .replace(&format!("    dictionary_schema: \"{}\"\n", self.schema),
                        &format!("    dictionary_source: native_dictionary\n    refresh_seconds: 1\n    include: [{{schema: {}, table: events}}]\n    exclude_columns: ['secret*']\n{extra}", self.schema));
                format!("{base}tools:\n  run_sql:\n    enabled: true\n    sources: [warehouse]\n    federation_allowed: true\n")
            }

            fn compose(&self, base: &str, column: Option<&str>) -> String {
                let dir = self.data.0.join("catalog");
                std::fs::create_dir_all(&dir).unwrap();
                let definitions = if column.is_some() {
                    "structure, metrics, grains, descriptions, column_types"
                } else {
                    "metrics, grains, descriptions"
                };
                std::fs::write(
                    dir.join("declaration.md"),
                    format!("---\nkind: declaration\ndefinitions: [{definitions}]\nknowledge: []\n---\n"),
                )
                .unwrap();
                std::fs::write(dir.join("metric.md"), "---\nkind: metric\nname: event_total\nmodel: events\nmeasure:\n  simple: {aggregate: sum, column: id}\ntime_column: month\ngrains: [month]\ndimensions: []\naudience: open\n---\nIncluded events.\n").unwrap();
                if let Some(column) = column {
                    let kind = if column == "secret" { "text" } else { "bigint" };
                    std::fs::write(dir.join("alias.md"), format!("---\nkind: model\nname: alias\nsource: warehouse\ntable: {}.events\ncolumns:\n  - {{name: {column}, type: {kind}}}\n---\n", self.schema)).unwrap();
                } else if let Err(cause) = std::fs::remove_file(dir.join("alias.md")) {
                    assert_eq!(
                        cause.kind(),
                        std::io::ErrorKind::NotFound,
                        "the old alias must be removed: {cause}"
                    );
                }
                base.replace("sources:\n", &format!("  - name: measures\n    kind: markdown\n    version: \"{}\"\n    refresh_seconds: 1\n    dir: {}\n    data_dir: {}\nsources:\n", crate::harness::VERSION, dir.display(), self.data.0.display()))
            }
        }

        impl Drop for Native {
            fn drop(&mut self) {
                drop(self.admin.run(&format!("DROP SCHEMA {} CASCADE", self.schema)));
            }
        }

        fn ran(served: &Served, statement: &str) -> Reply {
            served.post(
                &v1(sutura_http::constants::base_paths::RUN_SQL),
                Some(TOKEN),
                &serde_json::json!({"statement": statement}).to_string(),
            )
        }

        fn catalog(served: &Served) -> serde_json::Value {
            let reply = served.get(&v1(sutura_http::constants::base_paths::CATALOG), Some(TOKEN));
            assert_eq!(reply.status, 200, "{}", reply.body);
            reply.json()
        }

        #[test]
        fn native_postgres_metadata_is_exact_bounded_and_keeps_schema_patterns_as_data() {
            use sutura_catalog_rdbms::{
                ColumnMetadata, Dictionary, DictionaryReader, RdbmsCatalog, RdbmsError, Table, TableAddress,
            };
            use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};
            struct Recorded<'a>(&'a str);
            impl DictionaryReader for Recorded<'_> {
                fn read_dictionary(&self) -> Result<Dictionary, RdbmsError> {
                    Ok(Dictionary::new(
                        vec![
                            Table::new(
                                String::from("events"),
                                TableAddress::in_schema(self.0.to_owned(), String::from("events")),
                                vec![String::from("id"), String::from("month"), String::from("amount")],
                                Some(String::from("Metadata; 'quotes' stay descriptive.")),
                            )
                            .with_column_metadata([
                                (
                                    String::from("id"),
                                    ColumnMetadata::new(Some(String::from("bigint")), Some(String::from("The event key."))),
                                ),
                                (String::from("month"), ColumnMetadata::new(Some(String::from("date")), None)),
                                (
                                    String::from("amount"),
                                    ColumnMetadata::new(
                                        Some(String::from("numeric(12,2)")),
                                        Some(String::from("An exact decimal.")),
                                    ),
                                ),
                            ]),
                        ],
                        vec![],
                    ))
                }
            }
            let Some(tier) = discover_tier() else { return };
            let mut fixture = Native::new("native-metadata", &tier);
            fixture
                .admin
                .run(&format!(
                    "ALTER TABLE {}.events ADD amount numeric(12,2); COMMENT ON COLUMN {}.events.amount IS 'An exact decimal.'",
                    fixture.schema, fixture.schema
                ))
                .unwrap();
            let expected = RdbmsCatalog::new(
                SourceName::parse("dictionary").unwrap(),
                DefinitionVersion::parse(crate::harness::VERSION).unwrap(),
                Recorded(&fixture.schema),
            )
            .with_source_alias(SourceName::parse("warehouse").unwrap())
            .load()
            .unwrap();
            let base = fixture.native_settings(&tier, "");
            let served = start_configured("native-metadata", &base);
            assert_eq!(
                catalog(&served)["provenance"]["definition_digest"],
                expected.digest().as_str(),
                "native comments and exact decimal metadata must survive the served load"
            );
            drop(served);
            for (case, settings, expected) in [
                (
                    "row-bound",
                    fixture.native_settings(&tier, "    max_dictionary_rows: 1\n"),
                    "maximum of 1 rows",
                ),
                (
                    "byte-bound",
                    fixture.native_settings(&tier, "    max_dictionary_bytes: 1\n"),
                    "maximum of 1 bytes",
                ),
                ("no-columns", base.replace("['secret*']", "['*']"), "no included column"),
                (
                    "unmatched-table",
                    base.replace("table: events", "table: missing"),
                    "matches no visible table",
                ),
                (
                    "schema-data",
                    base.replace(&format!("schema: {}", fixture.schema), r#"schema: "x' OR '1'='1""#),
                    "matches no visible table",
                ),
            ] {
                let log =
                    refused_to_start(Environment::Development, written(&format!("native-{case}"), &settings), &[]).join("\n");
                assert!(log.contains(expected), "{case}: {log}");
            }
            for kind in [
                "jsonb",
                "real",
                "bytea",
                "timestamp without time zone",
                "timestamp with time zone",
            ] {
                fixture
                    .admin
                    .run(&format!("ALTER TABLE {}.events ADD unsupported {kind}", fixture.schema))
                    .unwrap();
                eprintln!("native dictionary refusal probe: PostgreSQL {kind}");
                let log = refused_to_start(Environment::Development, written("native-unsupported", &base), &[]).join("\n");
                assert!(
                    log.contains(&format!("no exact supported mapping for {kind}")),
                    "{kind}: {log}"
                );
                drop(start_configured(
                    "native-excluded-type",
                    &fixture.native_settings(&tier, &format!("    exclude_types: ['{kind}']\n")),
                ));
                fixture
                    .admin
                    .run(&format!("ALTER TABLE {}.events DROP COLUMN unsupported", fixture.schema))
                    .unwrap();
            }
        }

        #[test]
        fn a_served_native_dictionary_limits_real_planned_sql_and_composes_markdown_metrics() {
            let Some(tier) = discover_tier() else { return };
            let fixture = Native::new("native-planned", &tier);
            let settings = fixture.compose(&fixture.native_settings(&tier, ""), None);
            let served = start_configured("native-planned", &settings);
            let listing = catalog(&served);
            assert_eq!(listing["metrics"][0]["name"], "event_total");
            assert!(!listing.to_string().contains("secret"));
            let select = ran(&served, &format!("select * from {}.events", fixture.schema));
            assert_eq!(select.status, 200, "{}", select.body);
            assert_eq!(select.json()["columns"], serde_json::json!(["id", "month"]));
            assert_eq!(select.json()["rows"], serde_json::json!([["7", "2026-06-01"]]));
            for statement in [
                format!("select e.secret from {}.events e", fixture.schema),
                format!("select \"secret\" from \"{}\".\"events\"", fixture.schema),
                format!("select id from {}.events where secret = 'private value'", fixture.schema),
                format!(
                    "select a.id from {0}.events a join {0}.events b on a.secret = b.secret",
                    fixture.schema
                ),
            ] {
                let reply = ran(&served, &statement);
                assert_eq!(reply.status, 422, "{statement}: {}", reply.body);
                assert_eq!(reply.json()["outcome"], "raw_refusal", "{}", reply.body);
                assert_eq!(reply.json()["code"], "statement_failed", "{}", reply.body);
                assert!(!reply.body.contains("private value"));
            }
            let certified = served.post(&v1(sutura_http::constants::base_paths::QUERY), Some(TOKEN),
                r#"{"metrics":["event_total"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},"dimensions":[]}"#);
            assert_eq!(certified.status, 200, "{}", certified.body);
            assert_eq!(
                certified.json()["rows"],
                serde_json::json!([["2026-06-01", "7"]]),
                "{}",
                certified.body
            );
        }

        #[test]
        fn native_refresh_adds_allowed_columns_and_retains_the_last_bundle_on_refusal() {
            let Some(tier) = discover_tier() else { return };
            let mut fixture = Native::new("native-refresh", &tier);
            let served = start_configured("native-refresh", &fixture.native_settings(&tier, ""));
            let before = catalog(&served)["provenance"]["definition_digest"].clone();
            fixture.admin.run(&format!("ALTER TABLE {}.events ADD allowed bigint DEFAULT 9; ALTER TABLE {}.events ADD secret_new text DEFAULT 'another private value'", fixture.schema, fixture.schema)).unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            let after = loop {
                let digest = catalog(&served)["provenance"]["definition_digest"].clone();
                if digest != before {
                    break digest;
                }
                assert!(Instant::now() < deadline, "refresh never published the new allowed column");
                std::thread::sleep(Duration::from_millis(100));
            };
            let reply = ran(&served, &format!("select allowed from {}.events", fixture.schema));
            assert_eq!(reply.status, 200, "{}", reply.body);
            assert_eq!(reply.json()["rows"], serde_json::json!([["9"]]));
            let hidden = ran(&served, &format!("select secret_new from {}.events", fixture.schema));
            assert_eq!(hidden.status, 422, "{}", hidden.body);
            fixture
                .admin
                .run(&format!("ALTER TABLE {}.events ADD unsupported jsonb", fixture.schema))
                .unwrap();
            served.awaiting("keeping the bundle already served");
            assert_eq!(catalog(&served)["provenance"]["definition_digest"], after);
            let reply = ran(&served, &format!("select * from {}.events", fixture.schema));
            assert_eq!(reply.status, 200, "{}", reply.body);
            assert_eq!(reply.json()["columns"], serde_json::json!(["allowed", "id", "month"]));
        }

        #[test]
        fn a_native_dictionary_refuses_composed_aliases_and_a_second_authority() {
            let Some(tier) = discover_tier() else { return };
            let fixture = Native::new("native-alias", &tier);
            let base = fixture.native_settings(&tier, "");
            let accepted = fixture.compose(&base, Some("id"));
            drop(start_configured("native-alias-control", &accepted));
            let rejected = fixture.compose(&base, Some("secret"));
            eprintln!("native dictionary refusal probe: composed secret alias");
            let log = refused_to_start(Environment::Development, written("native-alias-refused", &rejected), &[]).join("\n");
            assert!(
                log.contains("outside its native allowlist") && log.contains("secret"),
                "{log}"
            );
            let unfaithful = fixture.compose(&base, None);
            let declaration = fixture.data.0.join("catalog/declaration.md");
            let text = std::fs::read_to_string(&declaration).unwrap();
            std::fs::write(
                &declaration,
                text.replace("definitions: [metrics", "definitions: [structure, metrics"),
            )
            .unwrap();
            let log = refused_to_start(Environment::Development, written("native-unfaithful", &unfaithful), &[]).join("\n");
            assert!(
                log.contains("declares kinds its own content does not supply"),
                "native models must not make Markdown's own declaration appear faithful: {log}"
            );
            let duplicate = base.replace("sources:\n", &format!("  - name: second\n    kind: rdbms\n    version: \"{}\"\n    dictionary_source: native_dictionary\n    source_alias: warehouse\n    include: [{{schema: {}, table: events}}]\n    exclude_columns: [secret]\n    connection:\n      host: 127.0.0.1\n      port: {}\n      database: {}\n      user: {}\n      password_file: {}\n      transport_mode: verified\n      transport_anchors: {}\nsources:\n", crate::harness::VERSION, fixture.schema, tier.port, tier.database, tier.user, fixture.data.0.join("password").display(), tier.anchor));
            let log = refused_to_start(Environment::Development, written("native-two-authorities", &duplicate), &[]).join("\n");
            assert!(log.contains("more than one native dictionary authority"), "{log}");
        }
    }
}
