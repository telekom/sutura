//! `catalog.kind: rdbms` over the stdio agent surface: a composed `sutura mcp` process boots over
//! a live Postgres documentation schema and lists the bundle it measures - issue #970 box A7, the
//! positive RDBMS run over the MCP transport the served-HTTP cell (`served/rdbms.rs`, PR #1147)
//! does not cover.
//!
//! # What this reuses, and what it does NOT prove
//!
//! The fixture is the SAME documentation schema `served/rdbms.rs` installs (PR #1147) and
//! `crates/sutura-catalog-rdbms/tests/provisioned.rs` reads: a per-run schema with a `columns`
//! table whose rows describe `public.orders`. The settings shape is the same `catalog.kind: rdbms`
//! + `kind: postgres` source pair, over the provisioned tier's verified TLS connection. The spawn
//!   harness is this suite's own (`spawn_configured`/`Agent`), not the served harness. The fixture
//!   install and its constants live in `tests/common/`, which both test binaries mount as
//!   `crate::common`, so the two cells cannot drift onto different rows.
//!
//! This cell proves the stdio transport specifically: the composed binary opens the rdbms catalog
//! through the ONE opener `sutura serve` uses, loads it (reading the dictionary from Postgres),
//! opens the `postgres` engine, and serves MCP on its pipes. It does NOT prove a row query (an
//! rdbms dictionary carries no measure, so `ask_metric` has nothing certified to answer), does NOT
//! prove leg-2 identity (a pipe establishes no caller), and does NOT prove the served-HTTP venue -
//! that is `served/rdbms.rs`'s own cell.
//!
//! # Why the assertions are what they are
//!
//! An rdbms dictionary yields structure and prose and no measure, so a bundle it produces has zero
//! certified metrics - the honest coverage is that the process boots and the surface describes the
//! bundle. `initialize.instructions` carries `physical_schema_guidance`'s fixed sentence exactly
//! when the bundle has models and zero metrics, so asserting that sentence is present proves the
//! dictionary's models loaded and pinned over THIS process's stdio transport - a fixed string a
//! stub could never produce, because the guidance is gated on a non-empty `models()` and an empty
//! `metrics()`. `describe_catalog` then returns the structured listing: zero metrics, the declared
//! version, and a non-empty digest.
//!
//! # RED/GREEN
//!
//! The mutation this cell is meant to catch: corrupt the version `open_one_rdbms_catalog` stamps
//! into the catalog (the killing patch at
//! `devco/claim-mutations/an_rdbms_catalog_boots_and_lists_over_stdio.patch` replaces
//! `settings.version().clone()` with a wrong `DefinitionVersion`), so the process boots and
//! `describe_catalog` responds but its `provenance.definition_version` no longer matches the
//! declared `VERSION` - the cell's own `assert_eq!` fires, RED. GREEN is this file as written.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use sutura_app::Capability;

    use crate::common::{CATALOG, ENVIRONMENT, SOURCE, install_documentation_schema};
    use crate::harness::{VERSION, spawn_configured};

    /// The service the provisioner is asked for - the same name `nix/postgres-tier.nix` publishes.
    const SERVICE: &str = "postgres";

    /// The tier's three credential exports.
    const USER: &str = "SUTURA_POSTGRES_TIER_USER";
    const PASSWORD: &str = "SUTURA_POSTGRES_TIER_PASSWORD";
    const DATABASE: &str = "SUTURA_POSTGRES_TIER_DB";
    const ANCHOR: &str = "SUTURA_POSTGRES_TIER_CA";

    /// A directory this test owns and removes on every path out - the password file the settings
    /// tree names lives here, so dropping it clears the secret rather than leaving it in `target/`.
    struct DataDir(PathBuf);

    impl DataDir {
        fn prepared(case: &str) -> Self {
            let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
                "{case}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("the clock is after the epoch")
                    .as_nanos()
            ));
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

    /// What the fixture and the deployment need from the provisioned tier: a unix-socket config
    /// for the fixture install (`local all all trust`, no TLS), and the TCP port plus credentials
    /// for the spawned binary's own `verified` connections. `None` where nothing is provisioned
    /// (the `SKIPPED` notice is already on stderr).
    fn tier() -> Option<Tier> {
        let endpoint = sutura_dev::provisioned::here(std::path::Path::new(env!("CARGO_MANIFEST_DIR")), SERVICE);
        let endpoint = endpoint.endpoint()?;
        let required = |name: &str| {
            std::env::var(name).unwrap_or_else(|_| panic!("the postgres tier is present but {name} is not published"))
        };
        let user = required(USER);
        let password = required(PASSWORD);
        let database = required(DATABASE);
        let anchor = required(ANCHOR);
        let mut fixture_config = tokio_postgres::Config::new();
        fixture_config
            .host(endpoint.host())
            .port(endpoint.port())
            .user(&user)
            .password(&password)
            .dbname(&database);
        Some(Tier {
            fixture_config,
            port: endpoint.port(),
            user,
            password,
            database,
            anchor,
        })
    }

    /// Everything the test reads off the provisioned tier.
    struct Tier {
        fixture_config: tokio_postgres::Config,
        port: u16,
        user: String,
        password: String,
        database: String,
        anchor: String,
    }

    /// Writes settings to a directory and returns the path.
    fn settings_tree(case: &str, base_yaml: &str) -> PathBuf {
        static CALLS: AtomicU32 = AtomicU32::new(0);
        let call = CALLS.fetch_add(1, Ordering::Relaxed);
        let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{case}-{}-{call}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a directory under the target dir is creatable");
        std::fs::write(dir.join("base.yaml"), base_yaml).expect("the settings file is writable");
        dir
    }

    /// Creates settings for the rdbms catalog test.
    fn settings(data_dir: &Path, tier: &Tier, documentation_schema: &str) -> PathBuf {
        let Tier {
            port,
            user,
            password,
            database,
            anchor,
            ..
        } = tier;
        let password_file = data_dir.join("password");
        std::fs::write(&password_file, format!("{password}\n")).expect("the password file is writable");
        settings_tree(
            "mcp-rdbms-boot-and-list",
            &format!(
                "security:\n  \
                   identity: \"single-user\"\n  \
                   single_user_because: \"a unit test reads its own fixture tier as one identity\"\n\
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
                 sources:\n  \
                   {SOURCE}:\n    \
                     kind: \"postgres\"\n    \
                     host: \"127.0.0.1\"\n    \
                     port: {port}\n    \
                     database: \"{database}\"\n    \
                     user: \"{user}\"\n    \
                     password_file: \"{password_file}\"\n    \
                     transport_mode: \"verified\"\n    \
                     transport_anchors: \"{anchor}\"\n    \
                     posture: \"shared-service-user\"\n",
                password_file = password_file.display(),
            ),
        )
    }

    /// An `rdbms`-kind deployment boots over the stdio MCP transport and lists the bundle it
    /// measures: the physical-schema guidance in `initialize.instructions` (which fires only when
    /// the bundle has models and zero metrics), `describe_catalog` advertised on `tools/list`,
    /// and a `describe_catalog` reply carrying zero metrics, the declared version and a digest.
    #[test]
    fn an_rdbms_catalog_boots_and_lists_over_stdio() {
        let Some(tier) = tier() else {
            return;
        };
        let data = DataDir::prepared("mcp-rdbms-boot-and-list");
        let documentation_schema = install_documentation_schema(&tier.fixture_config);
        let dir = settings(&data.0, &tier, &documentation_schema);

        let mut agent = spawn_configured(&dir);
        let result = agent.initialize();

        // The physical-schema guidance is rendered into `initialize.instructions` exactly when the
        // bundle has models and zero metrics (`physical_schema_guidance`'s own gate). A stub that
        // never read the dictionary would have no models, so this sentence would be absent - which
        // is why a fixed string here proves the dictionary loaded over THIS process's stdio path.
        assert!(
            result["instructions"]
                .as_str()
                .is_some_and(|text| text.contains("Physical structure is not a certified metric")),
            "the surface did not introduce itself with the physical-schema guidance an rdbms bundle carries: {result}"
        );

        // `describe_catalog` is on the advertised surface - the one capability a zero-metric
        // deployment still serves, beside `ask_metric`.
        let listed = agent.request("tools/list", &serde_json::json!({}));
        let advertised: Vec<&str> = listed["tools"]
            .as_array()
            .expect("tools/list returns an array")
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert!(
            advertised.contains(&Capability::DescribeCatalog.id()),
            "describe_catalog is not on the advertised surface: {advertised:?}"
        );

        // The catalog listing: zero certified metrics (the dictionary carries no measure), the
        // declared version, and a non-empty digest - the same three assertions the served-HTTP
        // cell makes, over the stdio transport.
        let reply = agent.call(Capability::DescribeCatalog, &serde_json::json!({}));
        let content = &reply["structuredContent"];
        assert_eq!(
            content["metrics"].as_array().map(Vec::len),
            Some(0),
            "an RDBMS dictionary carries no measure, so a stdio listing must carry none: {reply}"
        );
        assert_eq!(
            content["provenance"]["definition_version"], VERSION,
            "the stdio listing must be stamped with the declared version: {reply}"
        );
        assert!(
            content["provenance"]["definition_digest"]
                .as_str()
                .is_some_and(|digest| !digest.is_empty()),
            "the stdio listing carries no definition digest: {reply}"
        );

        assert!(agent.close().success(), "the process did not exit cleanly");
    }
}
