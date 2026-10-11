#![forbid(unsafe_code)]
#![expect(
    clippy::disallowed_methods,
    reason = "a test dials the driver directly to observe the driver itself; test code does not ship"
)]
//! The live Oracle documentation-schema reader against the compose `oracle` tier.
//!
//! `tests/provisioned.rs`'s twin for `crate::oracle_reader`: a fixture `columns` table in the tier
//! user's own schema describes a SEPARATELY DECLARED source alias, and an `RdbmsCatalog` over an
//! `OracleReader` is loaded through the driver's cursor, the reader's transaction and the driver's
//! `NUMBER` conversion - the read path the reader's unit cells cannot reach without a server.
//!
//! # Where this runs, and what it means when it does not
//!
//! Compose only, like `sutura-exec-oracle/tests/acceptance.rs`: `just oracle-acceptance` (by hand)
//! and `nix run .#oracle-acceptance` (the `oracle-tier` CI job) start the tier and run these
//! `#[ignore]`d cells, with `SUTURA_DEV_REQUIRE_TIER` set so an absent tier fails rather than
//! skips. A nix check has no docker socket, so a green `just validate` says nothing here.
//!
//! # What is NOT here
//!
//! No read-only probe: `SET TRANSACTION READ ONLY` stays unobserved, as the reader's header says.
//! One fixture user per tier, so two runs against the same tier race on the `columns` table.

// `#[cfg(test)]` for the reason `sutura-exec-postgres/tests/conformance.rs` gives: clippy honours
// `allow-expect-in-tests` only under a literal `#[cfg(test)]` ancestor.
#[cfg(test)]
mod oracle_provisioned {
    use std::collections::BTreeSet;
    use std::num::NonZeroU64;
    use std::path::Path;
    use std::time::{Duration, Instant};

