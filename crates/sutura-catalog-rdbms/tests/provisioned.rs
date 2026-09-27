#![forbid(unsafe_code)]
//! The live Postgres documentation-schema reader against the provisioned tier.
//!
//! This is where the reader (`crate::postgres_reader`) stops being a construction and becomes a
//! real read: a fixture documentation schema is created in the tier's Postgres whose rows describe
//! a SEPARATELY DECLARED source alias, and a [`crate::RdbmsCatalog`] over that reader is loaded and
//! its bundle is asserted to bind its models to the alias while the contribution manifest stays
//! under the catalog's own name - the split this change's `source_alias` separation is built for.
//!
//! # Where this runs, and what it means when it does not
//!
//! `sutura_dev::provisioned::here` reads the worktree's endpoint file and panics where a tier is
//! required, and writes its `SKIPPED` notice where none is. Every venue that runs the suite
//! provisions the tier (`just test`, `just gates`, `just causality`, `checks.nextest`), so these
//! cells RUN there; on a host without the tier binary they skip, the same direction every other
//! tier-backed cell in this workspace takes.
//!
//! # What is NOT here
//!
//! No foreign keys - this first slice declares the documentation schema carries none and never
//! invents one, which the unit-level contract already states. The TLS cells live in
//! `sutura-exec-postgres`'s own `tests/tls.rs` against the same tier; this suite checks the reader's
//! channel shape (read-only transaction, parameter binding, documented schema) over the unix-socket
//! plaintext connection the tier authenticates by `trust`.

