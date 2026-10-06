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
//! plaintext connection the tier authenticates by `trust`, through the shared connector the reader
//! dials with - which also installs the fixture, over the simple protocol. A catalog connection's
//! `mutual` TLS is not exercised: the tier dials its unix socket in plaintext, so that half needs a
//! live TLS server.

// `#[cfg(test)]` for the reason `sutura-exec-postgres/tests/conformance.rs` gives: clippy honours
// `allow-expect-in-tests` only under a literal `#[cfg(test)]` ancestor.
#[cfg(test)]
mod provisioned {
    use std::num::NonZeroU64;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use adbc_core::{Connection as _, Statement as _};
    use sutura_adbc_postgres::{AdbcError, Channel, ConnectionTarget, Conninfo, PostgresDriver};
    use sutura_catalog_rdbms::postgres_reader::{PostgresReader, RowPredicate};
    use sutura_catalog_rdbms::{DictionaryReader as _, RdbmsCatalog, RdbmsError};
    use sutura_dev::provisioned::{self, Provisioned};
    use sutura_domain::identity::Secret;
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

    /// The tier's unix socket over the exported credential - the driver and a way to build its
    /// connection string, one per reader - or `None` where nothing is provisioned (the `SKIPPED`
    /// notice is already on stderr). A provisioned tier names a driver, so its absence fails.
    fn tier() -> Option<Tier> {
        let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), SERVICE) {
            Provisioned::At(endpoint) => endpoint,
            Provisioned::Skipped(_) => return None,
        };
        Some(Tier {
            driver: PostgresDriver::from_host().expect("the tier is up, so a driver is named"),
            socket: String::from(endpoint.host()),
            port: endpoint.port(),
        })
    }

    struct Tier {
        driver: PostgresDriver,
        socket: String,
        port: u16,
    }

    impl Tier {
        /// The nix tier publishes its socket directory as a `/`-prefixed host, which libpq dials as
        /// a unix socket.
        fn conninfo(&self) -> Conninfo {
            let source = SourceName::parse("dictionary").expect("catalog name parses");
            let target = ConnectionTarget::Host(&self.socket);
            let password = Secret::new(required(PASSWORD));
            Conninfo::new(
                &source,
                target,
                self.port,
                &required(DATABASE),
                &required(USER),
                &password,
                Channel::Plaintext,
            )
            .expect("the tier's plaintext connection string builds")
        }

        fn reader(&self, schema: &str, row_cap: Option<NonZeroU64>, byte_cap: Option<NonZeroU64>) -> PostgresReader {
            let environment = String::from(ENVIRONMENT);
            PostgresReader::new(
                self.driver.clone(),
                self.conninfo(),
                String::from(schema),
                environment,
                RowPredicate::None,
                row_cap,
                byte_cap,
            )
            .expect("the fixture reader settings parse")
        }

        /// Runs every statement in `statement`, committed - the fixture's own DDL and rows.
        fn run_sql(&self, statement: &str) {
            let mut connection = self.driver.connect(&self.conninfo()).expect("the tier opens");
            let mut fixture = connection.new_statement().expect("a statement allocates");
            fixture.set_sql_query(statement).expect("the fixture text is accepted");
            #[expect(
                clippy::disallowed_methods,
                reason = "a fixture install: this file's own DDL and literal rows, never caller text"
            )]
            let installed = fixture.execute_update();
            installed.expect("the documentation-schema fixture installs");
        }
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
    fn install_fixture(tier: &Tier, schema: &str, read_only_schema: &str) {
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
        tier.run_sql(&statement);
    }

    #[test]
    fn a_live_documentation_schema_binds_models_to_the_declared_source_alias() {
        let Some(tier) = tier() else { return };
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_nanos();
        let schema = format!("{DOCUMENTATION_SCHEMA}_{}_{nonce}", std::process::id());
        let read_only_schema = format!("{schema}_ro");
        install_fixture(&tier, &schema, &read_only_schema);

        let catalog_name = SourceName::parse("dictionary").expect("catalog name parses");
        let source_alias = SourceName::parse("warehouse").expect("source alias parses");
        let version = DefinitionVersion::parse("dict-1").expect("version parses");

        // A read-only, plaintext reader over the tier's unix socket; the schema and environment are
        // this test's own validated literals, and the row/byte caps comfortably bound this fixture.
        let reader = tier.reader(&schema, NonZeroU64::new(1000), NonZeroU64::new(1 << 20));

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
            let reader = tier.reader(&schema, row_cap, byte_cap);
            assert_eq!(
                reader
                    .read_dictionary()
                    .expect_err("the declared cap refuses the stream")
                    .to_string(),
                expected,
            );
        }

        let reader = tier.reader(&read_only_schema, None, None);
        let error = reader
            .read_dictionary()
            .expect_err("a dictionary view cannot write during a read");
        let RdbmsError::Read(cause) = error else {
            panic!("expected a database read error")
        };
        let Some(AdbcError::Adbc(refused)) = cause.downcast_ref::<AdbcError>() else {
            panic!("expected the server's refusal: {cause:?}")
        };
        assert_eq!(
            refused.sqlstate.map(|c| u8::try_from(c).unwrap_or(0)),
            *b"25006",
            "read_only_sql_transaction: {refused:?}"
        );

        tier.run_sql(&format!(
            "UPDATE {schema}.columns SET is_primary_key = NULL WHERE column_name = 'amount'"
        ));
        assert_eq!(
            catalog.load().expect_err("null key evidence is malformed").to_string(),
            "reading the dictionary failed: a documentation row carried no primary-key evidence",
        );
        tier.run_sql(&format!(
            "UPDATE {schema}.columns SET is_primary_key = false WHERE column_name = 'amount'; \
             INSERT INTO {schema}.columns SELECT * FROM {schema}.columns WHERE column_name = 'amount'"
        ));
        assert_eq!(
            catalog.load().expect_err("a repeated column is malformed").to_string(),
            "reading the dictionary failed: a documentation table repeated column amount",
        );
    }

    #[test]
    fn a_live_reader_refuses_when_the_documentation_schema_is_absent() {
        let Some(tier) = tier() else { return };
        let error = tier
            .reader("no_such_schema", None, None)
            .read_dictionary()
            .expect_err("a missing documentation schema is a read error");
        assert!(matches!(error, RdbmsError::Read(_)), "{error:?}");
    }

    /// Installs the fixture under a fresh nonce-named schema and returns its name.
    fn fresh_fixture(tier: &Tier, tag: &str) -> String {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_nanos();
        let schema = format!("{DOCUMENTATION_SCHEMA}_{tag}_{}_{nonce}", std::process::id());
        install_fixture(tier, &schema, &format!("{schema}_ro"));
        schema
    }

    #[test]
    fn a_live_read_runs_at_repeatable_read() {
        let Some(tier) = tier() else { return };
        let schema = fresh_fixture(&tier, "iso");
        let probe = format!("{schema}_iso");
        tier.run_sql(&format!(
            "CREATE SCHEMA {probe}; \
             CREATE VIEW {probe}.columns AS \
             SELECT environment, catalog_name, schema_name, table_name, model_name, \
                    current_setting('transaction_isolation') AS table_description, \
                    column_name, column_ordinal, column_type, column_description, is_primary_key, is_deleted \
             FROM {schema}.columns"
        ));
        let dictionary = tier
            .reader(&probe, None, None)
            .read_dictionary()
            .expect("the probe view is readable");
        let [table] = dictionary.tables() else {
            panic!("one table: {dictionary:?}")
        };
        assert_eq!(table.description(), Some("repeatable read"));
    }

    #[test]
    fn the_limit_keeps_a_row_past_the_cap_out_of_the_byte_billing() {
        let Some(tier) = tier() else { return };
        let schema = fresh_fixture(&tier, "limit");
        let wide = format!("{schema}_wide");
        tier.run_sql(&format!(
            "CREATE SCHEMA {wide}; \
             CREATE TABLE {wide}.columns (LIKE {schema}.columns); \
             INSERT INTO {wide}.columns \
               (environment, schema_name, table_name, model_name, column_name, column_ordinal, \
                column_description, is_primary_key, is_deleted) \
             SELECT '{ENVIRONMENT}', 'public', 'orders', 'orders', 'c' || i, i, \
                    CASE WHEN i = 3 THEN repeat('x', 1048576) END, i = 1, false \
             FROM generate_series(1, 3) AS i"
        ));
        // Without the `LIMIT` the third (1 MiB) row reaches the batch and the BYTES cap refuses first.
        let error = tier
            .reader(&wide, NonZeroU64::new(1), NonZeroU64::new(256 * 1024))
            .read_dictionary()
            .expect_err("the second row is past the row cap");
        assert_eq!(
            error.to_string(),
            "reading the dictionary failed: the dictionary stream reached the declared maximum of 1 rows",
        );
    }
}