    use sutura_catalog_rdbms::oracle_reader::{OracleLogin, OracleReader};
    use sutura_catalog_rdbms::postgres_reader::RowPredicate;
    use sutura_catalog_rdbms::{DictionaryReader as _, RdbmsCatalog, RdbmsError};
    use sutura_dev::provisioned;
    use sutura_domain::identity::Secret;
    use sutura_domain::model::{ColumnName, ModelName, SourceName};
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};

    /// The tier's pluggable database, the one `sutura-exec-oracle`'s acceptance cell dials.
    const SERVICE_NAME: &str = "FREEPDB1";

    /// The declared environment key the fixture rows carry.
    const ENVIRONMENT: &str = "test";

    /// The provisioned endpoint and the fixture user the acceptance task exports.
    struct Tier {
        host: String,
        port: u16,
        user: String,
        password: String,
    }

    impl Tier {
        fn here() -> Self {
            let endpoint = provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "oracle")
                .endpoint()
                .cloned()
                .expect("the named acceptance task requires an Oracle endpoint");
            Self {
                host: endpoint.host().to_owned(),
                port: endpoint.port(),
                user: std::env::var("SUTURA_DEV_USER").expect("the acceptance task sets SUTURA_DEV_USER"),
                password: std::env::var("SUTURA_DEV_PASSWORD").expect("the acceptance task sets SUTURA_DEV_PASSWORD"),
            }
        }

        fn connect(&self) -> oracledb::Connection {
            let config = oracledb::Config::default()
                .set_connect_string(&format!("{}:{}/{SERVICE_NAME}", self.host, self.port))
                .expect("the discovered endpoint is an Oracle connect string")
                .set_credentials(&self.user, &self.password);
            oracledb::connect(config).expect("the provisioned Oracle accepts the fixture credential")
        }

        /// A reader logged in as the fixture user over `schema`, which the reader folds to upper case.
        fn reader(&self, schema: &str, row_cap: Option<NonZeroU64>, byte_cap: Option<NonZeroU64>) -> OracleReader {
            let login = OracleLogin::new(
                self.host.clone(),
                self.port,
                String::from(SERVICE_NAME),
                self.user.clone(),
                Secret::new(self.password.clone()),
            );
            OracleReader::new(
                login,
                schema,
                String::from(ENVIRONMENT),
                RowPredicate::None,
                row_cap,
                byte_cap,
            )
            .expect("the fixture reader settings parse")
        }

        /// Waits, bounded, until a read-only transaction can read the freshly created table: for a
        /// short window after the DDL Oracle refuses that snapshot with `ORA-01466`, as it would the
        /// reader's own. A fresh session per probe, because the driver drops one that hit the error.
        fn await_read_only_snapshot(&self) {
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                let probe = self.connect();
                probe
                    .execute("SET TRANSACTION READ ONLY", &[])
                    .expect("the probe transaction starts");
                match probe.query_row("SELECT COUNT(*) FROM \"COLUMNS\"", &[]) {
                    Ok(_) => return,
                    Err(cause) if cause.to_string().contains("ORA-01466") && Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    Err(cause) => panic!("the fixture never became readable under a read-only snapshot: {cause}"),
                }
            }
        }
    }

    /// Replaces the user's own `COLUMNS` table: `public.orders` bound to the model `orders`, with
    /// `order_id` as its key, `amount`, a soft-deleted `archived` row and a row of another
    /// environment. Committed, because the reader reads in a session of its own.
    fn install_fixture(connection: &oracledb::Connection) {
        if let Err(cause) = connection.execute("DROP TABLE \"COLUMNS\" PURGE", &[]) {
            assert!(
                cause.to_string().contains("ORA-00942"),
                "the previous fixture did not drop: {cause}"
            );
        }
        connection
            .execute(
                "CREATE TABLE \"COLUMNS\" (environment VARCHAR2(64) NOT NULL, catalog_name VARCHAR2(64), \
                 schema_name VARCHAR2(64) NOT NULL, table_name VARCHAR2(64) NOT NULL, \
                 model_name VARCHAR2(64) NOT NULL, table_description VARCHAR2(256), \
                 column_name VARCHAR2(64) NOT NULL, column_ordinal NUMBER(10) NOT NULL, \
                 column_type VARCHAR2(64), column_description VARCHAR2(256), \
                 is_primary_key NUMBER(1), is_deleted NUMBER(1) NOT NULL)",
                &[],
            )
            .expect("the fixture table creates");
        for (environment, column, ordinal, key, deleted) in [
            (ENVIRONMENT, "order_id", 1, 1, 0),
            (ENVIRONMENT, "amount", 2, 0, 0),
            (ENVIRONMENT, "archived", 3, 0, 1),
            ("prod", "prod_only", 4, 0, 0),
        ] {
            let statement = format!(
                "INSERT INTO \"COLUMNS\" (environment, schema_name, table_name, model_name, table_description, \
                 column_name, column_ordinal, column_type, is_primary_key, is_deleted) \
                 VALUES ('{environment}', 'public', 'orders', 'orders', 'Customer orders.', \
                 '{column}', {ordinal}, 'NUMBER', {key}, {deleted})"
            );
            connection.execute(&statement, &[]).expect("a fixture row inserts");
        }
        connection.commit().expect("the fixture commits");
    }

    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn a_live_oracle_dictionary_binds_models_to_the_declared_source_alias() {
        let tier = Tier::here();
        let setup = tier.connect();
        install_fixture(&setup);
        tier.await_read_only_snapshot();

        let catalog_name = SourceName::parse("dictionary").expect("catalog name parses");
        let source_alias = SourceName::parse("warehouse").expect("source alias parses");
        let version = DefinitionVersion::parse("dict-1").expect("version parses");
        let reader = tier.reader(&tier.user, NonZeroU64::new(1000), NonZeroU64::new(1 << 20));
        let catalog = RdbmsCatalog::new(catalog_name.clone(), version, reader).with_source_alias(source_alias.clone());

        let pinned = catalog.load().expect("the live Oracle dictionary loads");
        let orders = pinned
            .definitions()
            .model(&ModelName::parse("orders").expect("model name parses"))
            .expect("the fixture has orders");
        assert_eq!(orders.source(), &source_alias, "models bind to the declared alias");
        let column = |name: &str| ColumnName::parse(name).expect("fixture column name parses");
        assert!(orders.column(&column("amount")).is_some(), "a live row becomes a column");
        assert!(orders.column(&column("archived")).is_none(), "`is_deleted = 1` is not read");
        assert_eq!(
            orders.primary_key(),
            &BTreeSet::from([column("order_id")]),
            "`NUMBER(1)` key evidence decodes"
        );
        assert!(
            pinned.manifest().get(&catalog_name).is_some(),
            "the manifest is under the catalog"
        );
        assert!(
            pinned.manifest().get(&source_alias).is_none(),
            "the alias contributes nothing"
        );

        // Two live rows: a one-row cap refuses at the second row, a one-byte cap at the first.
        for (row_cap, byte_cap, unit) in [(1, 1 << 20, "rows"), (1000, 1, "bytes")] {
            let capped = tier.reader(&tier.user, NonZeroU64::new(row_cap), NonZeroU64::new(byte_cap));
            assert_eq!(
                capped.read_dictionary().expect_err("the declared cap refuses").to_string(),
                format!("reading the dictionary failed: the dictionary stream reached the declared maximum of 1 {unit}"),
            );
        }

        setup
            .execute(
                "UPDATE \"COLUMNS\" SET is_primary_key = NULL WHERE column_name = 'amount'",
                &[],
            )
            .expect("the key evidence clears");
        setup.commit().expect("the cleared evidence commits");
        assert_eq!(
            catalog.load().expect_err("a NULL flag is no evidence").to_string(),
            "reading the dictionary failed: a documentation row carried no primary-key evidence",
        );
    }

    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn a_live_oracle_reader_refuses_when_the_documentation_schema_is_absent() {
        let error = Tier::here()
            .reader("no_such_schema", None, None)
            .read_dictionary()
            .expect_err("a missing documentation schema is a read error");
        assert!(
            matches!(&error, RdbmsError::Read(_)) && error.to_string().contains("ORA-00942"),
            "the server, not the login, refused the missing view: {error:?}"
        );
    }
}
