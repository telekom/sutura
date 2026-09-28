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
//! The mutation this cell is meant to catch: drop every table description the reader decodes
//! (the killing patch at
//! `devco/claim-mutations/an_rdbms_catalog_boots_and_lists_from_the_served_binary.patch` filters
//! `table_description` to `None` in `PostgresReader::decode_row`), so the deployment boots and
//! `/catalog` responds but the served digest no longer matches the digest [`expected`] builds
//! independently from the same fixture rows - the cell's own `assert_eq!` fires, RED. GREEN is
//! this file as written. The implementation is already on main (#1105), so this is a claim cell:
//! the commit carries a `Claim-Cell:` trailer and the patch above is the mechanism that holds it.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
    use sutura_domain::catalog::{Column, Definitions, Description, Model};
    use sutura_domain::knowledge::{Knowledge, KnowledgeCapabilities};
    use sutura_domain::model::{ColumnName, ModelName, QualifiedTable, SourceName};
    use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

    use crate::harness::{
        DiscoveredTier, LOOPBACK, SINGLE_USER, TOKEN, VERSION, config_path, derived_beside, discover_tier, source_entry,
        start_configured, v1,
    };

    /// The catalog's declared name.
    const CATALOG: &str = "dictionary";

    /// The source alias the dictionary rows' models bind to - the `postgres` source's own name.
    const SOURCE: &str = "warehouse";

    /// The declared environment key the fixture rows carry.
    const ENVIRONMENT: &str = "test";

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

    /// The bundle this cell expects, built independently of the served binary from the same
    /// documentation-schema fixture [`install_documentation_schema`] writes - never from the served
    /// reply itself, so a reader that silently drops or corrupts what it measures (a description, a
    /// column, a type, the primary key) reddens this comparison, not only the "the field is present"
    /// checks a lone non-empty-digest assertion left standing.
    fn expected() -> PinnedDefinitions {
        let columns = [
            ("order_id", "bigint", "The order key."),
            ("amount", "numeric", "The order total."),
        ]
        .into_iter()
        .map(|(name, data_type, description)| {
            Column::from_metadata(
                ColumnName::parse(name).expect("a fixture column name parses"),
                Some(data_type),
                Some(description),
                None,
            )
            .expect("a fixture column description parses")
        });
        let model = Model::new(
            ModelName::parse("orders").expect("the fixture model name parses"),
            SourceName::parse(SOURCE).expect("the fixture source alias parses"),
            QualifiedTable::parse("public.orders").expect("the fixture table parses"),
            columns,
            Description::parse("Customer orders.").expect("the fixture description parses"),
        )
        .with_primary_key([ColumnName::parse("order_id").expect("the fixture primary-key column name parses")])
        .expect("the fixture primary key names one of the model's own columns");
        let declared = MetadataCapabilities::of(
            DefinitionCapabilities::of([DefinitionKind::Structure]).and_may_provide([
                DefinitionKind::Descriptions,
                DefinitionKind::Relationships,
                DefinitionKind::ColumnTypes,
                DefinitionKind::ColumnDescriptions,
            ]),
            KnowledgeCapabilities::none(),
        );
        PinnedDefinitions::pin(
            DefinitionVersion::parse(VERSION).expect("the served version parses"),
            Definitions::assemble(vec![model], vec![], vec![]).expect("the fixture definitions assemble"),
            Knowledge::none(),
            ContributionManifest::single(
                SourceName::parse(CATALOG).expect("the catalog name parses"),
                Contribution::of(declared),
            ),
        )
        .expect("the expected bundle hashes")
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
        let documentation_schema = install_documentation_schema(&tier.config);
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
}