// `#[cfg(test)]` for the reason `sutura-exec-postgres/tests/conformance.rs` gives: clippy honours
// `allow-expect-in-tests` only under a literal `#[cfg(test)]` ancestor.
#[cfg(test)]
mod provisioned {
    use std::num::NonZeroU64;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use sutura_catalog_rdbms::postgres_reader::{PostgresReader, RowPredicate};
    use sutura_catalog_rdbms::{DictionaryReader as _, RdbmsCatalog, RdbmsError};
    use sutura_dev::provisioned::{self, Provisioned};
    use sutura_domain::model::{ColumnName, ModelName, SourceName};
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};

    /// The service the provisioner is asked for - the same name `nix/postgres-tier.nix` publishes.
    const SERVICE: &str = "postgres";

    /// The tier's three credential exports, beside the CA/client pair `tests/tls.rs` reads there.
    const USER: &str = "SUTURA_POSTGRES_TIER_USER";
    const PASSWORD: &str = "SUTURA_POSTGRES_TIER_PASSWORD";
    const DATABASE: &str = "SUTURA_POSTGRES_TIER_DB";

    /// Prefix for the per-run documentation schema this test creates and reads back.
    const DOCUMENTATION_SCHEMA: &str = "dictionary";

    /// The declared environment key the fixture rows carry.
    const ENVIRONMENT: &str = "test";

    /// A connection config for the tier's unix socket, over the exported credential, or `None`
    /// where nothing is provisioned (the `SKIPPED` notice is already on stderr).
    fn tier_config() -> Option<tokio_postgres::Config> {
        let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), SERVICE) {
            Provisioned::At(endpoint) => endpoint,
            Provisioned::Skipped(_) => return None,
        };
        let mut config = tokio_postgres::Config::new();
        // The nix tier publishes its socket directory as a `/`-prefixed host; the driver treats
        // that as a unix socket directory.
        config.host(endpoint.host()).port(endpoint.port());
        config
            .user(required(USER))
            .password(required(PASSWORD))
            .dbname(required(DATABASE));
        Some(config)
    }

    fn required(variable: &str) -> String {
        let value = std::env::var(variable).expect("the provisioned tier exports its credential");
        assert!(!value.trim().is_empty(), "the provisioned tier exported an empty credential");
        value
    }
    /// Creates the documentation-schema fixture: the `columns` table and its live rows.
    ///
    /// Rows describe a physical table `public.orders` (one `order_id` primary-key column and one
    /// `amount` column), bound to the semantic model `orders` in `ENVIRONMENT`. The described
    /// objects' source alias is NOT the catalog name - the reader's caller declares a separate
    /// alias, and the bundle must bind models to it.
    fn install_fixture(config: &tokio_postgres::Config, schema: &str, read_only_schema: &str) {
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
                'amount', 2, 'numeric', 'The order total.', false, false), \
               ('{ENVIRONMENT}', 'public', 'orders', 'orders', 'Customer orders.', \
                'archived', 3, 'boolean', 'A soft-deleted order.', false, true); \
             CREATE SCHEMA {read_only_schema}; \
             CREATE FUNCTION {read_only_schema}.probe() RETURNS SETOF {schema}.columns \
             LANGUAGE plpgsql AS $$ BEGIN \
               INSERT INTO {schema}.columns \
                 (environment, schema_name, table_name, model_name, column_name, column_ordinal, is_primary_key, is_deleted) \
               VALUES ('{ENVIRONMENT}', 'public', 'orders', 'orders', 'unexpected', 4, false, false); \
               RETURN QUERY SELECT * FROM {schema}.columns; \
             END $$; \
             CREATE VIEW {read_only_schema}.columns AS SELECT * FROM {read_only_schema}.probe()"
        );
        run_sql(config, &statement);
    }

    fn run_sql(config: &tokio_postgres::Config, statement: &str) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime builds");
        let (client, connection) = runtime
            .block_on(config.clone().connect(tokio_postgres::NoTls))
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

    #[test]
    fn a_live_documentation_schema_binds_models_to_the_declared_source_alias() {
        let Some(config) = tier_config() else { return };
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_nanos();
        let schema = format!("{DOCUMENTATION_SCHEMA}_{}_{nonce}", std::process::id());
        let read_only_schema = format!("{schema}_ro");
        install_fixture(&config, &schema, &read_only_schema);

        let catalog_name = SourceName::parse("dictionary").expect("catalog name parses");
        let source_alias = SourceName::parse("warehouse").expect("source alias parses");
        let version = DefinitionVersion::parse("dict-1").expect("version parses");

        // A read-only, plaintext reader over the tier's unix socket. `tls = None` for the declared
        // `plaintext` channel; the schema and environment are this test's own validated literals;
        // and the row/byte caps comfortably bound this fixture.
        let reader = PostgresReader::new(
            config.clone(),
            None,
            schema.clone(),
            String::from(ENVIRONMENT),
            RowPredicate::None,
            NonZeroU64::new(1000),
            NonZeroU64::new(1 << 20),
        )
        .expect("the fixture reader settings parse");

        let catalog = RdbmsCatalog::new(catalog_name.clone(), version, reader).with_source_alias(source_alias.clone());
        let pinned = catalog.load().expect("the live dictionary loads");
        let orders = ModelName::parse("orders").expect("model name parses");
        assert_eq!(
            pinned.definitions().model(&orders).expect("the fixture has orders").source(),
            &source_alias,
            "models bind to the declared source alias, not to the catalog name"
        );
        let archived = ColumnName::parse("archived").expect("fixture column name parses");
        assert!(
            pinned
                .definitions()
                .model(&orders)
                .expect("the fixture has orders")
                .column(&archived)
                .is_none(),
            "soft-deleted dictionary rows do not become model columns"
        );
        assert!(
            pinned.manifest().get(&catalog_name).is_some(),
            "the manifest is under the catalog name"
        );
        assert!(
            pinned.manifest().get(&source_alias).is_none(),
            "the source alias is not a contributor to this catalog's manifest"
        );
        assert_eq!(
            pinned.digest(),
            catalog.load().expect("the same live dictionary loads again").digest(),
            "the live declaration has a stable digest"
        );

        for (row_cap, byte_cap, expected) in [
            (
                NonZeroU64::new(1),
                NonZeroU64::new(1 << 20),
                "reading the dictionary failed: the dictionary stream reached the declared maximum of 1 rows",
            ),
            (
                NonZeroU64::new(1000),
                NonZeroU64::new(1),
                "reading the dictionary failed: the dictionary stream reached the declared maximum of 1 bytes",
            ),
        ] {
            let reader = PostgresReader::new(
                config.clone(),
                None,
                schema.clone(),
                String::from(ENVIRONMENT),
                RowPredicate::None,
                row_cap,
                byte_cap,
            )
            .expect("the capped reader settings parse");
            assert_eq!(
                reader
                    .read_dictionary()
                    .expect_err("the declared cap refuses the stream")
                    .to_string(),
                expected,
            );
        }

        let reader = PostgresReader::new(
            config.clone(),
            None,
            read_only_schema,
            String::from(ENVIRONMENT),
            RowPredicate::None,
            None,
            None,
        )
        .expect("the read-only probe settings parse");
        let error = reader
            .read_dictionary()
            .expect_err("a dictionary view cannot write during a read");
        let RdbmsError::Read(cause) = error else {
            panic!("expected a database read error")
        };
        let database_error = cause
            .downcast_ref::<tokio_postgres::Error>()
            .and_then(tokio_postgres::Error::as_db_error)
            .expect("the server refused the write");
        assert_eq!(
            database_error.code(),
            &tokio_postgres::error::SqlState::READ_ONLY_SQL_TRANSACTION,
        );

        run_sql(
            &config,
            &format!("UPDATE {schema}.columns SET is_primary_key = NULL WHERE column_name = 'amount'"),
        );
        assert_eq!(
            catalog.load().expect_err("null key evidence is malformed").to_string(),
            "reading the dictionary failed: a documentation row carried no primary-key evidence",
        );
        run_sql(
            &config,
            &format!(
                "UPDATE {schema}.columns SET is_primary_key = false WHERE column_name = 'amount'; \
                 INSERT INTO {schema}.columns SELECT * FROM {schema}.columns WHERE column_name = 'amount'"
            ),
        );
        assert_eq!(
            catalog.load().expect_err("a repeated column is malformed").to_string(),
            "reading the dictionary failed: a documentation table repeated column amount",
        );
    }

    #[test]
    fn a_live_reader_refuses_when_the_documentation_schema_is_absent() {
        let Some(config) = tier_config() else { return };
        let reader = PostgresReader::new(
            config,
            None,
            String::from("no_such_schema"),
            String::from(ENVIRONMENT),
            RowPredicate::None,
            None,
            None,
        )
        .expect("the missing-schema reader settings parse");
        let error = reader
            .read_dictionary()
            .expect_err("a missing documentation schema is a read error");
        assert!(matches!(error, RdbmsError::Read(_)), "{error:?}");
    }
}
