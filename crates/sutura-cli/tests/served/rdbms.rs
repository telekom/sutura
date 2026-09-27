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
//! `postgres` source. `PostgresWarehouse` takes the port's default `preflight` (`NotAsked`), so
//! the table's absence from the tier is a `NotReported` notice at boot rather than a refusal, and
//! the deployment starts without the physical table loaded - which is why this cell needs no
//! `FixtureLoadGuard` and no example CSV load.
//!
//! # The documentation-schema fixture
//!
//! A per-run schema holds a `columns` table with two rows describing `public.orders` (model
//! `orders`, one `order_id` primary-key column and one `amount` column), `environment: test`,
//! `is_deleted: false` - the documented fixed schema `PostgresReader` reads, the same shape
//! `crates/sutura-catalog-rdbms/tests/provisioned.rs` installs.
//!
//! # RED/GREEN
//!
//! The mutation this cell is meant to catch: corrupt the version `open_one_rdbms_catalog` stamps
//! into the catalog (the killing patch at
//! `devco/claim-mutations/an_rdbms_catalog_boots_and_lists_from_the_served_binary.patch` replaces
//! `settings.version().clone()` with a wrong `DefinitionVersion`), so the deployment boots and
//! `/catalog` responds but its `provenance.definition_version` no longer matches the declared
//! `VERSION` - the cell's own `assert_eq!` fires, RED. GREEN is this file as written. The
//! implementation is already on main (#1105), so this is a claim cell: the commit carries a
//! `Claim-Cell:` trailer and the patch above is the mechanism that holds it.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::harness::{LOOPBACK, SINGLE_USER, TOKEN, VERSION, config_path, derived_beside, start_configured, v1};

    /// The catalog's declared name.
    const CATALOG: &str = "dictionary";

    /// The source alias the dictionary rows' models bind to - the `postgres` source's own name.
    const SOURCE: &str = "warehouse";

    /// The declared environment key the fixture rows carry.
    const ENVIRONMENT: &str = "test";

    /// The service the provisioner is asked for - the same name `nix/postgres-tier.nix` publishes.
    const SERVICE: &str = "postgres";

    /// The tier's three credential exports.
    const USER: &str = "SUTURA_POSTGRES_TIER_USER";
    const PASSWORD: &str = "SUTURA_POSTGRES_TIER_PASSWORD";
    const DATABASE: &str = "SUTURA_POSTGRES_TIER_DB";
    const ANCHOR: &str = "SUTURA_POSTGRES_TIER_CA";

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

    /// What the fixture and the deployment need from the provisioned tier: a unix-socket config
    /// for the fixture install (`local all all trust`, no TLS), and the TCP port plus credentials
    /// for the served binary's own `verified` connections. `None` where nothing is provisioned
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
        // The fixture installs over the unix socket the nix tier publishes as its host, which the
        // driver treats a `/`-prefixed string as. `local all all trust` authenticates it, so no TLS
        // and no password read - the same connection `provisioned.rs` opens.
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

    /// Installs the documentation-schema fixture: a per-run schema with a `columns` table whose
    /// rows describe `public.orders` (model `orders`, one primary-key column and one `amount`
    /// column), bound to `ENVIRONMENT` and not soft-deleted.
    fn install_documentation_schema(config: &tokio_postgres::Config) -> String {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_nanos();
        let schema = format!("sutura_dictionary_{nonce}_{}", std::process::id());
        let statement = format!(
            "CREATE SCHEMA {schema}; \
             CREATE TABLE {schema}.columns ( \
               environment text NOT NULL, \
               catalog_name text, \
               schema_name text NOT NULL, \
               table_name text NOT NULL, \
               model_name text NOT NULL, \
               table_description text, \
               column_name text NOT NULL, \
               column_ordinal int NOT NULL, \
               column_type text, \
               column_description text, \
               is_primary_key boolean, \
               is_deleted boolean NOT NULL \
             ); \
             INSERT INTO {schema}.columns \
               (environment, schema_name, table_name, model_name, table_description, \
                column_name, column_ordinal, column_type, column_description, is_primary_key, is_deleted) \
             VALUES \
               ('{ENVIRONMENT}', 'public', 'orders', 'orders', 'Customer orders.', \
                'order_id', 1, 'bigint', 'The order key.', true, false), \
               ('{ENVIRONMENT}', 'public', 'orders', 'orders', 'Customer orders.', \
                'amount', 2, 'numeric', 'The order total.', false, false)"
        );
        run_sql(config, &statement);
        schema
    }

    fn run_sql(config: &tokio_postgres::Config, statement: &str) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime builds");
        let (client, connection) = runtime
            .block_on(config.connect(tokio_postgres::NoTls))
            .expect("the tier opens");
        runtime.spawn(async move {
            #[expect(
                clippy::let_underscore_must_use,
                clippy::let_underscore_untyped,
                reason = "the connection driver task's own error has no caller in this setup path"
            )]
            let _ = connection.await;
        });
        runtime
            .block_on(client.batch_execute(statement))
            .expect("the documentation-schema fixture installs");
    }

    /// The settings an `rdbms`-kind deployment needs: one `catalog.kind: rdbms` entry reading the
    /// documentation schema over the tier's loopback TLS connection, and one `kind: postgres`
    /// source named for the catalog's `source_alias` reaching the same tier.
    fn settings(
        data: &DataDir,
        port: u16,
        user: &str,
        password: &str,
        database: &str,
        anchor: &str,
        documentation_schema: &str,
    ) -> String {
        let password_file = data.0.join("password");
        std::fs::write(&password_file, format!("{password}\n")).expect("the password file is writable");
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
        )
    }

    /// An `rdbms`-kind deployment boots from the served binary and lists the bundle it measures:
    /// zero certified metrics (the dictionary carries no measure), the declared version, and a
    /// digest.
    #[test]
    fn an_rdbms_catalog_boots_and_lists_from_the_served_binary() {
        let case = "rdbms-boot-and-list";
        let Some(tier) = tier() else {
            return;
        };
        let data = DataDir::prepared(case);
        let documentation_schema = install_documentation_schema(&tier.fixture_config);
        let deployment = start_configured(
            case,
            &settings(
                &data,
                tier.port,
                &tier.user,
                &tier.password,
                &tier.database,
                &tier.anchor,
                &documentation_schema,
            ),
        );

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
        assert!(
            body["provenance"]["definition_digest"]
                .as_str()
                .is_some_and(|digest| !digest.is_empty()),
            "the served listing carries no definition digest: {}",
            reply.body
        );
    }
}
