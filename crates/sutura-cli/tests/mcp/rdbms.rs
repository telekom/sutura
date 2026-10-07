//! `catalog.kind: rdbms` over the stdio agent surface: a composed `sutura mcp` process boots over
//! a live Postgres documentation schema and lists the bundle it measures - #970's acceptance for
//! the MCP transport, which the served-HTTP cell (`served/rdbms.rs`, PR #1147) does not cover.
//!
//! # What this reuses, and what it does NOT prove
//!
//! The fixture is the documentation schema `served/rdbms.rs` installs, the same shape
//! `crates/sutura-catalog-rdbms/tests/provisioned.rs` reads: a per-run schema with a `columns`
//! table whose rows describe `public.orders`. The settings are the same `catalog.kind: rdbms` and
//! `kind: postgres` source pair, over the provisioned tier's verified TLS connection. The spawn
//! harness is this suite's own (`spawn_configured`/`Agent`). The fixture install, its constants
//! and the independently built bundle live in `tests/common/`, which both test binaries mount as
//! `crate::common`, so the two cells cannot drift onto different rows.
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
//! The first standard-error line is the serving notice, which `sutura mcp` prints only once the
//! catalog loaded and the `postgres` arm reached `serve` - a refusal is a `sutura: <why>` line in
//! its place. An rdbms dictionary yields structure and prose and no measure, so
//! `initialize.instructions` carries `physical_schema_guidance`'s fixed sentence, which is gated on
//! a non-empty `models()` and an empty `metrics()`. `describe_catalog` then returns the structured
//! listing, held to the same three assertions as the served cell: zero metrics, the declared
//! version, and the digest `common::expected` builds from the fixture rows.
//!
//! # RED/GREEN
//!
//! The killing patch at `devco/claim-mutations/an_rdbms_catalog_boots_and_lists_over_stdio.patch`
//! makes `sutura mcp`'s `Opened::Postgres` arm refuse instead of serving - the one seam no other
//! cell holds, since no other stdio cell declares a `postgres` source and the served cell never
//! reaches `src/mcp.rs`. The boot assertion fires, RED. GREEN is this file as written.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use sutura_app::Capability;

    use crate::common::{CATALOG, ENVIRONMENT, SOURCE, expected, install_documentation_schema};
    use crate::harness::{VERSION, settings_tree, spawn_configured};

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

    /// What the fixture and the deployment need from the provisioned tier: the unix socket the
    /// fixture installs over (`local all all trust`, no TLS), and the TCP port plus credentials
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
        Some(Tier {
            socket: String::from(endpoint.host()),
            port: endpoint.port(),
            user,
            password,
            database,
            anchor,
        })
    }

    /// Everything the test reads off the provisioned tier.
    struct Tier {
        socket: String,
        port: u16,
        user: String,
        password: String,
        database: String,
        anchor: String,
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
        let documentation_schema = install_documentation_schema(&tier.socket, tier.port);
        let dir = settings(&data.0, &tier, &documentation_schema);

        let mut agent = spawn_configured(&dir);
        let boot = agent.expect_log("sutura: ");
        assert!(
            boot.contains("grants every capability"),
            "the stdio root did not serve an rdbms catalog over a postgres source: {boot}"
        );
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
        // declared version, and the independently built digest - the same three assertions the
        // served-HTTP cell makes, over the stdio transport.
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
        assert_eq!(
            content["provenance"]["definition_digest"],
            expected(VERSION).digest().as_str(),
            "the stdio RDBMS bundle differs from the independently built fixture digest: {reply}"
        );

        assert!(agent.close().success(), "the process did not exit cleanly");
    }
}
